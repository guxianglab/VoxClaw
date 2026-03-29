import { useEffect, useRef, useState } from "react";
import { Copy, Trash2, X } from "lucide-react";
import { api, events, HistoryItem } from "../lib/api";

export function HistoryList() {
  const [items, setItems] = useState<HistoryItem[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.getHistory().then(setItems);
    const unsubscribe = events.onTranscriptionUpdate((item) => {
      setItems((prev) => [item, ...prev]);
    });
    return () => {
      unsubscribe.then((fn) => fn());
    };
  }, []);

  const copyText = (text: string) => {
    navigator.clipboard.writeText(text);
  };

  const deleteItem = async (id: string) => {
    await api.deleteHistoryItem(id);
    setItems((prev) => prev.filter((item) => item.id !== id));
  };

  const clearAll = async () => {
    if (window.confirm("确定清空全部记录吗？")) {
      await api.clearHistory();
      setItems([]);
    }
  };

  return (
    <section className="flex h-full min-h-[280px] flex-col overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between pb-3">
        <h2 className="text-xs font-medium uppercase tracking-wider text-neutral-400">转写记录</h2>
        <div className="flex items-center gap-2">
          <span className="text-xs text-neutral-300">{items.length} 条</span>
          {items.length > 0 && (
            <button
              onClick={clearAll}
              className="p-1 text-neutral-400 transition-colors hover:text-red-500"
              title="清空"
            >
              <Trash2 className="h-4 w-4" />
            </button>
          )}
        </div>
      </div>

      {/* Divider */}
      <div className="border-b border-neutral-200" />

      {/* Content */}
      <div ref={scrollRef} className="custom-scrollbar mt-4 flex-1 overflow-y-auto">
        {items.length === 0 ? (
          <div className="flex h-full min-h-[200px] flex-col items-center justify-center text-center">
            <div className="text-sm text-neutral-400">暂无记录</div>
            <div className="mt-1 text-xs text-neutral-300">开始说话后会显示在这里</div>
          </div>
        ) : (
          <div className="-mt-px divide-y divide-neutral-200">
            {items.map((item) => (
              <article
                key={item.id}
                className="group flex items-start justify-between gap-4 py-4 transition-colors hover:bg-neutral-100"
              >
                <div className="min-w-0 flex-1">
                  <p className="line-clamp-3 text-sm leading-relaxed text-neutral-800">{item.text}</p>
                  <div className="mt-1.5 text-xs text-neutral-400">{item.timestamp}</div>
                </div>

                <div className="flex shrink-0 items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100">
                  <button
                    onClick={() => copyText(item.text)}
                    className="p-1 text-neutral-400 transition-colors hover:text-neutral-600"
                    title="复制"
                  >
                    <Copy className="h-4 w-4" />
                  </button>
                  <button
                    onClick={() => deleteItem(item.id)}
                    className="p-1 text-neutral-400 transition-colors hover:text-red-500"
                    title="删除"
                  >
                    <X className="h-4 w-4" />
                  </button>
                </div>
              </article>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
