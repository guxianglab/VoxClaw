import { useEffect, useState } from "react";
import { Check, Copy, FolderOpen, Loader2, Plus, RotateCcw, Trash2 } from "lucide-react";
import { openPath } from "@tauri-apps/plugin-opener";
import { api, AgentConfig, AppConfig, AudioDevice, LlmConfig, PromptProfile, events } from "../../lib/api";
import {
  ActionButton,
  Advanced,
  Area,
  ChoiceCard,
  Chip,
  Field,
  Key,
  Notice,
  NumberField,
  PrimaryButton,
  ProgressBar,
  Section,
  ToggleRow,
} from "../ui";
import { isAiReady } from "./useConfigDraft";

export interface PaneProps {
  config: AppConfig;
  update: (next: AppConfig) => void;
}

// ---------------------------------------------------------------------------
// General: hotkeys, microphone, network
// ---------------------------------------------------------------------------

export function HotkeySection({ config, update }: PaneProps) {
  return (
    <Section title="快捷键" desc="单击开始/结束听写，双击单次润色，长按唤起 AI 助手">
      <ToggleRow
        title="鼠标中键"
        desc="按一下滚轮"
        active={config.trigger_mouse}
        onToggle={() => update({ ...config, trigger_mouse: !config.trigger_mouse })}
      />
      <ToggleRow
        title="右侧 Alt 键"
        desc="键盘右下角的 Alt"
        active={config.trigger_toggle}
        onToggle={() => update({ ...config, trigger_toggle: !config.trigger_toggle })}
      />
      {!config.trigger_mouse && !config.trigger_toggle && (
        <Notice tone="amber">两个快捷键都关闭了，将无法开始听写。</Notice>
      )}
    </Section>
  );
}

export function MicrophoneSection({ config, update }: PaneProps) {
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [current, setCurrent] = useState("");
  const [level, setLevel] = useState(0);

  useEffect(() => {
    api.getInputDevices().then(setDevices);
    api.getCurrentInputDevice().then(setCurrent);
    const poll = window.setInterval(() => api.getInputDevices().then(setDevices), 3000);

    // Show a live level meter while this section is visible.
    api.startAudioTest().catch(() => {});
    const unsub = events.onAudioLevel((l) => setLevel(Math.min(1, l * 5)));
    return () => {
      window.clearInterval(poll);
      unsub.then((fn) => fn());
      api.stopAudioTest().catch(() => {});
    };
  }, []);

  const select = async (deviceId: string) => {
    await api.switchInputDevice(deviceId);
    setCurrent(deviceId);
    update({ ...config, input_device: deviceId });
  };

  return (
    <Section title="麦克风" desc="对着麦克风说话，下方的音量条应该会跳动">
      <select
        value={current}
        onChange={(event) => select(event.target.value)}
        className="input-underline w-full py-2 text-sm text-neutral-900"
      >
        <option value="">系统默认麦克风</option>
        {devices.map((device) => (
          <option key={device.id} value={device.id}>
            {device.name}
          </option>
        ))}
      </select>
      <ProgressBar value={level} />
    </Section>
  );
}

export function GeneralPane(props: PaneProps) {
  const { config, update } = props;
  return (
    <div className="space-y-8">
      <HotkeySection {...props} />
      <MicrophoneSection {...props} />
      <Section title="网络">
        <ToggleRow
          title="使用代理"
          desc="语音识别和 AI 请求都通过代理发送"
          active={config.proxy.enabled}
          onToggle={() => update({ ...config, proxy: { ...config.proxy, enabled: !config.proxy.enabled } })}
        />
        {config.proxy.enabled && (
          <Field
            label="代理地址"
            hint="仅支持 HTTP 代理"
            placeholder="http://127.0.0.1:7890"
            value={config.proxy.url}
            onChange={(url) => update({ ...config, proxy: { ...config.proxy, url } })}
          />
        )}
      </Section>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Speech recognition
// ---------------------------------------------------------------------------

type DownloadState = { active: boolean; file: string; progress: number; error: string | null };

export function SpeechPane({ config, update }: PaneProps) {
  const asr = config.asr;
  const isLocal = asr.provider === "zipformer_streaming";
  const [download, setDownload] = useState<DownloadState>({ active: false, file: "", progress: 0, error: null });

  useEffect(() => {
    const unsub = events.onAsrModelDownload((evt) => {
      if (evt.phase === "file") {
        const ratio = evt.size && evt.size > 0 ? evt.downloaded / evt.size : 0;
        setDownload({ active: true, file: evt.name, progress: (evt.index - 1 + ratio) / Math.max(1, evt.total), error: null });
      } else if (evt.phase === "failed") {
        setDownload({ active: false, file: "", progress: 0, error: evt.message });
      }
    });
    return () => {
      unsub.then((fn) => fn());
    };
  }, []);

  const startDownload = async () => {
    setDownload({ active: true, file: "", progress: 0, error: null });
    try {
      const dir = await api.downloadZipformerModel();
      setDownload({ active: false, file: "", progress: 1, error: null });
      update({ ...config, asr: { ...asr, zipformer: { model_dir: dir } } });
    } catch (e) {
      setDownload({ active: false, file: "", progress: 0, error: e instanceof Error ? e.message : String(e) });
    }
  };

  const setCred = (key: "app_key" | "access_key" | "resource_id", value: string) =>
    update({ ...config, asr: { ...asr, volcengine: { ...asr.volcengine, [key]: value } } });

  return (
    <div className="space-y-8">
      <Section title="识别方式">
        <div className="flex flex-col gap-3 md:flex-row">
          <ChoiceCard
            title="云端识别"
            desc="豆包语音，识别更准、支持更多口音。需要联网和火山引擎账号。"
            active={!isLocal}
            onClick={() => update({ ...config, asr: { ...asr, provider: "volcengine" } })}
          />
          <ChoiceCard
            title="本地识别"
            desc="在电脑上离线运行，声音不出本机。支持中英文，首次需下载约 300 MB。"
            active={isLocal}
            onClick={() => update({ ...config, asr: { ...asr, provider: "zipformer_streaming" } })}
          />
        </div>
      </Section>

      {!isLocal ? (
        <Section title="豆包语音账号" desc="在火山引擎控制台开通「豆包语音 · 流式语音识别」后，于应用详情页获取">
          <div className="grid gap-4 md:grid-cols-2">
            <Field label="APP ID" value={asr.volcengine.app_key} onChange={(v) => setCred("app_key", v)} />
            <Field
              label="Access Token"
              type="password"
              value={asr.volcengine.access_key}
              onChange={(v) => setCred("access_key", v)}
            />
          </div>
          <Advanced>
            <Field
              label="资源 ID"
              hint="默认 volc.seedasr.sauc.duration（流式识别 2.0），一般无需修改"
              value={asr.volcengine.resource_id}
              onChange={(v) => setCred("resource_id", v)}
            />
          </Advanced>
        </Section>
      ) : (
        <Section title="本地模型">
          <div className="flex flex-wrap items-center gap-3">
            {asr.zipformer.model_dir && !download.active ? (
              <span className="inline-flex items-center gap-1.5 text-sm text-emerald-600">
                <Check className="h-4 w-4" /> 模型已就绪
              </span>
            ) : (
              <PrimaryButton onClick={startDownload} disabled={download.active}>
                {download.active && <Loader2 className="h-4 w-4 animate-spin" />}
                {download.active ? "下载中…" : "下载模型"}
              </PrimaryButton>
            )}
            {download.active && <span className="truncate text-xs text-neutral-400">{download.file}</span>}
          </div>
          {download.active && <ProgressBar value={download.progress} />}
          {download.error && <Notice tone="red">下载失败：{download.error}</Notice>}
          {asr.zipformer.model_dir && (
            <Advanced>
              <div className="flex gap-2">
                <ActionButton onClick={startDownload} disabled={download.active}>
                  <RotateCcw className="h-3.5 w-3.5" /> 重新下载
                </ActionButton>
                <ActionButton onClick={() => openPath(asr.zipformer.model_dir)}>
                  <FolderOpen className="h-3.5 w-3.5" /> 打开模型文件夹
                </ActionButton>
              </div>
            </Advanced>
          )}
        </Section>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// AI model
// ---------------------------------------------------------------------------

interface LlmPreset {
  id: string;
  label: string;
  provider_type: LlmConfig["provider_type"];
  base_url: string;
  model: string;
}

const LLM_PRESETS: LlmPreset[] = [
  { id: "deepseek", label: "DeepSeek", provider_type: "openai_compatible", base_url: "https://api.deepseek.com/v1", model: "deepseek-chat" },
  { id: "qwen", label: "通义千问", provider_type: "openai_compatible", base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus" },
  { id: "openai", label: "OpenAI", provider_type: "openai_compatible", base_url: "https://api.openai.com/v1", model: "gpt-4o-mini" },
  { id: "claude", label: "Claude", provider_type: "anthropic", base_url: "https://api.anthropic.com", model: "claude-sonnet-5-5" },
];

const CUSTOM_PRESET = "custom";

function presetOf(llm: LlmConfig): string {
  const found = LLM_PRESETS.find(
    (p) => p.provider_type === llm.provider_type && p.base_url === llm.base_url.trim().replace(/\/$/, ""),
  );
  return found?.id ?? CUSTOM_PRESET;
}

export function AiPane({ config, update }: PaneProps) {
  const llm = config.llm_config;
  const [preset, setPreset] = useState(() => presetOf(llm));
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string } | null>(null);

  const setLlm = (patch: Partial<LlmConfig>) => {
    update({ ...config, llm_config: { ...llm, ...patch } });
    setResult(null);
  };

  const choosePreset = (id: string) => {
    setPreset(id);
    const p = LLM_PRESETS.find((item) => item.id === id);
    if (p) setLlm({ provider_type: p.provider_type, base_url: p.base_url, model: p.model });
    else setLlm({ provider_type: "openai_compatible" });
  };

  const test = async () => {
    setTesting(true);
    setResult(null);
    try {
      setResult({ ok: true, message: await api.testLlmConnection(llm, config.proxy) });
    } catch (e) {
      setResult({ ok: false, message: String(e) });
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className="space-y-8">
      <Section title="服务商" desc="AI 用于润色文字、执行语音指令和生成会议总结，不配置也能正常听写">
        <div className="flex flex-wrap gap-2">
          {LLM_PRESETS.map((p) => (
            <Chip key={p.id} label={p.label} active={preset === p.id} onClick={() => choosePreset(p.id)} />
          ))}
          <Chip label="其他" active={preset === CUSTOM_PRESET} onClick={() => choosePreset(CUSTOM_PRESET)} />
        </div>
      </Section>

      <Section title="连接信息">
        {preset === CUSTOM_PRESET && (
          <Field
            label="服务地址"
            hint="兼容 OpenAI 接口的地址，通常以 /v1 结尾"
            placeholder="https://example.com/v1"
            value={llm.base_url}
            onChange={(base_url) => setLlm({ base_url })}
          />
        )}
        <Field label="API Key" type="password" value={llm.api_key} onChange={(api_key) => setLlm({ api_key })} />
        <Field
          label="模型"
          hint="可在服务商文档中查到可用的模型名称"
          value={llm.model}
          onChange={(model) => setLlm({ model })}
        />
        <div className="flex flex-wrap items-center gap-3">
          <PrimaryButton onClick={test} disabled={testing || !isAiReady(config)}>
            {testing && <Loader2 className="h-4 w-4 animate-spin" />}
            {testing ? "正在连接…" : "测试连接"}
          </PrimaryButton>
          {result && (
            <span className={`text-sm ${result.ok ? "text-emerald-600" : "text-red-600"}`}>{result.message}</span>
          )}
        </div>
        {preset === CUSTOM_PRESET && (
          <Advanced>
            <ToggleRow
              title="使用 Anthropic 接口格式"
              desc="服务地址兼容 Anthropic Messages API 时开启"
              active={llm.provider_type === "anthropic"}
              onToggle={() =>
                setLlm({ provider_type: llm.provider_type === "anthropic" ? "openai_compatible" : "anthropic" })
              }
            />
          </Advanced>
        )}
      </Section>
    </div>
  );
}

function NeedsAi({ config, onSetupAi }: { config: AppConfig; onSetupAi: () => void }) {
  if (isAiReady(config)) return null;
  return (
    <Notice tone="amber">
      这项功能需要 AI 模型。
      <button onClick={onSetupAi} className="ml-2 text-neutral-900 underline underline-offset-2 transition-colors hover:text-chinese-indigo">
        去配置
      </button>
    </Notice>
  );
}

// ---------------------------------------------------------------------------
// Polish
// ---------------------------------------------------------------------------

const toLines = (items: string[]) => items.join("\n");
const fromLines = (value: string) =>
  value
    .split("\n")
    .map((item) => item.trim())
    .filter(Boolean);

export function PolishPane({ config, update, onSetupAi }: PaneProps & { onSetupAi: () => void }) {
  const llm = config.llm_config;
  const profiles = llm.profiles;
  const active = profiles.find((p) => p.id === llm.active_profile_id) ?? profiles[0] ?? null;
  const [defaults, setDefaults] = useState<PromptProfile[]>([]);

  useEffect(() => {
    api.getDefaultSceneProfiles().then(setDefaults);
  }, []);

  const setLlm = (patch: Partial<LlmConfig>) => update({ ...config, llm_config: { ...llm, ...patch } });
  const edit = (patch: Partial<PromptProfile>) =>
    active && setLlm({ profiles: profiles.map((p) => (p.id === active.id ? { ...p, ...patch } : p)) });

  const add = (base: PromptProfile | null) => {
    const id = `profile_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
    const profile: PromptProfile = base
      ? { ...base, id, name: `${base.name} 副本`, preserve_rules: [...base.preserve_rules], glossary: [...base.glossary] }
      : { id, name: "新风格", goal: "", tone: "", format_style: "", preserve_rules: [], glossary: [], advanced_instruction: "" };
    setLlm({ profiles: [...profiles, profile], active_profile_id: id });
  };

  const remove = () => {
    if (!active || profiles.length <= 1) return;
    const rest = profiles.filter((p) => p.id !== active.id);
    setLlm({ profiles: rest, active_profile_id: rest[0].id });
  };

  const missingDefaults = defaults.filter((d) => !profiles.some((p) => p.id === d.id));

  return (
    <div className="space-y-8">
      <NeedsAi config={config} onSetupAi={onSetupAi} />

      <Section title="何时润色">
        <ToggleRow
          title="每次听写都润色"
          desc="关闭时只在需要时润色：双击快捷键开始或结束听写即可"
          active={llm.enabled}
          onToggle={() => setLlm({ enabled: !llm.enabled })}
        />
      </Section>

      <Section title="润色风格" desc="选中的风格决定 AI 如何整理你说的话">
        <div className="flex flex-wrap items-center gap-2">
          {profiles.map((p) => (
            <Chip key={p.id} label={p.name || "未命名"} active={active?.id === p.id} onClick={() => setLlm({ active_profile_id: p.id })} />
          ))}
          <ActionButton onClick={() => add(null)} title="新建风格">
            <Plus className="h-4 w-4" />
          </ActionButton>
        </div>

        {active && (
          <div className="space-y-4">
            <Field label="名称" value={active.name} onChange={(name) => edit({ name })} />
            <Area
              label="希望 AI 做什么"
              value={active.goal}
              onChange={(goal) => edit({ goal })}
              placeholder="例如：修正错别字，把口语整理成通顺的书面语"
            />
            <div className="grid gap-4 md:grid-cols-2">
              <Field label="语气" placeholder="例如：专业、礼貌" value={active.tone} onChange={(tone) => edit({ tone })} />
              <Field
                label="输出格式"
                placeholder="例如：一段话；或分点列出"
                value={active.format_style}
                onChange={(format_style) => edit({ format_style })}
              />
            </div>
            <Advanced>
              <Area
                label="专有名词"
                hint="每行一个，AI 会按这里的写法输出，如人名、产品名"
                value={toLines(active.glossary)}
                onChange={(v) => edit({ glossary: fromLines(v) })}
              />
              <Area
                label="必须遵守的规则"
                hint="每行一条"
                value={toLines(active.preserve_rules)}
                onChange={(v) => edit({ preserve_rules: fromLines(v) })}
              />
              <Area
                label="补充说明"
                value={active.advanced_instruction}
                onChange={(advanced_instruction) => edit({ advanced_instruction })}
              />
            </Advanced>
            <div className="flex flex-wrap gap-1 border-t border-neutral-200 pt-3">
              <ActionButton onClick={() => add(active)}>
                <Copy className="h-3.5 w-3.5" /> 复制此风格
              </ActionButton>
              <ActionButton onClick={remove} disabled={profiles.length <= 1}>
                <Trash2 className="h-3.5 w-3.5" /> 删除
              </ActionButton>
              {missingDefaults.length > 0 && (
                <ActionButton onClick={() => setLlm({ profiles: [...profiles, ...missingDefaults] })}>
                  <RotateCcw className="h-3.5 w-3.5" /> 找回内置风格
                </ActionButton>
              )}
            </div>
          </div>
        )}
      </Section>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Assistant
// ---------------------------------------------------------------------------

const THINKING_LEVELS: Array<{ value: string; label: string }> = [
  { value: "", label: "关闭" },
  { value: "low", label: "低" },
  { value: "medium", label: "中" },
  { value: "high", label: "高" },
];

export function AssistantPane({ config, update, onSetupAi }: PaneProps & { onSetupAi: () => void }) {
  const agent = config.agent_config;
  const set = <K extends keyof AgentConfig>(key: K, value: AgentConfig[K]) =>
    update({ ...config, agent_config: { ...agent, [key]: value } });

  return (
    <div className="space-y-8">
      <NeedsAi config={config} onSetupAi={onSetupAi} />

      <Section title="怎么用">
        <div className="space-y-2 text-sm leading-6 text-neutral-600">
          <p>
            按住 <Key>{config.trigger_mouse ? "鼠标中键" : "右 Alt"}</Key> 说出指令，松开后执行。例如：
          </p>
          <ul className="space-y-1 pl-4 text-neutral-500">
            <li>“打开 B 站”</li>
            <li>“截个图”</li>
            <li>“帮我写封邮件给张三，说明天的会改到下午三点”</li>
          </ul>
          <p className="text-xs text-neutral-400">如果先选中了一段文字，助手会把它当作参考。</p>
        </div>
      </Section>

      <Section title="偏好">
        <ToggleRow
          title="连续对话"
          desc="记住上一条指令，可以接着追问或纠正"
          active={agent.continuous_mode}
          onToggle={() => set("continuous_mode", !agent.continuous_mode)}
        />
        <Area
          label="关于你"
          hint="助手每次都会参考这些信息"
          placeholder="例如：我叫小王，常用浏览器是 Edge，邮件落款用“王明”"
          value={agent.persistent_context}
          onChange={(v) => set("persistent_context", v)}
        />
      </Section>

      <Advanced>
        <div>
          <label className="mb-2 block text-xs text-neutral-400">深度思考</label>
          <div className="flex gap-2">
            {THINKING_LEVELS.map((level) => (
              <Chip
                key={level.value}
                label={level.label}
                active={(agent.thinking_level || "") === level.value || (level.value === "" && agent.thinking_level === "none")}
                onClick={() => set("thinking_level", level.value)}
              />
            ))}
          </div>
          <div className="mt-1 text-xs text-neutral-400">越高越聪明但越慢，仅部分模型支持</div>
        </div>
        <NumberField
          label="最多执行步数"
          hint="一条指令最多调用几次工具"
          value={agent.max_iterations}
          min={1}
          max={50}
          onChange={(v) => set("max_iterations", v)}
        />
        <NumberField
          label="参考最近几次指令"
          hint="0 表示不参考"
          value={agent.context_history_count}
          min={0}
          max={20}
          onChange={(v) => set("context_history_count", v)}
        />
        <Area
          label="自定义系统提示词"
          hint="留空使用内置提示词"
          rows={4}
          value={agent.system_prompt}
          onChange={(v) => set("system_prompt", v)}
        />
      </Advanced>
    </div>
  );
}
