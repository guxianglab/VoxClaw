import { Settings2 } from "lucide-react";
import { useEffect, useState } from "react";
import { events } from "../lib/api";

interface StatusSectionProps {
  asrConfigured: boolean;
  onSettingsClick: () => void;
}

export function StatusSection({ asrConfigured, onSettingsClick }: StatusSectionProps) {
  const [isRecording, setIsRecording] = useState(false);

  useEffect(() => {
    const unsub = events.onRecordingStatus(setIsRecording);
    return () => { unsub.then(f => f()); };
  }, []);

  const statusText = isRecording ? "录音中" : asrConfigured ? "待机中" : "需要完成设置";
  const helperText = isRecording
    ? "松开后自动转写"
    : asrConfigured
      ? "单击开始，双击触发润色，长按执行技能"
      : "先配置麦克风和识别服务";

  return (
    <section className="flex items-center justify-between py-4">
      <div data-tauri-drag-region className="flex items-center gap-4">
        <div className="relative flex h-3 w-3 items-center justify-center">
          {isRecording ? (
            <>
              <span className="absolute inset-0 animate-ping rounded-full bg-chinese-indigo opacity-75" />
              <span className="relative h-2 w-2 rounded-full bg-chinese-indigo" />
            </>
          ) : asrConfigured ? (
            <span className="h-2 w-2 rounded-full bg-emerald-500" />
          ) : (
            <span className="h-2 w-2 rounded-full bg-neutral-300" />
          )}
        </div>

        <div>
          <div className="text-sm text-neutral-900">{statusText}</div>
          <div className="text-xs text-neutral-400">{helperText}</div>
        </div>
      </div>

      <button
        onClick={onSettingsClick}
        className="p-1 text-neutral-400 transition-colors hover:bg-neutral-200 hover:text-neutral-700"
      >
        <Settings2 className="h-4 w-4" />
      </button>
    </section>
  );
}
