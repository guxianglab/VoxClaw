import { useState } from "react";
import { MessageSquarePlus } from "lucide-react";
import { api } from "../lib/api";

/**
 * Shown when continuous conversation is on: the assistant remembers earlier
 * commands until the user starts over.
 */
export function SessionBar() {
  const [justReset, setJustReset] = useState(false);

  const startOver = async () => {
    await api.sessionNew();
    setJustReset(true);
    window.setTimeout(() => setJustReset(false), 2000);
  };

  return (
    <div className="flex items-center justify-between py-1 text-xs text-neutral-400">
      <span>连续对话已开启，AI 助手会记住之前的指令</span>
      <button
        onClick={startOver}
        className="inline-flex items-center gap-1 px-2 py-1 text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-neutral-900"
      >
        <MessageSquarePlus className="h-3.5 w-3.5" />
        {justReset ? "已开始新对话" : "开始新对话"}
      </button>
    </div>
  );
}
