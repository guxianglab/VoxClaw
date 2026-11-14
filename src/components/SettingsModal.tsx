import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { AiPane, AssistantPane, GeneralPane, PolishPane, SpeechPane } from "./settings/panes";
import { isAiReady, isSpeechReady, useConfigDraft } from "./settings/useConfigDraft";
import { Notice } from "./ui";

export type SettingsTab = "general" | "speech" | "ai" | "polish" | "assistant";

const TABS: Array<{ key: SettingsTab; label: string; desc: string }> = [
  { key: "general", label: "常规", desc: "快捷键、麦克风" },
  { key: "speech", label: "语音识别", desc: "语音转文字（豆包 / 离线）" },
  { key: "ai", label: "AI 模型", desc: "润色、助手（大语言模型）" },
  { key: "polish", label: "润色", desc: "把口语整理成书面语" },
  { key: "assistant", label: "AI 助手", desc: "长按说出指令" },
];

interface SettingsModalProps {
  isOpen: boolean;
  initialTab?: SettingsTab;
  onClose: () => void;
}

export function SettingsModal({ isOpen, initialTab = "general", onClose }: SettingsModalProps) {
  const { config, update, flush, saveState, saveError, saveNotice } = useConfigDraft(isOpen);
  const [tab, setTab] = useState<SettingsTab>(initialTab);

  useEffect(() => {
    if (isOpen) setTab(initialTab);
  }, [isOpen, initialTab]);

  if (!isOpen || !config) return null;

  const close = async () => {
    await flush();
    onClose();
  };

  // A small dot marks sections that still need attention.
  const needsAttention: Partial<Record<SettingsTab, boolean>> = {
    speech: !isSpeechReady(config),
    ai: !isAiReady(config),
  };

  const saveText = { idle: "", saving: "保存中…", saved: "已保存", error: "保存失败" }[saveState];
  const pane = { config, update };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-neutral-950/20 p-4" onClick={close}>
      <div
        className="flex h-[min(720px,90vh)] w-full max-w-4xl overflow-hidden bg-neutral-50"
        onClick={(event) => event.stopPropagation()}
      >
        <aside className="flex w-44 flex-shrink-0 flex-col border-r border-neutral-200 py-6">
          <h2 className="px-5 pb-5 text-lg font-semibold text-neutral-900">设置</h2>
          <nav className="flex flex-col">
            {TABS.map((t) => (
              <button
                key={t.key}
                onClick={() => setTab(t.key)}
                className={`border-l-2 px-5 py-2.5 text-left transition-colors ${
                  tab === t.key ? "border-chinese-indigo bg-neutral-100" : "border-transparent hover:bg-neutral-100"
                }`}
              >
                <div className="flex items-center gap-2 text-sm text-neutral-900">
                  {t.label}
                  {needsAttention[t.key] && <span className="h-1.5 w-1.5 rounded-full bg-amber-500" />}
                </div>
                <div className="mt-0.5 text-xs text-neutral-400">{t.desc}</div>
              </button>
            ))}
          </nav>
          <div className={`mt-auto px-5 text-xs ${saveState === "error" ? "text-red-600" : "text-neutral-400"}`}>
            {saveText}
          </div>
        </aside>

        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex justify-end px-3 pt-3">
            <button
              onClick={close}
              className="inline-flex h-8 w-8 items-center justify-center text-neutral-400 transition-colors hover:bg-neutral-200 hover:text-neutral-700"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
          <div className="custom-scrollbar min-h-0 flex-1 overflow-y-auto px-8 pb-8">
            {(saveError || saveNotice) && (
              <div className="mb-6">
                <Notice tone={saveError ? "red" : "amber"}>{saveError ?? saveNotice}</Notice>
              </div>
            )}
            {tab === "general" && <GeneralPane {...pane} />}
            {tab === "speech" && <SpeechPane {...pane} />}
            {tab === "ai" && <AiPane {...pane} />}
            {tab === "polish" && <PolishPane {...pane} onSetupAi={() => setTab("ai")} />}
            {tab === "assistant" && <AssistantPane {...pane} onSetupAi={() => setTab("ai")} />}
          </div>
        </div>
      </div>
    </div>
  );
}
