import { type ReactNode, useState, useEffect } from "react";
import { AlertCircle, Sparkles, Wrench, X } from "lucide-react";
import { HistoryList } from "./components/HistoryList";
import { SettingsModal } from "./components/SettingsModal";
import { StatusSection } from "./components/StatusSection";
import { TitleBar } from "./components/TitleBar";
import { api, events, VoiceCommandFeedback } from "./lib/api";
import "./index.css";

function App() {
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [asrConfigured, setAsrConfigured] = useState(false);
  const [needsSetup, setNeedsSetup] = useState(false);
  const [llmError, setLlmError] = useState<string | null>(null);
  const [runtimeNotice, setRuntimeNotice] = useState<string | null>(null);
  const [voiceFeedback, setVoiceFeedback] = useState<VoiceCommandFeedback | null>(null);

  useEffect(() => {
    api.getAsrStatus().then(status => {
      setAsrConfigured(status.configured);
    });

    api.takeRuntimeNotice().then((notice) => {
      if (notice) {
        setRuntimeNotice(notice);
      }
    });

    Promise.all([api.getConfig(), api.getAsrStatus()]).then(([config, asrStatus]) => {
      const noDevice = !config.input_device || config.input_device === "";
      const noAsr = !asrStatus.configured;
      const noLlm = !config.llm_config.base_url || !config.llm_config.api_key;
      if (noDevice || noAsr || noLlm) {
        setNeedsSetup(true);
        setIsSettingsOpen(true);
      }
    });

    const unsubLlmError = events.onLlmError((message) => {
      setLlmError(message);
    });
    const unsubVoiceFeedback = events.onVoiceCommandFeedback(setVoiceFeedback);

    return () => {
      unsubLlmError.then(f => f());
      unsubVoiceFeedback.then(f => f());
    };
  }, []);

  useEffect(() => {
    if (!llmError) return;

    const timeout = window.setTimeout(() => {
      setLlmError(null);
    }, 8000);

    return () => window.clearTimeout(timeout);
  }, [llmError]);

  useEffect(() => {
    if (!voiceFeedback) return;

    const timeout = window.setTimeout(() => {
      setVoiceFeedback(null);
    }, 5000);

    return () => window.clearTimeout(timeout);
  }, [voiceFeedback]);

  const handleSettingsClose = () => {
    if (needsSetup) {
      Promise.all([api.getConfig(), api.getAsrStatus()]).then(([config, asrStatus]) => {
        const hasDevice = config.input_device && config.input_device !== "";
        const hasAsr = asrStatus.configured;
        const hasLlm = config.llm_config.base_url && config.llm_config.api_key;
        setAsrConfigured(hasAsr);
        if (hasDevice && hasAsr && hasLlm) {
          setNeedsSetup(false);
          setIsSettingsOpen(false);
        }
      });
    } else {
      api.getAsrStatus().then(status => setAsrConfigured(status.configured));
      setIsSettingsOpen(false);
    }
  };

  return (
    <div className="flex h-screen w-full overflow-hidden bg-neutral-100 text-neutral-900 selection:bg-chinese-indigo/15">
      <TitleBar />
      <div className="mx-auto flex h-full w-full max-w-6xl flex-1 flex-col px-4 pb-4 pt-8 md:px-6 md:pt-10">
        <div className="flex flex-col gap-3 pb-4">
          <StatusSection
            asrConfigured={asrConfigured}
            onSettingsClick={() => setIsSettingsOpen(true)}
          />

          {(runtimeNotice || llmError || voiceFeedback) && (
            <div className="flex flex-col gap-2">
              {runtimeNotice && (
                <NoticeBar
                  tone="amber"
                  icon={<Wrench className="h-4 w-4" />}
                  title="系统提示"
                  message={runtimeNotice}
                  onClose={() => setRuntimeNotice(null)}
                />
              )}
              {llmError && (
                <NoticeBar
                  tone="red"
                  icon={<AlertCircle className="h-4 w-4" />}
                  title="润色异常"
                  message={llmError}
                  onClose={() => setLlmError(null)}
                />
              )}
              {voiceFeedback && (
                <NoticeBar
                  tone={
                    voiceFeedback.level === "error"
                      ? "red"
                      : voiceFeedback.level === "success"
                        ? "emerald"
                        : "blue"
                  }
                  icon={<Sparkles className="h-4 w-4" />}
                  title="语音控制"
                  message={voiceFeedback.message}
                  onClose={() => setVoiceFeedback(null)}
                />
              )}
            </div>
          )}
        </div>

        <div className="min-h-0 flex-1">
          <HistoryList />
        </div>
      </div>

      <SettingsModal isOpen={isSettingsOpen} onClose={handleSettingsClose} isFirstSetup={needsSetup} />
    </div>
  );
}

function NoticeBar({
  tone,
  icon,
  title,
  message,
  onClose,
}: {
  tone: "amber" | "red" | "emerald" | "blue";
  icon: ReactNode;
  title: string;
  message: string;
  onClose: () => void;
}) {
  const barColor =
    tone === "amber"
      ? "bg-amber-500"
      : tone === "emerald"
        ? "bg-emerald-500"
        : tone === "blue"
          ? "bg-sky-500"
          : "bg-red-500";

  return (
    <div className="relative flex items-start gap-3 overflow-hidden bg-neutral-50 py-3 pr-4 pl-5">
      <div className={`absolute left-0 top-0 h-full w-1 ${barColor}`} />
      <div className="mt-0.5 flex-shrink-0 text-neutral-400">{icon}</div>
      <div className="min-w-0 flex-1">
        <div className="text-xs font-medium uppercase tracking-wider text-neutral-400">{title}</div>
        <div className="mt-1 text-sm leading-6 text-neutral-600">{message}</div>
      </div>
      <button
        onClick={onClose}
        className="p-1 text-neutral-400 transition-colors hover:bg-neutral-200 hover:text-neutral-600"
      >
        <X className="h-4 w-4" />
      </button>
    </div>
  );
}

export default App;
