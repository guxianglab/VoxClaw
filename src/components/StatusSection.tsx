import { Settings2 } from "lucide-react";
import type { AppConfig } from "../lib/api";
import { Key, PrimaryButton } from "./ui";

interface StatusSectionProps {
  config: AppConfig | null;
  speechReady: boolean;
  onSetup: () => void;
  onSettingsClick: () => void;
}

export function StatusSection({ config, speechReady, onSetup, onSettingsClick }: StatusSectionProps) {
  const keys = config
    ? [config.trigger_mouse && "鼠标中键", config.trigger_toggle && "右 Alt"].filter(Boolean).join(" / ")
    : "";

  return (
    <section className="flex items-center justify-between gap-4 py-4">
      <div data-tauri-drag-region className="flex min-w-0 items-center gap-3">
        <span className={`h-2 w-2 flex-shrink-0 rounded-full ${speechReady ? "bg-emerald-500" : "bg-amber-500"}`} />
        {speechReady ? (
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-neutral-500">
            {keys && <Key>{keys}</Key>}
            <span>单击听写</span>
            <span className="text-neutral-300">·</span>
            <span>双击润色</span>
            <span className="text-neutral-300">·</span>
            <span>长按 AI 助手</span>
          </div>
        ) : (
          <div className="flex items-center gap-3">
            <span className="text-sm text-neutral-900">还差一步：设置语音识别</span>
            <PrimaryButton onClick={onSetup}>继续设置</PrimaryButton>
          </div>
        )}
      </div>

      <button
        onClick={onSettingsClick}
        className="inline-flex flex-shrink-0 items-center gap-1.5 px-2 py-1 text-sm text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-neutral-900"
      >
        <Settings2 className="h-4 w-4" />
        设置
      </button>
    </section>
  );
}
