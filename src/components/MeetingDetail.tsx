import { ArrowLeft, Copy, Loader2, Sparkles, Trash2 } from "lucide-react";
import type { MeetingRecord, MeetingSummary } from "../lib/api";
import { formatDuration } from "./MeetingView";

function summaryToText(summary: MeetingSummary): string {
    const list = (title: string, items: string[]) =>
        items.length ? `${title}\n${items.map((item) => `- ${item}`).join("\n")}` : "";
    return [
        summary.title,
        list("关键要点", summary.key_points),
        list("决议", summary.decisions),
        list("待办事项", summary.todos),
    ]
        .filter(Boolean)
        .join("\n\n");
}

export function MeetingDetail({
    record,
    summarizing,
    error,
    onBack,
    onSummarize,
    onDelete,
}: {
    record: MeetingRecord;
    summarizing: boolean;
    error: string | null;
    onBack: () => void;
    onSummarize: () => void;
    onDelete: () => void;
}) {
    const copyAll = () => {
        const parts = [record.summary ? summaryToText(record.summary) : "", record.transcript];
        navigator.clipboard.writeText(parts.filter(Boolean).join("\n\n---\n\n"));
    };
    const hasTranscript = record.transcript.trim().length > 0;

    return (
        <div className="flex h-full min-h-0 flex-col">
            <div className="flex items-center justify-between border-b border-neutral-200 pb-3">
                <button
                    onClick={onBack}
                    className="inline-flex items-center gap-2 px-2 py-1 text-sm text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-neutral-900"
                >
                    <ArrowLeft className="h-4 w-4" /> 返回
                </button>
                <div className="flex items-center gap-1">
                    <button
                        onClick={copyAll}
                        disabled={!hasTranscript}
                        className="inline-flex items-center gap-2 px-2 py-1 text-sm text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-neutral-900 disabled:cursor-not-allowed disabled:opacity-40"
                    >
                        <Copy className="h-4 w-4" /> 复制
                    </button>
                    <button
                        onClick={onDelete}
                        className="inline-flex items-center gap-2 px-2 py-1 text-sm text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-red-600"
                    >
                        <Trash2 className="h-4 w-4" /> 删除
                    </button>
                    <button
                        onClick={onSummarize}
                        disabled={summarizing || !hasTranscript}
                        className="inline-flex items-center gap-2 bg-neutral-900 px-3 py-1.5 text-sm font-medium text-neutral-50 transition-opacity hover:opacity-70 disabled:cursor-not-allowed disabled:opacity-50"
                    >
                        {summarizing ? <Loader2 className="h-4 w-4 animate-spin" /> : <Sparkles className="h-4 w-4" />}
                        {summarizing ? "总结中" : record.summary ? "重新总结" : "生成总结"}
                    </button>
                </div>
            </div>

            <div className="custom-scrollbar min-h-0 flex-1 overflow-y-auto">
                <div className="py-4">
                    <div className="text-xl font-semibold text-neutral-900">{record.summary?.title || "未命名会议"}</div>
                    <div className="mt-1 text-xs text-neutral-400">
                        {new Date(record.started_at).toLocaleString()} · {formatDuration(record.duration_ms)} · {record.asr_provider}
                    </div>
                    {(error || record.last_error) && (
                        <div className="mt-3 border-l-2 border-red-500 bg-red-50 px-3 py-2 text-xs text-red-700">
                            {error || record.last_error}
                        </div>
                    )}
                    {record.status === "recording" && (
                        <div className="mt-3 border-l-2 border-amber-500 bg-amber-50 px-3 py-2 text-xs text-amber-800">
                            这场会议没有正常结束，以下是中断前保存的转写草稿。
                        </div>
                    )}
                </div>

                {record.summary && (
                    <div className="space-y-5 pb-6">
                        <SummarySection title="关键要点" items={record.summary.key_points} />
                        <SummarySection title="决议" items={record.summary.decisions} />
                        <SummarySection title="待办事项" items={record.summary.todos} />
                    </div>
                )}

                <div className="border-b border-neutral-200 pb-2 text-xs font-medium uppercase tracking-wider text-neutral-400">
                    转写全文
                </div>
                <div className="whitespace-pre-wrap py-4 text-sm leading-7 text-neutral-700">
                    {record.transcript || "（无转写内容）"}
                </div>
            </div>
        </div>
    );
}

function SummarySection({ title, items }: { title: string; items: string[] }) {
    if (items.length === 0) return null;
    return (
        <div>
            <div className="border-b border-neutral-200 pb-2 text-xs font-medium uppercase tracking-wider text-neutral-400">
                {title}
            </div>
            <ul className="mt-3 space-y-1.5">
                {items.map((item, i) => (
                    <li key={i} className="flex gap-2 text-sm leading-6 text-neutral-700">
                        <span className="mt-2.5 inline-block h-1 w-1 flex-shrink-0 rounded-full bg-chinese-indigo" />
                        <span>{item}</span>
                    </li>
                ))}
            </ul>
        </div>
    );
}
