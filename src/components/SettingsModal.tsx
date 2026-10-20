import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { AlertCircle, Copy, FolderOpen, Loader2, Plus, Trash2, X } from "lucide-react";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  api,
  AgentConfig,
  AppConfig,
  AsrConfig,
  AudioDevice,
  LlmConfig,
  PromptProfile,
  events,
} from "../lib/api";

interface SettingsModalProps {
  isOpen: boolean;
  onClose: (skip?: boolean) => void;
  isFirstSetup?: boolean;
}

type SettingsTab = "audio" | "models" | "polish" | "agent";

const TABS: Array<{ key: SettingsTab; label: string }> = [
  { key: "audio", label: "录音" },
  { key: "models", label: "模型" },
  { key: "polish", label: "润色" },
  { key: "agent", label: "助手" },
];

const toLines = (items: string[]) => items.join("\n");
const fromLines = (value: string) =>
  value
    .split("\n")
    .map((item) => item.trim())
    .filter(Boolean);

const makeProfileId = () => `profile_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;

export function SettingsModal({ isOpen, onClose, isFirstSetup = false }: SettingsModalProps) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [currentDevice, setCurrentDevice] = useState("");
  const [defaultProfiles, setDefaultProfiles] = useState<PromptProfile[]>([]);
  const [audioLevel, setAudioLevel] = useState(0);
  const [testingAudio, setTestingAudio] = useState(false);
  const [testingLlm, setTestingLlm] = useState(false);
  const [llmResult, setLlmResult] = useState<{ success: boolean; message: string } | null>(null);
  const [showWarning, setShowWarning] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [saveOk, setSaveOk] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  // Non-fatal notice returned by save (e.g. "engine selected but model not
  // downloaded yet" — config was still saved).
  const [saveNotice, setSaveNotice] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<SettingsTab>("audio");
  const [modelDownload, setModelDownload] = useState<{
    active: boolean;
    file: string;
    progress: number;
    error: string | null;
  }>({ active: false, file: "", progress: 0, error: null });
  const [zfPresent, setZfPresent] = useState<boolean | null>(null);
  const timerRef = useRef<number | null>(null);
  const pendingRef = useRef<AppConfig | null>(null);

  const saveLater = useCallback((next: AppConfig) => {
    pendingRef.current = next;
    if (timerRef.current) window.clearTimeout(timerRef.current);
    setIsSaving(true);
    setSaveNotice(null);
    timerRef.current = window.setTimeout(async () => {
      const finalConfig = pendingRef.current;
      if (finalConfig) {
        try {
          const notice = await api.saveConfig(finalConfig);
          setSaveOk(true);
          setSaveError(null);
          if (notice) setSaveNotice(notice);
          window.setTimeout(() => setSaveOk(false), 1500);
        } catch (error) {
          // Surface the backend error (e.g. engine switch blocked by an active
          // session) instead of silently keeping the old provider. The config
          // was NOT persisted in that case.
          setSaveError(error instanceof Error ? error.message : String(error));
          console.error("save config failed", error);
        }
      }
      pendingRef.current = null;
      setIsSaving(false);
    }, 300);
  }, []);

  useEffect(() => {
    return () => {
      if (timerRef.current) window.clearTimeout(timerRef.current);
    };
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    api.getConfig().then(setConfig);
    api.getInputDevices().then(setDevices);
    api.getCurrentInputDevice().then(setCurrentDevice);
    api.getDefaultSceneProfiles().then(setDefaultProfiles);
    setActiveTab("audio");
    setShowWarning(false);
    setLlmResult(null);
    setSaveNotice(null);
    const interval = window.setInterval(() => api.getInputDevices().then(setDevices), 3000);
    return () => window.clearInterval(interval);
  }, [isOpen]);

  useEffect(() => {
    if (!testingAudio) return;
    const unsub = events.onAudioLevel((level) => setAudioLevel(Math.min(1, level * 5)));
    return () => {
      unsub.then((fn) => fn());
    };
  }, [testingAudio]);

  useEffect(() => {
    if (!isOpen && testingAudio) {
      api.stopAudioTest();
      setTestingAudio(false);
      setAudioLevel(0);
    }
  }, [isOpen, testingAudio]);

  useEffect(() => {
    if (!isOpen) return;
    const unsub = events.onAsrModelDownload((evt) => {
      if (evt.phase === "started") {
        setModelDownload({ active: true, file: "", progress: 0, error: null });
      } else if (evt.phase === "file") {
        const ratio = evt.size && evt.size > 0 ? evt.downloaded / evt.size : 0;
        const overall = (evt.index - 1 + ratio) / Math.max(1, evt.total);
        setModelDownload({ active: true, file: evt.name, progress: overall, error: null });
      } else if (evt.phase === "finished") {
        setModelDownload({ active: false, file: "", progress: 1, error: null });
      } else if (evt.phase === "failed") {
        setModelDownload({ active: false, file: "", progress: 0, error: evt.message });
      }
    });
    return () => {
      unsub.then((fn) => fn());
    };
  }, [isOpen]);

  // Re-check after a download finishes so the badge updates without reopening.
  const zipformerDir = config?.asr.zipformer.model_dir;
  useEffect(() => {
    if (!isOpen) return;
    (async () => {
      const dir = zipformerDir || (await api.getZipformerDefaultDir());
      setZfPresent(await api.checkZipformerModelPresent(dir));
    })().catch(() => {});
  }, [isOpen, zipformerDir, modelDownload.active]);

  if (!isOpen || !config) return null;

  const update = (next: AppConfig) => {
    setConfig(next);
    saveLater(next);
  };

  const updateConfig = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) =>
    update({ ...config, [key]: value });

  const updateAsr = (patch: Partial<AsrConfig>) => updateConfig("asr", { ...config.asr, ...patch });

  const updateLlm = (patch: Partial<LlmConfig>) => {
    updateConfig("llm_config", { ...config.llm_config, ...patch });
    setLlmResult(null);
  };

  const updateAgent = <K extends keyof AgentConfig>(key: K, value: AgentConfig[K]) =>
    updateConfig("agent_config", { ...config.agent_config, [key]: value });

  const downloadZipformer = async () => {
    setModelDownload({ active: true, file: "", progress: 0, error: null });
    try {
      const dir = await api.downloadZipformerModel();
      const next = { ...config, asr: { ...config.asr, zipformer: { model_dir: dir } } };
      setConfig(next);
    } catch (e) {
      setModelDownload({ active: false, file: "", progress: 0, error: e instanceof Error ? e.message : String(e) });
    }
  };

  const openZipformerDir = async () => {
    openPath(config.asr.zipformer.model_dir || (await api.getZipformerDefaultDir()));
  };

  const profiles = config.llm_config.profiles;
  const active = profiles.find((p) => p.id === config.llm_config.active_profile_id) ?? profiles[0] ?? null;

  const updateActive = (patch: Partial<PromptProfile>) => {
    if (!active) return;
    updateLlm({ profiles: profiles.map((p) => (p.id === active.id ? { ...p, ...patch } : p)) });
  };

  const addProfile = (base: PromptProfile | null, name: string) => {
    const id = makeProfileId();
    const profile: PromptProfile = base
      ? { ...base, id, name, preserve_rules: [...base.preserve_rules], glossary: [...base.glossary] }
      : { id, name, goal: "", tone: "", format_style: "", preserve_rules: [], glossary: [], advanced_instruction: "" };
    updateLlm({ profiles: [...profiles, profile], active_profile_id: id });
  };

  const deleteProfile = () => {
    if (!active || profiles.length <= 1) return;
    const rest = profiles.filter((p) => p.id !== active.id);
    updateLlm({ profiles: rest, active_profile_id: rest[0].id });
  };

  const restoreDefaults = () => {
    const ids = new Set(profiles.map((p) => p.id));
    updateLlm({ profiles: [...profiles, ...defaultProfiles.filter((p) => !ids.has(p.id))] });
  };

  const testLlm = async () => {
    setTestingLlm(true);
    setLlmResult(null);
    try {
      const message = await api.testLlmConnection(config.llm_config, config.proxy);
      setLlmResult({ success: true, message });
    } catch (error) {
      setLlmResult({ success: false, message: String(error) });
    } finally {
      setTestingLlm(false);
    }
  };

  const switchDevice = async (deviceId: string) => {
    await api.switchInputDevice(deviceId);
    setCurrentDevice(deviceId);
    updateConfig("input_device", deviceId);
  };

  const toggleAudio = async () => {
    if (testingAudio) {
      await api.stopAudioTest();
      setTestingAudio(false);
      setAudioLevel(0);
    } else {
      await api.startAudioTest();
      setTestingAudio(true);
    }
  };

  const flush = async () => {
    if (!pendingRef.current) return;
    if (timerRef.current) window.clearTimeout(timerRef.current);
    await api.saveConfig(pendingRef.current);
    pendingRef.current = null;
    setIsSaving(false);
  };

  const close = async () => {
    const asrReady =
      config.asr.provider === "zipformer_streaming"
        ? !!zfPresent
        : !!config.asr.volcengine.app_key && !!config.asr.volcengine.access_key;
    if (isFirstSetup && !(config.input_device && asrReady)) {
      setShowWarning(true);
      return;
    }
    await flush();
    onClose();
  };

  const saveText = isSaving ? "正在保存..." : saveError ? "保存失败，请查看提示" : saveOk ? "已保存" : "自动保存";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-neutral-950/20 p-4" onClick={close}>
      <div
        className="flex max-h-[88vh] w-full max-w-5xl flex-col overflow-hidden bg-neutral-50"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-neutral-200 px-6 py-5">
          <div className="min-w-0">
            <h2 className="text-xl font-semibold text-neutral-900">{isFirstSetup ? "完成设置" : "设置"}</h2>
            <div className={`mt-1 text-sm ${saveError ? "text-red-600" : "text-neutral-500"}`}>
              {isFirstSetup ? "先配置麦克风和语音识别" : saveText}
            </div>
            {saveError && !isFirstSetup && (
              <div className="mt-2 max-w-xl text-xs leading-relaxed text-red-600">{saveError}</div>
            )}
            {saveNotice && !saveError && !isFirstSetup && (
              <div className="mt-2 max-w-xl text-xs leading-relaxed text-amber-600">{saveNotice}</div>
            )}
          </div>
          <button
            onClick={close}
            className="inline-flex h-10 w-10 shrink-0 items-center justify-center text-neutral-400 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="flex min-h-0 flex-1 flex-col md:flex-row">
          <aside className="border-b border-neutral-200 px-3 py-4 md:w-[100px] md:border-b-0 md:border-r">
            <nav className="flex gap-2 md:flex-col">
              {TABS.map((tab) => (
                <button
                  key={tab.key}
                  onClick={() => setActiveTab(tab.key)}
                  className={`flex items-center gap-2 px-2 py-2.5 text-left text-sm transition-colors ${
                    activeTab === tab.key ? "text-neutral-900" : "text-neutral-500 hover:text-neutral-700"
                  }`}
                >
                  <span
                    className={`h-1.5 w-1.5 rounded-full ${
                      activeTab === tab.key ? "bg-neutral-900" : "border border-neutral-300"
                    }`}
                  />
                  <span className={activeTab === tab.key ? "font-medium" : ""}>{tab.label}</span>
                </button>
              ))}
            </nav>
          </aside>

          <div className="custom-scrollbar min-h-0 flex-1 overflow-y-auto px-6 py-6">
            {activeTab === "audio" && (
              <div className="space-y-5">
                <Section title="触发方式">
                  <ToggleRow
                    title="鼠标中键"
                    desc="单击切换听写，长按唤起 AI 助手"
                    active={config.trigger_mouse}
                    onToggle={() => updateConfig("trigger_mouse", !config.trigger_mouse)}
                  />
                  <ToggleRow
                    title="右 Alt"
                    desc="单击切换听写，长按唤起 AI 助手"
                    active={config.trigger_toggle}
                    onToggle={() => updateConfig("trigger_toggle", !config.trigger_toggle)}
                  />
                </Section>

                <Section title="输入设备">
                  <label className="mb-2 block text-xs text-neutral-400">设备</label>
                  <div className="flex flex-col gap-3 lg:flex-row">
                    <select
                      value={currentDevice}
                      onChange={(event) => switchDevice(event.target.value)}
                      className="input-underline w-full py-2 text-neutral-900"
                    >
                      <option value="">默认设备</option>
                      {devices.map((device) => (
                        <option key={device.id} value={device.id}>
                          {device.name}
                          {device.is_default ? "（默认）" : ""}
                        </option>
                      ))}
                    </select>
                    <div className="flex gap-3">
                      <ActionButton onClick={() => api.getInputDevices().then(setDevices)}>刷新设备</ActionButton>
                      <PrimaryButton onClick={toggleAudio}>{testingAudio ? "停止测试" : "测试麦克风"}</PrimaryButton>
                    </div>
                  </div>
                  <ProgressBar value={audioLevel} />
                </Section>
              </div>
            )}

            {activeTab === "models" && (
              <div className="space-y-5">
                <Section title="语音识别引擎">
                  <div className="flex flex-wrap gap-2">
                    <ChipButton
                      label="豆包（云端流式）"
                      active={config.asr.provider === "volcengine"}
                      onClick={() => updateAsr({ provider: "volcengine" })}
                    />
                    <ChipButton
                      label="Zipformer（本地流式）"
                      active={config.asr.provider === "zipformer_streaming"}
                      onClick={() => updateAsr({ provider: "zipformer_streaming" })}
                    />
                  </div>

                  {config.asr.provider === "volcengine" ? (
                    <div className="grid gap-3 md:grid-cols-2">
                      {(["app_key", "access_key", "resource_id"] as const).map((key) => (
                        <Field
                          key={key}
                          label={key}
                          value={config.asr.volcengine[key]}
                          onChange={(value) => updateAsr({ volcengine: { ...config.asr.volcengine, [key]: value } })}
                        />
                      ))}
                    </div>
                  ) : (
                    <div className="space-y-3">
                      <p className="text-xs text-neutral-400">中英双语本地流式模型，无需联网。首次使用需下载约 300 MB。</p>
                      <div className="flex flex-wrap items-center gap-3">
                        <PrimaryButton onClick={downloadZipformer} disabled={modelDownload.active}>
                          {modelDownload.active ? "下载中…" : zfPresent ? "重新下载" : "下载模型"}
                        </PrimaryButton>
                        <ActionButton onClick={openZipformerDir}>
                          <FolderOpen className="mr-1 inline h-3.5 w-3.5" />
                          打开模型目录
                        </ActionButton>
                        {modelDownload.active && (
                          <span className="truncate text-xs text-neutral-500">{modelDownload.file || "正在准备…"}</span>
                        )}
                        {zfPresent && !modelDownload.active && <span className="text-xs text-emerald-600">已下载</span>}
                        {!modelDownload.active && modelDownload.error && (
                          <span className="truncate text-xs text-red-600">{modelDownload.error}</span>
                        )}
                      </div>
                      {modelDownload.active && <ProgressBar value={modelDownload.progress} />}
                    </div>
                  )}
                </Section>

                <Section title="语言模型">
                  <p className="text-xs text-neutral-400">润色、AI 助手和会议总结共用此配置。</p>
                  <div className="flex flex-wrap gap-2">
                    <ChipButton
                      label="OpenAI 兼容"
                      active={config.llm_config.provider_type !== "anthropic"}
                      onClick={() => updateLlm({ provider_type: "openai_compatible" })}
                    />
                    <ChipButton
                      label="Anthropic"
                      active={config.llm_config.provider_type === "anthropic"}
                      onClick={() => updateLlm({ provider_type: "anthropic" })}
                    />
                  </div>
                  <div className="grid gap-3 md:grid-cols-2">
                    <Field label="base_url" value={config.llm_config.base_url} onChange={(value) => updateLlm({ base_url: value })} />
                    <Field label="model" value={config.llm_config.model} onChange={(value) => updateLlm({ model: value })} />
                  </div>
                  <Field
                    label="api_key"
                    type="password"
                    value={config.llm_config.api_key}
                    onChange={(value) => updateLlm({ api_key: value })}
                  />
                  <div className="flex flex-wrap items-center gap-3">
                    <PrimaryButton onClick={testLlm} disabled={testingLlm}>
                      {testingLlm && <Loader2 className="h-4 w-4 animate-spin" />}
                      {testingLlm ? "测试中..." : "测试连接"}
                    </PrimaryButton>
                    {llmResult && (
                      <span className={`text-sm ${llmResult.success ? "text-emerald-600" : "text-red-600"}`}>
                        {llmResult.message}
                      </span>
                    )}
                  </div>
                </Section>

                <Section title="网络代理">
                  <ToggleRow
                    title="启用代理"
                    desc="语音识别和语言模型共用，仅支持 http:// 代理"
                    active={config.proxy.enabled}
                    onToggle={() => updateConfig("proxy", { ...config.proxy, enabled: !config.proxy.enabled })}
                  />
                  {config.proxy.enabled && (
                    <Field
                      label="代理地址"
                      value={config.proxy.url}
                      placeholder="http://127.0.0.1:7890"
                      onChange={(value) => updateConfig("proxy", { ...config.proxy, url: value })}
                    />
                  )}
                </Section>
              </div>
            )}

            {activeTab === "polish" && (
              <div className="space-y-5">
                <Section title="启用">
                  <ToggleRow
                    title="始终润色"
                    desc="开启后所有听写都经过润色；关闭时双击触发键可单次润色"
                    active={config.llm_config.enabled}
                    onToggle={() => updateLlm({ enabled: !config.llm_config.enabled })}
                  />
                </Section>

                <Section title="场景">
                  <div className="flex flex-wrap gap-2">
                    {profiles.map((profile) => (
                      <ChipButton
                        key={profile.id}
                        label={profile.name}
                        active={active?.id === profile.id}
                        onClick={() => updateLlm({ active_profile_id: profile.id })}
                      />
                    ))}
                  </div>
                  <div className="flex flex-wrap items-center gap-1">
                    <MiniButton onClick={() => addProfile(null, `场景 ${profiles.length + 1}`)} icon={<Plus className="h-4 w-4" />} title="新增场景" />
                    <MiniButton onClick={() => active && addProfile(active, `${active.name} 副本`)} icon={<Copy className="h-4 w-4" />} title="复制当前场景" />
                    <MiniButton onClick={deleteProfile} icon={<Trash2 className="h-4 w-4" />} disabled={profiles.length <= 1} title="删除当前场景" />
                    <ActionButton onClick={restoreDefaults}>恢复默认场景</ActionButton>
                  </div>

                  {active && (
                    <div className="space-y-4">
                      <div className="grid gap-3 md:grid-cols-2">
                        <Field label="场景名称" value={active.name} onChange={(value) => updateActive({ name: value })} />
                        <Field label="语气" value={active.tone} onChange={(value) => updateActive({ tone: value })} />
                      </div>
                      <Area label="目标" value={active.goal} onChange={(value) => updateActive({ goal: value })} />
                      <Area label="输出格式" value={active.format_style} onChange={(value) => updateActive({ format_style: value })} />
                      <div className="grid gap-3 lg:grid-cols-2">
                        <Area
                          label="保留规则（每行一条）"
                          value={toLines(active.preserve_rules)}
                          onChange={(value) => updateActive({ preserve_rules: fromLines(value) })}
                        />
                        <Area
                          label="术语表（每行一条）"
                          value={toLines(active.glossary)}
                          onChange={(value) => updateActive({ glossary: fromLines(value) })}
                        />
                      </div>
                      <Area
                        label="补充指令"
                        value={active.advanced_instruction}
                        onChange={(value) => updateActive({ advanced_instruction: value })}
                        placeholder="额外规则、结构偏好或禁止改写项（可选）"
                      />
                    </div>
                  )}
                </Section>
              </div>
            )}

            {activeTab === "agent" && (
              <div className="space-y-5">
                <Section title="AI 助手">
                  <p className="text-xs text-neutral-400">
                    长按触发键说出指令，助手可以打开网页和应用、截图、读写剪贴板、写邮件。使用“模型”页中的语言模型。
                  </p>
                  <div className="grid gap-3 md:grid-cols-2">
                    <Field
                      label="思考强度"
                      value={config.agent_config.thinking_level}
                      placeholder="none / low / medium / high"
                      onChange={(value) => updateAgent("thinking_level", value)}
                    />
                    <NumberField
                      label="最大迭代次数"
                      value={config.agent_config.max_iterations}
                      min={1}
                      max={50}
                      onChange={(value) => updateAgent("max_iterations", value || 10)}
                    />
                  </div>
                  <Area
                    label="系统提示词"
                    value={config.agent_config.system_prompt}
                    onChange={(value) => updateAgent("system_prompt", value)}
                    rows={4}
                    placeholder="留空使用内置默认提示词"
                  />
                  <Area
                    label="常驻上下文"
                    value={config.agent_config.persistent_context}
                    onChange={(value) => updateAgent("persistent_context", value)}
                    placeholder="每次执行都会注入的信息，如常用缩写、个人偏好"
                  />
                  <NumberField
                    label="历史上下文条数（0 为关闭）"
                    value={config.agent_config.context_history_count}
                    min={0}
                    max={20}
                    onChange={(value) => updateAgent("context_history_count", value)}
                  />
                  <ToggleRow
                    title="连续会话"
                    desc="多次语音指令共享上下文，可追问或纠正"
                    active={config.agent_config.continuous_mode}
                    onToggle={() => updateAgent("continuous_mode", !config.agent_config.continuous_mode)}
                  />
                </Section>
              </div>
            )}
          </div>
        </div>
      </div>

      {showWarning && (
        <div className="fixed inset-0 z-[60] flex items-center justify-center bg-neutral-950/25 p-4">
          <div className="w-full max-w-sm bg-neutral-50 p-6" onClick={(event) => event.stopPropagation()}>
            <div className="flex items-center gap-3">
              <AlertCircle className="h-5 w-5 flex-shrink-0 text-amber-600" />
              <div>
                <h3 className="text-base font-semibold text-neutral-900">还没有完成设置</h3>
                <p className="mt-1 text-sm text-neutral-500">请先选择输入设备，并配置语音识别引擎。</p>
              </div>
            </div>
            <div className="mt-6 flex justify-end gap-2">
              <ActionButton
                onClick={async () => {
                  await flush();
                  onClose(true);
                }}
              >
                暂不设置
              </ActionButton>
              <PrimaryButton onClick={() => setShowWarning(false)}>继续设置</PrimaryButton>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="space-y-4">
      <div className="border-b border-neutral-200 pb-2 text-xs font-medium uppercase tracking-wider text-neutral-400">{title}</div>
      <div className="space-y-4">{children}</div>
    </section>
  );
}

function ProgressBar({ value }: { value: number }) {
  return (
    <div className="h-1.5 overflow-hidden rounded-full bg-neutral-200">
      <div className="h-full rounded-full bg-chinese-indigo transition-all" style={{ width: `${Math.round(value * 100)}%` }} />
    </div>
  );
}

function ToggleRow({
  title,
  desc,
  active,
  onToggle,
}: {
  title: string;
  desc: string;
  active: boolean;
  onToggle: () => void;
}) {
  return (
    <button onClick={onToggle} className="group flex w-full items-start justify-between gap-4 py-2 text-left">
      <div>
        <div className="text-sm text-neutral-900">{title}</div>
        <div className="mt-0.5 text-xs text-neutral-400">{desc}</div>
      </div>
      <div className="relative mt-1 h-4 w-7 flex-shrink-0">
        <div className="absolute left-0.5 right-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-200" />
        <div
          className={`absolute left-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-900 transition-all duration-200 ease-out ${
            active ? "w-6" : "w-0"
          }`}
        />
        <div
          className={`absolute top-0 h-4 w-4 transition-all duration-200 ease-out ${
            active ? "left-3.5 bg-neutral-900" : "left-0 border-2 border-neutral-300 bg-neutral-50"
          }`}
        />
      </div>
    </button>
  );
}

function MiniButton({
  onClick,
  icon,
  disabled = false,
  title,
}: {
  onClick: () => void;
  icon: ReactNode;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      className="inline-flex h-8 w-8 items-center justify-center text-neutral-400 transition-colors hover:bg-neutral-200 hover:text-neutral-700 disabled:cursor-not-allowed disabled:opacity-40"
    >
      {icon}
    </button>
  );
}

function Field({
  label,
  value,
  onChange,
  type = "text",
  placeholder,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  placeholder?: string;
}) {
  return (
    <div>
      <label className="mb-1 block text-xs text-neutral-400">{label}</label>
      <input
        type={type}
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
        className="input-underline w-full py-2 text-sm text-neutral-900"
      />
    </div>
  );
}

function NumberField({
  label,
  value,
  min,
  max,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}) {
  return (
    <div>
      <label className="mb-1 block text-xs text-neutral-400">{label}</label>
      <input
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(event) => onChange(parseInt(event.target.value) || 0)}
        className="input-underline w-full py-2 text-sm text-neutral-900"
      />
    </div>
  );
}

function Area({
  label,
  value,
  onChange,
  rows = 3,
  placeholder,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  rows?: number;
  placeholder?: string;
}) {
  return (
    <div>
      <label className="mb-1 block text-xs text-neutral-400">{label}</label>
      <textarea
        rows={rows}
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
        className="w-full resize-none bg-neutral-50 px-3 py-2 text-sm text-neutral-900 outline-none transition placeholder:text-neutral-300 focus:bg-white focus:shadow-[inset_0_0_0_1px_#d4d4d4]"
      />
    </div>
  );
}

function ActionButton({ children, onClick }: { children: ReactNode; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="px-2 py-1 text-sm text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-neutral-900"
    >
      {children}
    </button>
  );
}

function PrimaryButton({
  children,
  onClick,
  disabled = false,
}: {
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="inline-flex items-center justify-center gap-2 bg-neutral-900 px-3 py-1.5 text-sm font-medium text-neutral-50 transition-opacity hover:opacity-70 disabled:cursor-not-allowed disabled:opacity-40"
    >
      {children}
    </button>
  );
}

function ChipButton({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={`px-3 py-1.5 text-sm transition-colors ${
        active ? "bg-neutral-900 text-neutral-50" : "bg-neutral-100 text-neutral-500 hover:bg-neutral-200 hover:text-neutral-700"
      }`}
    >
      {label}
    </button>
  );
}
