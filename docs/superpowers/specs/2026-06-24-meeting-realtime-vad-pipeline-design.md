# 会议模式实时 VAD 流水线设计

**日期**: 2026-06-24
**状态**: 已确认（设计阶段）
**目标**: 彻底解决离线 SenseVoice 引擎在会议模式下"长录音转写卡死"的问题。

---

## 1. 背景与根因

### 问题现象
离线模型（SenseVoice）的会议模式，录制 20-30 分钟后，点击停止、转文字时直接卡死。

### 根因（已通过代码追踪确认）
当前会议模式把**整场录音的音频全部缓冲到内存**，停止时**一次性**送进 SenseVoice ONNX 推理：

- `sensevoice/provider.rs:320` 的 collector 线程把所有音频 `extend_from_slice` 进一个 `Vec<f32>`，**永不分段**
- `sensevoice/provider.rs:349` `finish_and_wait` 取出整段 buffer，`run_inference` 整段喂给 ONNX

**规模测算（30 分钟会议）**：
- 48kHz 录音 → 重采样 16k 后 1,728,000 个 f32 样本
- Fbank → ~10,800 帧 → LFR 后输入张量 T ≈ 1800，形状 `[1, 1800, 560]`
- SenseVoice 是 Transformer，自注意力复杂度 **O(T²)**，T=1800 时注意力矩阵 324万元素
- 模型最大上下文约 30 秒，超出即 OOM 或极度缓慢

dictation（听写）模式不受影响，因为它是短录音（几秒~几十秒）。

### 业界最佳实践（已调研确认）
sherpa-onnx 官方、FunASR、生产级服务器用的是**同一个标准架构**：

```
音频流(16k) ─► Silero VAD 端点检测 ─► 语音段 ─► SenseVoice 分段推理 ─► 文本拼接
```

- **Silero VAD**（ONNX，~2MB）是端点检测的绝对标准
- VAD 的职责就是把长音频切分成短片段，喂给非流式 ASR
- SenseVoice 单段推理 ~60-100ms（量化版 60ms，4核8G Windows）

---

## 2. 解决方案总览

引入一个独立的 `MeetingPipeline` 层，在 SenseVoice provider 之上，负责"VAD 分段 + 边录边转"。

### 三层职责分离

| 层 | 职责 | 改动 |
|---|---|---|
| **MeetingAudioCapture**（`meeting/audio.rs`） | 采集 mic+loopback，输出 48k f32 chunks | **基本不变** |
| **MeetingPipeline**（新建） | ① 重采样 48k→16k ② VAD 检测语音端点 ③ 切出语音段 ④ 调度 SenseVoice 推理 ⑤ 累积全文 + segment 时间戳 | **新建** |
| **SenseVoiceProvider**（`sensevoice/provider.rs`） | 单段音频 → 文本 | **零改动** |

### 为什么手写 VAD 推理器而非引入第三方 crate
- 项目固定使用 `ort = 2.0.0-rc.11`（`load-dynamic` 特性）。第三方 VAD crate（如 `silero-vad-rs`）依赖的 ort 版本不一定匹配，引入有版本冲突风险
- 手写 Silero VAD 推理逻辑极简（一个 stateful RNN，输入 16k 音频块，输出语音概率），完全可控
- 复用 SenseVoice 已初始化的同一个 ONNX Runtime 全局环境，零额外运行时依赖
- 模型用同样的下载机制管理

---

## 3. 详细设计

### 3.1 VAD 模块（`asr/sensevoice/vad.rs`，新建）

手写 Silero VAD 的 ONNX 推理。

**模型选型 — 无显式 state 输入的导出版**：

经核实（参考 `silero-vad-rs` crate 的 `model.rs` 实现），我们采用**无显式 LSTM state 输入**的 Silero VAD 导出版。这类导出（如 `silero_vad` 的简洁版）只有单个 `input` 张量，通过 **context 前缀**技巧在外部维护跨块连续性，无需管理 `h`/`c`/`state` 张量，手写最简单可控。

**模型契约**：
- 唯一输入：`input`，形状 `[1, 576]`（= 64 context + 512 当前音频块）
- 唯一输出：语音概率 `[1]`（0.0~1.0）
- **跨块状态维护**：每次推理前，把上一次喂入的 512 块的**最后 64 个样本**作为前缀拼到当前 512 块前，组成 576 长度输入。首块 context 补零。这样模型通过滑窗 context 近似维护状态。
- 固定块长：**512 采样**（32ms@16k），16kHz 单声道
- 这样每个 512 块产生一个概率值，对应块末尾时刻

**说明**：这比 v4 的 stateful 版（需管理 `[2,1,64]` 的 h/c 张量并传回模型）实现简单得多，且精度对于"会议分段"目的完全足够——我们只需要可靠的语音端点，不需要逐帧精确。

**公开接口**：
```rust
pub struct SileroVad {
    session: Mutex<Session>,
    context: Mutex<Vec<f32>>,   // 64 个样本的跨块 context，首块为全 0
}

pub struct VadSegment {
    pub samples: Vec<f32>,   // 16kHz 单段音频
    pub start_sample: u64,   // 相对流起始的样本偏移（16k 域）
    pub end_sample: u64,
}

impl SileroVad {
    pub fn try_new(model_path: &Path) -> Result<Self>;
    /// 喂入一个 512 采样块，返回该块的语音概率。
    /// 内部用 context 前缀组装 [1,576] 输入，并更新 context。
    pub fn process_chunk(&self, chunk_512: &[f32]) -> Result<f32>;
    /// 重置 context（新会话时调用）。
    pub fn reset(&self);
}
```

**关键参数（业界经验值）**：
- 固定块长：**512 采样**（32ms@16k）——Silero v4 硬性要求
- `threshold`（语音概率阈值）：**0.5**
- `min_silence_duration_ms`（触发切分的最小静音）：**500ms**
- `speech_pad_ms`（段首段尾各补静音）：**200ms**，保证句尾不截断
- 最大段长兜底：**30 秒**（即使没检测到静音也强制切，防超长）

**模型分发**：随应用打包内置（~2MB），首次使用时从 ModelScope 下载到与 SenseVoice 同级的目录，复用现有下载机制。

### 3.2 VAD 端点检测调度器（`asr/sensevoice/vad.rs` 内）

状态机，把 VAD 概率流转换为语音段流：

```
状态: Silent ⇄ Speech
- Silent 状态收到 prob >= threshold → 进入 Speech，记录段起始
- Speech 状态收到 prob < threshold 持续 >= min_silence_duration_ms → 切出段，回 Silent
- Speech 持续 >= max_segment_ms(30s) → 强制切出段，重置
```

**接口**：
```rust
pub struct VadEndpointer {
    vad: Arc<SileroVad>,
    threshold: f32,
    min_silence_samples: u64,   // min_silence_duration_ms 换算
    speech_pad_samples: u64,
    max_segment_samples: u64,
    // 运行状态
    in_speech: bool,
    current_segment: Vec<f32>,
    segment_start_sample: u64,
    silence_since: Option<u64>,
    total_samples_seen: u64,
    temp_chunk_buffer: Vec<f32>,  // 累积到 512 采样再喂 VAD
}

impl VadEndpointer {
    pub fn new(vad: Arc<SileroVad>) -> Self;
    /// 喂入任意长度 16k 音频，返回 0 个或多个已完成的语音段。
    /// （内部按 512 采样分块喂 VAD，按端点规则切出完整段）
    pub fn feed(&mut self, samples: &[f32]) -> Vec<VadSegment>;
    /// flush 所有缓冲（停止时调用，把进行中的段强制吐出）
    pub fn flush(&mut self) -> Vec<VadSegment>;
}
```

### 3.3 会议流水线（`meeting/pipeline.rs`，新建）

把"重采样 + VAD 端点 + 分段推理 + 文本累积"串起来。

```rust
pub struct MeetingPipeline {
    resampler: MeetingResampler,        // 48k → 16k
    endpointer: VadEndpointer,          // VAD 端点检测
    provider: Arc<SenseVoiceProvider>,  // 单段推理（复用现有）
    // 累积结果
    full_text: String,
    segments: Vec<TranscriptSegment>,
    sample_offset_to_ms: f64,           // 16k 样本 → 毫秒
    on_segment: Box<dyn Fn(String, String) + Send>,  // (累积全文, 本次新增段文本)
}
```

**工作线程模型**：
```
[采集线程] ──48k chunks──► [pipeline feeder 线程]
                              │
                              ├─ 重采样 16k
                              ├─ endpointer.feed() → Vec<VadSegment>
                              ├─ 对每个段: provider.run_segment() → 文本
                              ├─ 追加到 full_text / segments
                              └─ on_segment(full_text, new_segment) 回调
                                   └─► meeting_partial 事件 → 前端
```

**关键点：feeder 线程在采集线程之外**。VAD + 推理都是 CPU 工作，不能阻塞 cpal/WASAPI 采集回调（否则丢音频）。feeder 从 `audio_rx` 拉数据，处理完推送结果。

**时间戳**：每个 `VadSegment` 带 `start_sample`/`end_sample`（16k 域），换算成 `start_ms`/`end_ms` 填入 `TranscriptSegment`。修正现状（全填 0）的问题。

**停止流程**：
```
1. capture.stop() → sender drop → audio_rx EOF
2. feeder 线程检测 EOF → endpointer.flush() 吐出最后一段 → 推理完 → 退出
3. join feeder 线程
4. 返回累积的 full_text + segments
```

### 3.4 SenseVoice 单段推理接口

现有 `run_inference` 是 `provider.rs` 内的私有函数。需要暴露一个公开的单段推理入口供 pipeline 调用：

```rust
// sensevoice/provider.rs 新增
impl SenseVoiceProvider {
    /// 对单段 16kHz 音频做推理，返回文本。
    /// pipeline 复用此方法。
    pub fn transcribe_segment(&self, samples_16k: &[f32]) -> Result<String> {
        run_inference(&self.inner, samples_16k)
    }
}
```

`run_inference` 已存在且逻辑完整，只需把它从私有提升为通过公开方法暴露。SenseVoice session 的 `Mutex` 保证分段推理不会并发冲突。

### 3.5 VAD 模型下载与管理

**模型**：`silero_vad.onnx`（~2MB，v5 版本）
**下载源**：ModelScope 或 HuggingFace `snakers4/silero-vad`
**存储路径**：与 SenseVoice 模型同级，`<model_dir>/vad/silero_vad.onnx`
**下载机制**：复用 `sensevoice/download.rs` 的流式 + 进度事件机制，封装一个独立的 `download_vad_model` 命令
**存在性检查**：`vad::is_present(dir)` 类比 `model::is_present`

**首次使用策略**：会议模式启动时检查 VAD 模型是否存在，不存在则提示下载（复用现有 `asr_model_download` 事件机制）。

### 3.6 meeting/session.rs 改造

`ActiveMeeting` 持有 `MeetingPipeline` 而非 `StreamingSession`：

```rust
pub struct ActiveMeeting {
    // ... 既有字段
    pipeline: MeetingPipeline,   // 替代 asr_session: StreamingSession
}
```

`start_meeting`：
- 构造 VAD（加载模型）
- 构造 `MeetingPipeline`，传入 SenseVoice provider 引用
- spawn feeder 线程消费 `audio_rx`

`ActiveMeeting::stop`：
- `capture.stop()`
- 等待 feeder 线程 flush + 退出（join）
- 从 pipeline 取出 `full_text` + `segments`
- 构造 `MeetingRecord`

### 3.7 配置

`SenseVoiceOnnxConfig` 增加可选的 VAD 相关字段（带 serde default，向后兼容）：

```rust
pub struct SenseVoiceOnnxConfig {
    // ... 既有字段
    #[serde(default)]
    pub vad_threshold: f32,              // 默认 0.5
    #[serde(default)]
    pub vad_min_silence_ms: u32,         // 默认 500
}
```

无配置文件时用默认值，用户无需任何配置即可使用。

---

## 4. 数据流（完整）

```
用户点击"开始会议"
  │
  ▼
start_meeting()
  ├─ 加载 VAD 模型（不存在则触发下载）
  ├─ 构造 MeetingPipeline（resampler + endpointer + provider）
  ├─ MeetingAudioCapture.start() → audio_rx
  └─ spawn feeder 线程:
       loop {
         audio_rx.recv() → 48k chunk
         │ 重采样 → 16k chunk
         │ endpointer.feed(16k) → Vec<VadSegment>
         │ for seg in segments {
         │   provider.transcribe_segment(seg.samples) → text
         │   full_text += text
         │   segments.push({start_ms, end_ms, text})
         │ }
         └─ on_segment(full_text) → emit meeting_partial
       }
  │
  ▼ （用户说话，feeder 持续产出段 + 文本，前端实时显示）
  │
用户点击"停止会议"
  │
  ▼
stop_meeting()
  ├─ capture.stop() → audio_rx EOF
  ├─ feeder 线程: EOF → endpointer.flush() → 最后一段推理 → 退出
  ├─ join feeder
  ├─ pipeline.take_result() → (full_text, segments)
  └─ 构造 + 落库 MeetingRecord → emit meeting_finalized
```

**内存特性**：恒定占用。只保留当前正在积累的语音段（最多 30s × 16k × 4B ≈ 2MB）+ LSTM 状态，不再累积整场音频。

---

## 5. 错误处理

| 场景 | 处理 |
|---|---|
| VAD 模型文件缺失 | 启动时 `is_present` 失败 → 返回明确错误提示下载 |
| VAD 推理单块出错 | 记录日志，跳过该块（不中断会议） |
| 单段 SenseVoice 推理出错 | 记录日志，该段文本为空，继续下一段（不中断会议） |
| feeder 线程 panic | join 时检测，stop 返回错误 + 已积累的 partial 文本兜底 |
| onnxruntime.dll 缺失 | SenseVoice provider 加载阶段就失败，用户在开始会议前就能看到 |

**降级**：任何单段失败都不影响整场会议，已转写部分保留。这比现状（整段失败则全丢）健壮得多。

---

## 6. 测试策略

### 6.1 单元测试（`#[cfg(test)]`，无需真实模型）

**VAD 模块**：
- `SileroVad::process_chunk`：mock ONNX session 不现实，改为**纯逻辑层** `VadEndpointer` 用假概率序列测试状态机
  - 静音段不产出 segment
  - 语音段 + 静音 → 切出一个 segment
  - 连续语音超 30s → 强制切
  - speech_pad 正确应用
  - flush 吐出进行中的段
- `resample_to_16k`：正弦波已知频率，重采样后频率匹配

**pipeline 文本累积**：
- 模拟连续喂入段 → full_text 正确拼接
- 时间戳换算正确（样本 → 毫秒）

### 6.2 集成测试（需真实模型，标 `#[ignore]` 手动运行）

- 用一段真实会议录音 WAV（或生成测试音频），跑完整 pipeline，验证：
  - 不卡死（核心回归测试）
  - 产出非空文本
  - segments 有合理时间戳

### 6.3 回归验证

- dictation 模式（短录音）行为不变：SenseVoice provider 零改动保证
- 30 分钟模拟长录音：内存占用恒定、停止后秒级出全文、不卡死

---

## 7. 实现顺序（供后续 plan 参考）

1. `asr/sensevoice/vad.rs`：SileroVad 推理器 + VadEndpointer 状态机 + 单元测试
2. `asr/sensevoice/download.rs` + `model.rs`：VAD 模型下载/管理（复用机制）
3. `asr/sensevoice/provider.rs`：暴露 `transcribe_segment` 公开方法
4. `meeting/pipeline.rs`：MeetingPipeline（重采样 + VAD + 推理 + 累积）
5. `meeting/session.rs`：改 ActiveMeeting 用 pipeline
6. 前端/命令层：VAD 模型下载命令、首次使用提示
7. 集成测试 + 回归验证

---

## 8. 范围之外（YAGNI）

- **说话人分离（diarization）**：当前不做，`speaker` 字段保持 None。VAD 段是自然的说话轮次单元，未来加分轨很容易
- **流式 VAD 阈值自适应**：用固定阈值 + 合理默认值，不引入自适应复杂度
- **标点后处理模型**：SenseVoice 自带 ITN/标点（`TEXT_NORM_WITH_ITN_ID`），不额外加标点模型
- **dictation 模式改造**：短录音不需要 VAD，保持现状
