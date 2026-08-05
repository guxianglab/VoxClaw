<div align="center">

# VoxClaw

**下一代 Windows 全局 AI 语音助手与输入法**

*按住说话，松开上屏。极速、全局、懂你的上下文。*

[![Platform](https://img.shields.io/badge/Platform-Windows-blue.svg?style=for-the-badge&logo=windows)](https://github.com/guxiangjun/VoxClaw/releases)
[![Rust](https://img.shields.io/badge/Built_with-Rust-f46623.svg?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Powered_by-Tauri-FFC131.svg?style=for-the-badge&logo=tauri)](https://tauri.app/)
[![License: MIT](https://img.shields.io/badge/License-MIT-success.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)

[**English**](./README.md) • [**简体中文**](./README_zh.md)

</div>

---

## ⚡ 为什么选择 VoxClaw？

传统的语音输入软件往往只是单纯的“把声音转成文字”。**VoxClaw** 则完全不同。它将 **极速在线流式 ASR** 与 **智能大语言模型（LLM）纠错** 完美结合，确保你口述的内容不仅精准识别，更能根据上下文自动转换为书面语言。

无论你是在敲代码、写周报、还是调用系统快捷指令，VoxClaw 都是你桌面端最优雅、如影随形的隐形副驾驶（Co-pilot）。

## ✨ 核心特性

- 🎙️ **在线/离线双引擎识别:** 既支持基于顶级在线 ASR 的实时流式识别（边说边看预览），又全新加入了 **SenseVoice 离线本地模型** 支持，无需联网即可使用，100% 保障您的数据隐私。
- ⌨️ **全局沉浸输入:** 通过底层的剪贴板注入 (`Ctrl+V`) 和低延迟的原生鼠标与键盘事件监听，完美适配微信、浏览器、IDE、甚至是系统的任何输入框。
- 🤖 **自主 AI 智能体:** 强大的 ReAct 架构智能体，能够理解复杂意图，自动刷网页、改代码、运行指令甚至截屏。
- 📅 **会议管理:** 新增会议管理功能与会话处理，助力高效记录与总结。
- 🎯 **三位一体模式:**
  - **听写模式 (Dictation):** 高效记录，支持大模型自动润色。
  - **智能体模式 (Agent):** 开启自主行动能力，处理多步骤复杂任务（如：“整理今天的股市新闻并存到桌面”）。
  - **指令模式 (Skills):** 语音成为系统捷径（例如：说“打开计算器”、“截个图”并瞬间响应）。
- 🎧 **极强音频鲁棒性:** 内置高级音频处理模块，支持设备断开、睡眠唤醒后的自动恢复与热插拔监测。
- 🎨 **极致优雅的 UI:** 为桌面美学打造的悬浮毛玻璃胶囊，极简、克制且充满交互细节。

---

## 🚀 30秒极速上手

### 1. 下载与安装
前往 [Releases 页面](https://github.com/guxiangjun/VoxClaw/releases) 下载最新的 Windows 安装包（`.exe`）。

### 2. 配置 ASR 引擎
首次启动后打开 **Settings (设置)**，填入你的在线 ASR 凭证。
*(你可以在 Settings -> Audio Input 测试麦克风是否正常收音)*

### 3. 按住并开始你的讲述！
我们极其推荐你使用 **鼠标中键** 作为主力触发方式：

| 模式 | 触发键位 | 交互逻辑 |
| --- | --- | --- |
| 📝 **听写功能** | `鼠标中键` (长按) | 长按开始说话，松手即刻上屏到光标处。 |
| 📝 **听写功能** | `右侧 Alt` (单点) | 点击一次开始，再点击一次结束并上屏。 |
| ⚡ **语音指令** | `Ctrl + Win` (长按) | 说出指令（如”截个图”），松开立即执行。 |

---

## 📦 离线模式（SenseVoice）运行时 DLL 安装说明

基于本地 SenseVoice 模型的离线模式需要依赖**微软的 ONNX Runtime** 才能进行推理。出于规避杀软误报以及缩小安装包体积的考虑，VoxClaw **未内置** 该 DLL，首次使用时需要你手动下载并放置 `onnxruntime.dll`。

> ℹ️ 若跳过此步骤，VoxClaw 会回退到机器上**已有的** `onnxruntime.dll`（通常来自 Python 安装环境），而这些版本**几乎都是不兼容的旧版**，从而触发 `ort ... is not compatible with the ONNX Runtime binary` 报错。

### 第一步 —— 下载正确的 DLL

从微软官方仓库下载 **onnxruntime-win-x64-1.23.0**（CPU 标准版）：

```
https://github.com/microsoft/onnxruntime/releases/download/v1.23.0/onnxruntime-win-x64-1.23.0.zip
```

> ⚠️ **版本要求：** 项目内置的 `ort` 库（v2.0.0-rc.11）要求 ONNX Runtime **`>= 1.23.x`**。**不要**使用 1.20.x —— 这是绝大多数用户中招报错的版本。

解压后取 `lib\onnxruntime.dll` 这**一个文件**即可。

### 第二步 —— 放置 DLL（三选一）

VoxClaw 会按以下顺序查找 DLL，任选其一即可：

| 优先级 | 位置 | 说明 |
| --- | --- | --- |
| 1 | `voxclaw.exe` 同级目录 | **推荐。** 右键桌面快捷方式 → “打开文件所在的位置”，把 DLL 丢进去。完全脱离 Python 环境。 |
| 2 | 环境变量 `SONICCLAW_ONNXRUNTIME_DLL` | 值设为 DLL 的完整路径。需要重启应用生效。 |
| 3 | Python site-packages 目录 | **不推荐。** 可能被 `pip` 覆盖，版本会漂移。仅作最后兜底。 |

### 第三步 —— 重启 VoxClaw

ONNX Runtime 在一个进程内**只加载一次**并被冻结至进程结束。你**必须完全退出并重新打开 VoxClaw**，新 DLL 才会生效 —— 仅在运行中的进程里重新点击”切换模型”不会换 DLL。

### 验证是否成功

打开 **设置 → 切换到 SenseVoice → 切换模型**，应当**不再**出现 `is not compatible` 报错。若仍报错，请检查 DLL 是否在上述任一位置、以及应用是否真的重启过。

### 一行 PowerShell 自动修复（默认安装路径）

保存为 `fix-ort.ps1` 以管理员身份运行：

```powershell
$ErrorActionPreference = “Stop”
Invoke-WebRequest -Uri “https://github.com/microsoft/onnxruntime/releases/download/v1.23.0/onnxruntime-win-x64-1.23.0.zip” -OutFile “$env:TEMP\ort.zip”
Expand-Archive -Path “$env:TEMP\ort.zip” -DestinationPath “$env:TEMP\ort” -Force
Copy-Item “$env:TEMP\ort\onnxruntime-win-x64-1.23.0\lib\onnxruntime.dll” -Destination “$env:LOCALAPPDATA\voxclaw\onnxruntime.dll” -Force
Write-Host “完成。请重启 VoxClaw。”
```

> 💡 若 VoxClaw 不在默认路径，请将 `$env:LOCALAPPDATA\voxclaw` 替换为实际安装路径。

> ⚠️ **关于 GPU：** 该 CPU 标准版 DLL **不含** DirectML 执行提供程序。若设置中开启了 `use_gpu`，DirectML 初始化会失败并静默回退到 CPU —— 听写功能照常可用，只是没有 GPU 加速。

---

## 🧠 智能体场景 (LLM Profiles)

接入任何兼容 OpenAI 格式的 API，将听写拉升至“智能体”层级。
你不再受限于一套生硬的提示词，而是能为不同场景自由建立 Profile：
- **会议纪要**：提取核心要点，条理清晰。
- **商务沟通**：转换你的口语为极其专业的邮件措辞。
- **写代码/注释**：说大白话，自动生成高质量 Markdown 注释。

---

## 🛠️ 技术栈与极致性能

我们深知桌面软件“性能即正义”，采用纯血现代技术栈打造：

- **核心层 & 运行时**: `Rust` + `Tauri v2` (极低的后台常驻内存占用)
- **前端动画层**: `React` + `TypeScript` + `TailwindCSS` (丝滑交互与现代美学)
- **音频捕获**: `cpal`
- **全局输入架构**: Native Win32 Raw Input (零 Hook 架构) + 原生 FFI `SendInput`

---

## 🏗️ 开发者指南

请确保你的 Windows 环境下已安装 **Node.js 18+** 与 **Rust 最新稳定版**。

```bash
# 安装依赖
pnpm install

# 启动开发调试模式
pnpm tauri dev

# 打包构建 Windows NSIS 安装包
pnpm tauri build
```
*构建产物将输出至: `src-tauri/target/release/bundle/`。*

---

## 🔒 隐私与数据安全

你的声音，你做主。
- **关于语音数据:** 音频仅会流式发送给**你在配置页主动作出选择的 ASR 平台**。
- **关于模型数据:** 只有当你手动打开 LLM 纠错开关，识别出来的**文本**才会被发给你配置的个人 LLM API。
- **本地存储:** 所有配置文件和历史记录都储存在本地的 `%APPDATA%\com.voxclaw\`。
- **系统安全:** 采用 Windows 原生 Raw Input API 代替全局钩子 (Hooks)，架构更标准、透明，在保证零延迟的同时，极大程度避免了杀毒软件的误报。

---

## 🤝 参与贡献

欢迎提交 Issue 和 Pull Requests！如果您想反馈问题：
1. 请务必在描述中附带：触发方式、所交互的目标软件、复现步骤及可能的截图。
2. 提交 PR 时，请尽量让改动保持内聚，我们极其看重代码的可维护性，避免引入复杂的冗余状态。

## 📄 开源许可证

本项目基于 [MIT License](LICENSE) 许可协议开源。

<div align="center">
  <i>为 Windows 硬核玩家用 ❤️ 打造。</i>
</div>
