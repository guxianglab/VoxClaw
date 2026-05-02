import { useEffect, useState } from "react";
import { MessageSquarePlus, RotateCw } from "lucide-react";
import { api } from "../lib/api";

/**
 * Compact bar shown when continuous-session mode is enabled.
 * Surfaces the current session id and lets the user start a fresh one.
 */
export function SessionBar() {
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = async () => {
    try {
      const id = await api.sessionCurrent();
      setSessionId(id);
    } catch {
      setSessionId(null);
    }
  };

  useEffect(() => {
    refresh();
    const t = window.setInterval(refresh, 4000);
    return () => window.clearInterval(t);
  }, []);

  const handleNew = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const id = await api.sessionNew();
      setSessionId(id);
    } finally {
      setBusy(false);
    }
  };

  const shortId = sessionId ? sessionId.slice(0, 8) : "未开启";

  return (
    <div className="flex items-center justify-between border-b border-neutral-200 py-2">
      <div className="flex items-center gap-2 text-xs">
        <span className="uppercase tracking-wider text-neutral-400">当前会话</span>
        <span className="font-mono text-neutral-700">{shortId}</span>
      </div>
      <div className="flex items-center gap-1">
        <button
          onClick={refresh}
          disabled={busy}
          className="flex items-center gap-1 px-2 py-1 text-xs text-neutral-500 transition-colors hover:bg-neutral-200"
          title="刷新"
        >
          <RotateCw className="h-3 w-3" />
        </button>
        <button
          onClick={handleNew}
          disabled={busy}
          className="flex items-center gap-1 px-2 py-1 text-xs text-neutral-500 transition-colors hover:bg-neutral-200"
        >
          <MessageSquarePlus className="h-3 w-3" />
          新建会话
        </button>
      </div>
    </div>
  );
}
