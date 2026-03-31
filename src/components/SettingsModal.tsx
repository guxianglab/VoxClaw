import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { AlertCircle, ChevronDown, ChevronRight, Copy, Loader2, Plus, RotateCcw, Trash2, X } from "lucide-react";
import {
  api,
  AppConfig,
  AudioDevice,
  BrowserSkillOptions,
  LlmConfig,
  PromptProfile,
  ProxyConfig,
  SafetyRule,
  SkillConfig,
  SkillSubCommandConfig,
  events,
} from "../lib/api";

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  isFirstSetup?: boolean;
}

type SettingsTab = "audio" | "models" | "polish" | "skills";

const TABS: Array<{ key: SettingsTab; label: string }> = [
  { key: "audio", label: "录音" },
  { key: "models", label: "模型" },
  { key: "polish", label: "润色" },
  { key: "skills", label: "技能" },
];

const toLines = (items: string[]) => items.join("\n");
const fromLines = (value: string) =>
  value
    .split("\n")
    .map((item) => item.trim())
    .filter(Boolean);

const toInlineList = (items: string[]) => items.join(" / ");
const fromInlineList = (value: string) =>
  value
    .split(/[\n,，/、]+/)
    .map((item) => item.trim())
    .filter(Boolean);

const cloneProfile = (profile: PromptProfile, patch: Partial<PromptProfile> = {}): PromptProfile => ({
  ...profile,
  voice_aliases: [...profile.voice_aliases],
  preserve_rules: [...profile.preserve_rules],
  glossary: [...profile.glossary],
  examples: profile.examples.map((example) => ({ ...example })),
  ...patch,
});

const makeProfileId = () => `profile_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;

const mergeDefaultProfiles = (profiles: PromptProfile[], defaults: PromptProfile[]) => {
  const existingIds = new Set(profiles.map((profile) => profile.id));
  const restored = defaults
    .filter((profile) => !existingIds.has(profile.id))
    .map((profile) => cloneProfile(profile));
  return [...profiles, ...restored];
};

const makeBrowserSiteId = () => `browser_site_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
const isBrowserSkill = (skill: SkillConfig) => skill.id === "open_browser";

const ensureBrowserOptions = (skill: SkillConfig): BrowserSkillOptions => ({
  llm_site_resolution_enabled: skill.browser_options?.llm_site_resolution_enabled ?? true,
  search_fallback_enabled: skill.browser_options?.search_fallback_enabled ?? true,
  search_url_template: skill.browser_options?.search_url_template ?? "https://www.bing.com/search?q={query}",
  sites: skill.browser_options?.sites ?? [],
});

export function SettingsModal({ isOpen, onClose, isFirstSetup = false }: SettingsModalProps) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [currentDevice, setCurrentDevice] = useState("");
  const [defaultProfiles, setDefaultProfiles] = useState<PromptProfile[]>([]);
  const [blankProfile, setBlankProfile] = useState<PromptProfile | null>(null);
  const [audioLevel, setAudioLevel] = useState(0);
  const [testingAudio, setTestingAudio] = useState(false);
  const [testingLlm, setTestingLlm] = useState(false);
  const [llmResult, setLlmResult] = useState<{ success: boolean; message: string } | null>(null);
  const [showWarning, setShowWarning] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [saveOk, setSaveOk] = useState(false);
  const [activeTab, setActiveTab] = useState<SettingsTab>("audio");
  const timerRef = useRef<number | null>(null);
  const pendingRef = useRef<AppConfig | null>(null);

  const saveLater = useCallback((next: AppConfig) => {
    pendingRef.current = next;
    if (timerRef.current) window.clearTimeout(timerRef.current);
    setIsSaving(true);
    timerRef.current = window.setTimeout(async () => {
      const finalConfig = pendingRef.current;
      if (finalConfig) {
        try {
          await api.saveConfig(finalConfig);
          setSaveOk(true);
          window.setTimeout(() => setSaveOk(false), 1500);
        } catch (error) {
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
    api.getDefaultSceneTemplate().then(setBlankProfile);
    setActiveTab("audio");
    setShowWarning(false);
    setLlmResult(null);
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
    const unsub = events.onConfigUpdated((nextConfig) => {
      pendingRef.current = null;
      setIsSaving(false);
      setConfig(nextConfig);
      setLlmResult(null);
    });
    return () => {
      unsub.then((fn) => fn());
    };
  }, [isOpen]);

  if (!isOpen || !config) return null;

  const updateConfig = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
    const next = { ...config, [key]: value };
    setConfig(next);
    saveLater(next);
  };

  const updateLlm = <K extends keyof LlmConfig>(key: K, value: LlmConfig[K]) => {
    const next = { ...config, llm_config: { ...config.llm_config, [key]: value } };
    setConfig(next);
    saveLater(next);
    setLlmResult(null);
  };

  const updateProxy = <K extends keyof ProxyConfig>(key: K, value: ProxyConfig[K]) => {
    updateConfig("proxy", { ...config.proxy, [key]: value });
  };

  const updateSkill = <K extends keyof SkillConfig>(id: string, key: K, value: SkillConfig[K]) => {
    updateConfig(
      "skills",
      config.skills.map((skill) => (skill.id === id ? { ...skill, [key]: value } : skill)),
    );
  };

  const updateAgentConfig = (key: string, value: string | number | boolean) => {
    const next = {
      ...config,
      agent_config: { ...config.agent_config, [key]: value },
    };
    setConfig(next);
    saveLater(next);
  };

  const updateBrowserOptions = (id: string, patch: Partial<BrowserSkillOptions>) => {
    updateConfig(
      "skills",
      config.skills.map((skill) =>
        skill.id === id
          ? { ...skill, browser_options: { ...ensureBrowserOptions(skill), ...patch } }
          : skill,
      ),
    );
  };

  const updateBrowserSubCommand = (
    skillId: string,
    commandId: string,
    patch: Partial<SkillSubCommandConfig>,
  ) => {
    updateConfig(
      "skills",
      config.skills.map((skill) =>
        skill.id === skillId
          ? {
              ...skill,
              sub_commands: skill.sub_commands.map((command) =>
                command.id === commandId ? { ...command, ...patch } : command,
              ),
            }
          : skill,
      ),
    );
  };

  const updateBrowserSite = (skillId: string, siteId: string, patch: Partial<{ name: string; aliases: string; url: string; enabled: boolean }>) => {
    updateConfig(
      "skills",
      config.skills.map((skill) =>
        skill.id === skillId
          ? {
              ...skill,
              browser_options: {
                ...ensureBrowserOptions(skill),
                sites: ensureBrowserOptions(skill).sites.map((site) =>
                  site.id === siteId ? { ...site, ...patch } : site,
                ),
              },
            }
          : skill,
      ),
    );
  };

  const addBrowserSite = (skillId: string) => {
    updateConfig(
      "skills",
      config.skills.map((skill) =>
        skill.id === skillId
          ? {
              ...skill,
              browser_options: {
                ...ensureBrowserOptions(skill),
                sites: [
                  ...ensureBrowserOptions(skill).sites,
                  {
                    id: makeBrowserSiteId(),
                    name: "",
                    aliases: "",
                    url: "",
                    enabled: true,
                  },
                ],
              },
            }
          : skill,
      ),
    );
  };

  const removeBrowserSite = (skillId: string, siteId: string) => {
    updateConfig(
      "skills",
      config.skills.map((skill) =>
        skill.id === skillId
          ? {
              ...skill,
              browser_options: {
                ...ensureBrowserOptions(skill),
                sites: ensureBrowserOptions(skill).sites.filter((site) => site.id !== siteId),
              },
            }
          : skill,
      ),
    );
  };

  const active =
    config.llm_config.profiles.find((profile) => profile.id === config.llm_config.active_profile_id) ??
    config.llm_config.profiles[0] ??
    null;

  const activeDefaultProfile =
    defaultProfiles.find((profile) => profile.id === active?.id) ??
    defaultProfiles.find((profile) => profile.preset_key === active?.preset_key) ??
    null;

  const updateActive = (patch: Partial<PromptProfile>) => {
    if (!active) return;
    updateLlm(
      "profiles",
      config.llm_config.profiles.map((profile) =>
        profile.id === active.id ? cloneProfile(profile, patch) : profile,
      ),
    );
  };

  const selectScene = (id: string) => {
    updateLlm("active_profile_id", id);
  };

  const createProfile = () => {
    const base = blankProfile ?? active ?? defaultProfiles[0];
    if (!base) return;

    const id = makeProfileId();
    const profile = cloneProfile(base, {
      id,
      name: `场景 ${config.llm_config.profiles.length + 1}`,
      voice_aliases: [],
      advanced_instruction: "",
      expert_mode: false,
      legacy_imported: false,
    });
    const next = {
      ...config,
      llm_config: {
        ...config.llm_config,
        profiles: [...config.llm_config.profiles, profile],
        active_profile_id: id,
      },
    };
    setConfig(next);
    saveLater(next);
    setLlmResult(null);
  };

  const copyProfile = () => {
    if (!active) return;
    const id = makeProfileId();
    const copied = cloneProfile(active, {
      id,
      name: `${active.name} 副本`,
    });
    const next = {
      ...config,
      llm_config: {
        ...config.llm_config,
        profiles: [...config.llm_config.profiles, copied],
        active_profile_id: id,
      },
    };
    setConfig(next);
    saveLater(next);
    setLlmResult(null);
  };

  const deleteProfile = () => {
    if (!active || config.llm_config.profiles.length <= 1) return;
    const profiles = config.llm_config.profiles.filter((profile) => profile.id !== active.id);
    const next = {
      ...config,
      llm_config: {
        ...config.llm_config,
        profiles,
        active_profile_id: profiles[0].id,
      },
    };
    setConfig(next);
    saveLater(next);
    setLlmResult(null);
  };

  const resetProfile = () => {
    if (!active) return;
    const base = activeDefaultProfile ?? blankProfile;
    if (!base) return;

    const keepLabel = !defaultProfiles.some((profile) => profile.id === active.id);
    updateActive({
      name: keepLabel ? active.name : base.name,
      voice_aliases: keepLabel ? [...active.voice_aliases] : [...base.voice_aliases],
      preset_key: base.preset_key,
      goal: base.goal,
      tone: base.tone,
      format_style: base.format_style,
      preserve_rules: [...base.preserve_rules],
      glossary: [],
      examples: [],
      advanced_instruction: "",
      expert_mode: false,
      legacy_imported: false,
    });
  };

  const restoreDefaults = () => {
    if (defaultProfiles.length === 0) return;
    const profiles = mergeDefaultProfiles(config.llm_config.profiles, defaultProfiles);
    const next = {
      ...config,
      llm_config: {
        ...config.llm_config,
        profiles,
        active_profile_id: config.llm_config.active_profile_id || profiles[0]?.id || "",
      },
    };
    setConfig(next);
    saveLater(next);
    setLlmResult(null);
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
    const ready = !!config.input_device && !!config.online_asr_config.app_key && !!config.online_asr_config.access_key && !!config.llm_config.base_url && !!config.llm_config.api_key;
    if (isFirstSetup && !ready) {
      setShowWarning(true);
      return;
    }
    await flush();
    onClose();
  };

  const saveText = isSaving ? "正在保存..." : saveOk ? "已保存" : "自动保存";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-neutral-950/20 p-4 backdrop-blur-sm" onClick={close}>
      <div
        className="flex max-h-[88vh] w-full max-w-5xl flex-col overflow-hidden bg-neutral-50 shadow-[0_4px_24px_rgba(0,0,0,0.06)]"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-neutral-200 px-6 py-5">
          <div>
            <h2 className="text-xl font-semibold text-neutral-900">{isFirstSetup ? "完成设置" : "设置"}</h2>
            <div className="mt-1 text-sm text-neutral-500">
              {isFirstSetup ? "先配置麦克风和模型服务" : saveText}
            </div>
          </div>
          <button
            onClick={close}
            className="inline-flex h-10 w-10 items-center justify-center text-neutral-400 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
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
                    activeTab === tab.key
                      ? "text-neutral-900"
                      : "text-neutral-500 hover:text-neutral-700"
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
                  <div className="space-y-3">
                    <ToggleRow
                      title="鼠标中键"
                      desc="单击切换听写，长按执行技能"
                      active={config.trigger_mouse}
                      onToggle={() => updateConfig("trigger_mouse", !config.trigger_mouse)}
                    />
                    <ToggleRow
                      title="右 Alt"
                      desc="单击切换听写，长按执行技能"
                      active={config.trigger_toggle}
                      onToggle={() => updateConfig("trigger_toggle", !config.trigger_toggle)}
                    />
                  </div>
                </Section>

                <Section title="输入设备">
                  <Surface>
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
                        <PrimaryButton onClick={toggleAudio}>
                          {testingAudio ? "停止测试" : "测试麦克风"}
                        </PrimaryButton>
                      </div>
                    </div>
                    <div className="mt-4 h-1.5 overflow-hidden rounded-full bg-neutral-200">
                      <div
                        className="h-full rounded-full bg-chinese-indigo transition-all"
                        style={{ width: `${audioLevel * 100}%` }}
                      />
                    </div>
                  </Surface>
                </Section>
              </div>
            )}

            {activeTab === "models" && (
              <div className="space-y-5">
                <Section title="流式语音识别">
                  <div className="mb-4 rounded-md bg-neutral-100 px-4 py-3">
                    <div className="flex items-center gap-2 text-sm">
                      <span className="font-medium text-neutral-700">豆包</span>
                      <span className="text-neutral-400">·</span>
                      <span className="text-neutral-500">火山引擎流式语音识别服务</span>
                    </div>
                  </div>
                  <div className="grid gap-3 md:grid-cols-2">
                    <Field
                      label="app_key"
                      value={config.online_asr_config.app_key}
                      onChange={(value) =>
                        updateConfig("online_asr_config", { ...config.online_asr_config, app_key: value })
                      }
                    />
                    <Field
                      label="access_key"
                      value={config.online_asr_config.access_key}
                      onChange={(value) =>
                        updateConfig("online_asr_config", { ...config.online_asr_config, access_key: value })
                      }
                    />
                    <Field
                      label="resource_id"
                      value={config.online_asr_config.resource_id}
                      onChange={(value) =>
                        updateConfig("online_asr_config", { ...config.online_asr_config, resource_id: value })
                      }
                    />
                  </div>
                </Section>

                <Section title="语言模型">
                  <div className="mb-4 rounded-md bg-neutral-100 px-4 py-3">
                    <div className="flex items-center gap-2 text-sm">
                      <span className="font-medium text-neutral-700">OpenAI 兼容</span>
                      <span className="text-neutral-400">·</span>
                      <span className="text-neutral-500">支持 OpenAI 格式的任意模型服务</span>
                    </div>
                  </div>
                  <div className="space-y-3">
                    <div className="grid gap-3 md:grid-cols-2">
                      <Field label="base_url" value={config.llm_config.base_url} onChange={(value) => updateLlm("base_url", value)} />
                      <Field label="model" value={config.llm_config.model} onChange={(value) => updateLlm("model", value)} />
                    </div>
                    <Field
                      label="api_key"
                      type="password"
                      value={config.llm_config.api_key}
                      onChange={(value) => updateLlm("api_key", value)}
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
                  </div>
                </Section>

                <Section title="网络代理">
                  <ToggleRow
                    title="启用代理"
                    desc="语音识别和语言模型共用同一个代理"
                    active={config.proxy.enabled}
                    onToggle={() => updateProxy("enabled", !config.proxy.enabled)}
                  />
                  {config.proxy.enabled && (
                    <div className="mt-3">
                      <Field label="代理地址" value={config.proxy.url} onChange={(value) => updateProxy("url", value)} />
                    </div>
                  )}
                </Section>
              </div>
            )}

            {activeTab === "polish" && (
              <div className="space-y-5">
                <Section title="启用功能">
                  <ToggleRow
                    title="强制开启润色"
                    desc="开启后所有普通听写都走润色；关闭后通过双击开始或双击结束决定本次是否润色"
                    active={config.llm_config.enabled}
                    onToggle={() => updateLlm("enabled", !config.llm_config.enabled)}
                  />
                </Section>

                <Section title="场景设置">
                  {active && (
                    <div className="space-y-4">
                      <Surface>
                        <div className="space-y-4">
                          <div className="flex flex-wrap gap-2">
                            {config.llm_config.profiles.map((profile) => (
                              <button
                                key={profile.id}
                                onClick={() => selectScene(profile.id)}
                                className={`px-3 py-1.5 text-sm transition-colors ${
                                  config.llm_config.active_profile_id === profile.id
                                    ? "bg-neutral-900 text-neutral-50"
                                    : "bg-neutral-100 text-neutral-500 hover:bg-neutral-200 hover:text-neutral-700"
                                }`}
                              >
                                {profile.name}
                              </button>
                            ))}
                          </div>

                          <div className="flex flex-wrap items-center gap-2">
                            <MiniButton onClick={createProfile} icon={<Plus className="h-4 w-4" />} title="新增场景" />
                            <MiniButton onClick={copyProfile} icon={<Copy className="h-4 w-4" />} title="复制当前场景" />
                            <MiniButton onClick={deleteProfile} icon={<Trash2 className="h-4 w-4" />} disabled={config.llm_config.profiles.length <= 1} title="删除当前场景" />
                            <MiniButton onClick={resetProfile} icon={<RotateCcw className="h-4 w-4" />} title="重置当前场景" />
                            <ActionButton onClick={restoreDefaults}>恢复默认场景</ActionButton>
                          </div>
                        </div>
                      </Surface>

                      <div className="flex items-center gap-2 text-xs text-neutral-400">
                        <span>{active.id}</span>
                        <span className="h-1 w-1 rounded-full bg-neutral-300" />
                        <span>{activeDefaultProfile ? "内置场景" : "自定义场景"}</span>
                      </div>

                      <div className="grid gap-3 md:grid-cols-2">
                        <Field label="场景名称" value={active.name} onChange={(value) => updateActive({ name: value })} />
                        <Field
                          label="语音别名"
                          value={toInlineList(active.voice_aliases)}
                          onChange={(value) => updateActive({ voice_aliases: fromInlineList(value) })}
                        />
                      </div>

                      <div className="grid gap-3 md:grid-cols-2">
                        <Area label="目标" value={active.goal} onChange={(value) => updateActive({ goal: value })} />
                        <Field label="语气" value={active.tone} onChange={(value) => updateActive({ tone: value })} />
                      </div>

                      <Area
                        label="输出格式"
                        value={active.format_style}
                        onChange={(value) => updateActive({ format_style: value })}
                      />

                      <div className="grid gap-3 lg:grid-cols-2">
                        <Area
                          label="保留规则"
                          value={toLines(active.preserve_rules)}
                          onChange={(value) => updateActive({ preserve_rules: fromLines(value) })}
                        />
                        <Area
                          label="术语表"
                          value={toLines(active.glossary)}
                          onChange={(value) => updateActive({ glossary: fromLines(value) })}
                        />
                      </div>

                      <ToggleRow
                        title="高级模式"
                        desc="追加更细的补充指令"
                        active={active.expert_mode}
                        onToggle={() => updateActive({ expert_mode: !active.expert_mode })}
                      />

                      {active.expert_mode && (
                        <Area
                          label="高级指令"
                          value={active.advanced_instruction}
                          onChange={(value) => updateActive({ advanced_instruction: value })}
                          rows={5}
                          placeholder="补充额外规则、语气要求、结构偏好或禁止改写项"
                        />
                      )}
                    </div>
                  )}
                </Section>
              </div>
            )}

            {activeTab === "skills" && (
              <div className="space-y-5">
                <Section title="长按模式">
                  <div className="space-y-3">
                    <div className="flex items-center gap-2">
                      {(["skill", "agent"] as const).map((mode) => (
                        <button
                          key={mode}
                          onClick={() => {
                            const next = { ...config, agent_config: { ...config.agent_config, mode } };
                            setConfig(next);
                            saveLater(next);
                          }}
                          className={`px-3 py-1.5 text-sm font-medium transition-opacity ${
                            config.agent_config.mode === mode
                              ? "bg-neutral-900 text-neutral-50"
                              : "text-neutral-500 hover:bg-neutral-200 hover:text-neutral-900"
                          }`}
                        >
                          {mode === "skill" ? "技能模式" : "AI Agent"}
                        </button>
                      ))}
                    </div>
                    <p className="text-xs text-neutral-400">
                      选择长按触发键时的处理方式。技能模式通过关键词匹配执行操作，AI Agent 通过大模型理解指令。
                    </p>
                  </div>
                </Section>
                {config.agent_config.mode === "agent" && (
                <>
                <Section title="Agent 配置">
                  <div className="space-y-4">
                    <div className="mb-4 bg-neutral-100 px-4 py-3">
                      <div className="flex items-center gap-2 text-sm">
                        <span className="font-medium text-neutral-700">LLM Provider</span>
                        <span className="text-neutral-400">·</span>
                        <span className="text-neutral-500">支持 OpenAI / Anthropic 格式的模型服务</span>
                      </div>
                    </div>
                    <div className="grid gap-3 md:grid-cols-2">
                      <Field
                        label="Base URL"
                        value={config.agent_config.provider_base_url || ""}
                        onChange={(value) => updateAgentConfig("provider_base_url", value)}
                        placeholder="留空则使用 LLM 设置中的 URL"
                      />
                      <Field
                        label="API Key"
                        value={config.agent_config.provider_api_key || ""}
                        onChange={(value) => updateAgentConfig("provider_api_key", value)}
                        type="password"
                      />
                      <Field
                        label="Model"
                        value={config.agent_config.provider_model || ""}
                        onChange={(value) => updateAgentConfig("provider_model", value)}
                        placeholder="留空则使用 LLM 设置中的模型"
                      />
                      <Field
                        label="Thinking Level"
                        value={config.agent_config.thinking_level || "none"}
                        onChange={(value) => updateAgentConfig("thinking_level", value)}
                      />
                    </div>

                    <ToggleRow
                      title="Confirm dangerous operations"
                      desc="执行危险操作前弹窗确认"
                      active={config.agent_config.confirm_dangerous}
                      onToggle={() => updateAgentConfig("confirm_dangerous", !config.agent_config.confirm_dangerous)}
                    />

                    <Area
                      label="系统提示词"
                      value={config.agent_config.system_prompt || ""}
                      onChange={(value) => updateAgentConfig("system_prompt", value)}
                      rows={4}
                      placeholder="留空使用内置默认提示词。可自定义 Agent 的人格、行为规则和输出格式。"
                    />

                    <Area
                      label="常驻上下文"
                      value={config.agent_config.persistent_context || ""}
                      onChange={(value) => updateAgentConfig("persistent_context", value)}
                      rows={3}
                      placeholder="每次 Agent 执行时都会注入的上下文信息，如当前项目路径、常用缩写等。"
                    />

                    <ToggleRow
                      title="自动注入环境信息"
                      desc="自动追加操作系统、用户名等信息到系统提示词"
                      active={config.agent_config.auto_inject_env}
                      onToggle={() => updateAgentConfig("auto_inject_env", !config.agent_config.auto_inject_env)}
                    />

                    <div className="flex items-center justify-between py-2">
                      <div>
                        <div className="text-sm text-neutral-900">Max Iterations</div>
                        <div className="mt-0.5 text-xs text-neutral-400">最大迭代次数</div>
                      </div>
                      <input
                        type="number"
                        min="1"
                        max="50"
                        value={config.agent_config.max_iterations}
                        onChange={(e) => updateAgentConfig("max_iterations", parseInt(e.target.value) || 10)}
                        className="input-underline w-20 py-2 text-right text-sm text-neutral-900"
                      />
                    </div>

                    <div className="flex items-center justify-between py-2">
                      <div>
                        <div className="text-sm text-neutral-900">历史上下文条数</div>
                        <div className="mt-0.5 text-xs text-neutral-400">注入最近 N 次对话摘要，0 为关闭</div>
                      </div>
                      <input
                        type="number"
                        min="0"
                        max="20"
                        value={config.agent_config.context_history_count ?? 0}
                        onChange={(e) => updateAgentConfig("context_history_count", parseInt(e.target.value) || 0)}
                        className="input-underline w-20 py-2 text-right text-sm text-neutral-900"
                      />
                    </div>
                  </div>
                </Section>
                <Section title="安全规则">
                  <div className="space-y-4">
                    <div className="space-y-2">
                      <label className="mb-1 block text-xs text-neutral-400">默认策略</label>
                      <div className="flex gap-4">
                        {(["confirm", "deny", "allow"] as const).map((policy) => (
                          <label key={policy} className="flex items-center gap-1.5 text-sm cursor-pointer">
                            <input
                              type="radio"
                              name="safety_policy"
                              value={policy}
                              checked={config.agent_config.default_safety_policy === policy}
                              onChange={() => updateAgentConfig("default_safety_policy", policy)}
                              className="accent-neutral-900"
                            />
                            {policy === "confirm" ? "弹窗确认" : policy === "deny" ? "自动拒绝" : "自动允许"}
                          </label>
                        ))}
                      </div>
                      <p className="text-xs text-neutral-400">未命中任何规则时的处理方式</p>
                    </div>

                    <div className="space-y-2">
                      <div className="flex items-center justify-between">
                        <label className="mb-1 block text-xs text-neutral-400">规则列表</label>
                        <ActionButton
                          onClick={() => {
                            const newRule: SafetyRule = { tool: "*", action: "deny" };
                            const next = {
                              ...config,
                              agent_config: {
                                ...config.agent_config,
                                safety_rules: [...config.agent_config.safety_rules, newRule],
                              },
                            };
                            setConfig(next);
                            saveLater(next);
                          }}
                        >
                          + 添加规则
                        </ActionButton>
                      </div>
                      {config.agent_config.safety_rules.length === 0 && (
                        <p className="text-xs text-neutral-400">暂无规则，所有操作将按默认策略处理</p>
                      )}
                      {config.agent_config.safety_rules.map((rule, idx) => (
                        <div key={idx} className="flex items-center gap-2 text-sm py-2">
                          <span className="text-neutral-900">{rule.tool}</span>
                          {rule.command_pattern && (
                            <span className="text-neutral-500">{rule.command_pattern}</span>
                          )}
                          {rule.path_scope && rule.path_scope.length > 0 && (
                            <span className="text-neutral-500">path: {rule.path_scope.join(", ")}</span>
                          )}
                          <span className={`ml-auto text-xs ${
                            rule.action === "allow" ? "text-emerald-600" : "text-red-600"
                          }`}>
                            {rule.action === "allow" ? "允许" : "拒绝"}
                          </span>
                          <button
                            onClick={() => {
                              const rules = [...config.agent_config.safety_rules];
                              rules.splice(idx, 1);
                              const next = {
                                ...config,
                                agent_config: { ...config.agent_config, safety_rules: rules },
                              };
                              setConfig(next);
                              saveLater(next);
                            }}
                            className="text-neutral-400 hover:text-red-500 text-lg leading-none"
                          >
                            ×
                          </button>
                        </div>
                      ))}
                    </div>
                  </div>
                </Section>
                </>
                )}
                {config.agent_config.mode === "skill" && (
                <Section title="技能列表">
                  <div className="space-y-3">
                    {config.skills.map((skill) => (
                      <SkillCard
                        key={skill.id}
                        skill={skill}
                        onToggle={() => updateSkill(skill.id, "enabled", !skill.enabled)}
                        onKeywordsChange={(value) => updateSkill(skill.id, "keywords", value)}
                        onSubCommandChange={(commandId, patch) =>
                          updateBrowserSubCommand(skill.id, commandId, patch)
                        }
                        onBrowserOptionChange={(patch) => updateBrowserOptions(skill.id, patch)}
                        onBrowserSiteChange={(siteId, patch) => updateBrowserSite(skill.id, siteId, patch)}
                        onAddBrowserSite={() => addBrowserSite(skill.id)}
                        onRemoveBrowserSite={(siteId) => removeBrowserSite(skill.id, siteId)}
                      />
                    ))}
                  </div>
                </Section>
                )}
              </div>
            )}
          </div>
        </div>
      </div>

      {showWarning && (
        <div className="fixed inset-0 z-[60] flex items-center justify-center bg-neutral-950/25 p-4 backdrop-blur-sm">
          <div
            className="w-full max-w-sm bg-neutral-50 p-6 shadow-[0_4px_24px_rgba(0,0,0,0.06)]"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="flex items-center gap-3">
              <div className="flex h-10 w-10 items-center justify-center bg-amber-100 text-amber-600">
                <AlertCircle className="h-5 w-5" />
              </div>
              <div>
                <h3 className="text-base font-semibold text-neutral-900">还没有完成设置</h3>
                <p className="mt-1 text-sm text-neutral-500">请先选择输入设备，并填写语音识别和语言模型的凭证。</p>
              </div>
            </div>
            <div className="mt-6 flex justify-end">
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

function Surface({ children }: { children: ReactNode }) {
  return <div className="py-2">{children}</div>;
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
    <button
      onClick={onToggle}
      className="group flex w-full items-start justify-between gap-4 py-2 text-left"
    >
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
            active
              ? "left-3.5 bg-neutral-900"
              : "left-0 border-2 border-neutral-300 bg-neutral-50"
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

function ActionButton({
  children,
  onClick,
}: {
  children: ReactNode;
  onClick: () => void;
}) {
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

function SkillCard({
  skill,
  onToggle,
  onKeywordsChange,
  onSubCommandChange,
  onBrowserOptionChange,
  onBrowserSiteChange,
  onAddBrowserSite,
  onRemoveBrowserSite,
}: {
  skill: SkillConfig;
  onToggle: () => void;
  onKeywordsChange: (value: string) => void;
  onSubCommandChange: (commandId: string, patch: Partial<SkillSubCommandConfig>) => void;
  onBrowserOptionChange: (patch: Partial<BrowserSkillOptions>) => void;
  onBrowserSiteChange: (
    siteId: string,
    patch: Partial<{ name: string; aliases: string; url: string; enabled: boolean }>,
  ) => void;
  onAddBrowserSite: () => void;
  onRemoveBrowserSite: (siteId: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [subSkillsExpanded, setSubSkillsExpanded] = useState(false);
  const browserOptions = ensureBrowserOptions(skill);
  const browserSkill = isBrowserSkill(skill);
  const hasSubCommands = skill.sub_commands.length > 0;

  return (
    <div className="border-b border-neutral-200">
      <div className="flex items-center justify-between gap-4 py-3">
        <button
          onClick={() => setExpanded(!expanded)}
          className="flex flex-1 items-center gap-2 text-left"
        >
          {expanded ? (
            <ChevronDown className="h-4 w-4 flex-shrink-0 text-neutral-400" />
          ) : (
            <ChevronRight className="h-4 w-4 flex-shrink-0 text-neutral-400" />
          )}
          <span className="text-sm text-neutral-900">{skill.name}</span>
        </button>
        <button
          onClick={onToggle}
          className="relative h-4 w-7 flex-shrink-0"
        >
          <div className="absolute left-0.5 right-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-200" />
          <div
            className={`absolute left-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-900 transition-all duration-200 ease-out ${
              skill.enabled ? "w-6" : "w-0"
            }`}
          />
          <div
            className={`absolute top-0 h-4 w-4 transition-all duration-200 ease-out ${
              skill.enabled
                ? "left-3.5 bg-neutral-900"
                : "left-0 border-2 border-neutral-300 bg-neutral-50"
            }`}
          />
        </button>
      </div>
      {expanded && (
        <div className="pb-4 pl-6">
          <div className="mb-3 text-xs text-neutral-400">命中后执行，不粘贴文本</div>
          <Field
            label="关键词"
            value={skill.keywords}
            onChange={onKeywordsChange}
          />

          {hasSubCommands && (
            <div className="mt-6 space-y-5">
              <div className="space-y-3">
                <button
                  onClick={() => setSubSkillsExpanded((value) => !value)}
                  className="flex w-full items-center justify-between py-2 text-left"
                >
                  <div>
                    <div className="text-xs font-medium uppercase tracking-wider text-neutral-400">子技能</div>
                  </div>
                  {subSkillsExpanded ? (
                    <ChevronDown className="h-4 w-4 flex-shrink-0 text-neutral-400" />
                  ) : (
                    <ChevronRight className="h-4 w-4 flex-shrink-0 text-neutral-400" />
                  )}
                </button>

                {subSkillsExpanded && (
                  <div className="grid gap-3 md:grid-cols-2">
                    {skill.sub_commands.map((command) => (
                      <div key={command.id} className="py-2">
                        <div className="flex items-center justify-between gap-3">
                          <div>
                            <div className="text-sm text-neutral-900">{command.name}</div>
                            <div className="mt-0.5 text-xs text-neutral-400">{command.id}</div>
                          </div>
                          <button
                            onClick={() => onSubCommandChange(command.id, { enabled: !command.enabled })}
                            className="relative h-4 w-7 flex-shrink-0"
                          >
                            <div className="absolute left-0.5 right-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-200" />
                            <div
                              className={`absolute left-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-900 transition-all duration-200 ease-out ${
                                command.enabled ? "w-6" : "w-0"
                              }`}
                            />
                            <div
                              className={`absolute top-0 h-4 w-4 transition-all duration-200 ease-out ${
                                command.enabled
                                  ? "left-3.5 bg-neutral-900"
                                  : "left-0 border-2 border-neutral-300 bg-neutral-50"
                              }`}
                            />
                          </button>
                        </div>
                        <div className="mt-3">
                          <label className="mb-1 block text-xs text-neutral-400">关键词</label>
                          <input
                            value={command.keywords}
                            onChange={(event) => onSubCommandChange(command.id, { keywords: event.target.value })}
                            className="input-underline w-full py-2 text-sm text-neutral-900"
                          />
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>

              {browserSkill && (
                <div className="space-y-3">
                <div className="border-b border-neutral-200 pb-2 text-xs font-medium uppercase tracking-wider text-neutral-400">网址解析</div>
                <ToggleRow
                  title="未命中时使用 LLM 解析"
                  desc="借助已配置的大模型把站点名称转成公开网址"
                  active={browserOptions.llm_site_resolution_enabled}
                  onToggle={() =>
                    onBrowserOptionChange({
                      llm_site_resolution_enabled: !browserOptions.llm_site_resolution_enabled,
                    })
                  }
                />
                <ToggleRow
                  title="解析失败时改为搜索"
                  desc="当网址无法精确解析时，用搜索引擎打开原始关键词"
                  active={browserOptions.search_fallback_enabled}
                  onToggle={() =>
                    onBrowserOptionChange({
                      search_fallback_enabled: !browserOptions.search_fallback_enabled,
                    })
                  }
                />
                <Field
                  label="搜索 URL 模板"
                  value={browserOptions.search_url_template}
                  onChange={(value) => onBrowserOptionChange({ search_url_template: value })}
                />
                </div>
              )}

              {browserSkill && (
                <div className="space-y-3">
                <div className="flex items-center justify-between gap-3">
                  <div className="border-b border-neutral-200 pb-2 text-xs font-medium uppercase tracking-wider text-neutral-400">站点映射</div>
                  <ActionButton onClick={onAddBrowserSite}>新增站点</ActionButton>
                </div>
                {browserOptions.sites.length === 0 ? (
                  <div className="text-xs text-neutral-400">还没有站点映射，未命中时会优先走 LLM 或搜索兜底。</div>
                ) : (
                  browserOptions.sites.map((site) => (
                    <div key={site.id} className="space-y-3 py-2">
                      <div className="flex items-center justify-between gap-3">
                        <div className="text-xs text-neutral-400">{site.id}</div>
                        <div className="flex items-center gap-2">
                          <button
                            onClick={() => onBrowserSiteChange(site.id, { enabled: !site.enabled })}
                            className="relative h-4 w-7 flex-shrink-0"
                          >
                            <div className="absolute left-0.5 right-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-200" />
                            <div
                              className={`absolute left-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-900 transition-all duration-200 ease-out ${
                                site.enabled ? "w-6" : "w-0"
                              }`}
                            />
                            <div
                              className={`absolute top-0 h-4 w-4 transition-all duration-200 ease-out ${
                                site.enabled
                                  ? "left-3.5 bg-neutral-900"
                                  : "left-0 border-2 border-neutral-300 bg-neutral-50"
                              }`}
                            />
                          </button>
                          <MiniButton
                            onClick={() => onRemoveBrowserSite(site.id)}
                            icon={<Trash2 className="h-4 w-4" />}
                            title="删除站点"
                          />
                        </div>
                      </div>
                      <div className="grid gap-3 md:grid-cols-2">
                        <Field
                          label="名称"
                          value={site.name}
                          onChange={(value) => onBrowserSiteChange(site.id, { name: value })}
                        />
                        <Field
                          label="别名"
                          value={site.aliases}
                          onChange={(value) => onBrowserSiteChange(site.id, { aliases: value })}
                        />
                      </div>
                      <Field
                        label="URL"
                        value={site.url}
                        onChange={(value) => onBrowserSiteChange(site.id, { url: value })}
                      />
                    </div>
                  ))
                )}
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

