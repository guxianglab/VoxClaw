import { useEffect, useMemo, useState } from "react";
import { ArrowLeft, Copy, Loader2, Sparkles, Trash2 } from "lucide-react";
import type { MeetingRecord } from "../lib/api";

function formatDuration(ms: number): string {
    const totalSeconds = Math.max(0, Math.floor(ms / 1000));
    const h = Math.floor(totalSeconds / 3600);
    const m = Math.floor((totalSeconds % 3600) / 60);
    const s = totalSeconds % 60;
    if (h > 0) {
        return `${h}:${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
    }
    return `${m}:${s.toString().padStart(2, "0")}`;
}

type Tab = "summary" | "corrected" | "raw";

export function MeetingDetail({
    record,
    isActive,
    liveRawText,
    isPolishing,
    polishStage,
    onBack,
    onPolish,
    onDelete,
}: {
    record: MeetingRecord;
    isActive: boolean;
    liveRawText?: string;
    isPolishing: boolean;
    polishStage: string | null;
    onBack: () => void;
    onPolish: () => void;
    onDelete: () => void;
}) {
    const hasSummary = !!record.summary;
    const hasCorrected = !!record.corrected_text;
    const [tab, setTab] = useState<Tab>(hasSummary ? "summary" : hasCorrected ? "corrected" : "raw");
    const effectiveRawText = isActive && liveRawText ? liveRawText : record.raw_text;

    useEffect(() => {
        setTab(hasSummary ? "summary" : hasCorrected ? "corrected" : "raw");
    }, [record.id, hasSummary, hasCorrected]);

    const currentText = useMemo(() => {
        if (tab === "corrected") {
            return record.corrected_text ?? "";
        }
        if (tab === "raw") {
            return effectiveRawText;
        }
        if (!record.summary) {
            return "";
        }
        return [
            record.summary.title,
            record.summary.key_points.length ? `关键要点\n${record.summary.key_points.map((item) => `- ${item}`).join("\n")}` : "",
            record.summary.decisions.length ? `决议\n${record.summary.decisions.map((item) => `- ${item}`).join("\n")}` : "",
            record.summary.todos.length ? `待办事项\n${record.summary.todos.map((item) => `- ${item}`).join("\n")}` : "",
        ]
            .filter(Boolean)
            .join("\n\n");
    }, [effectiveRawText, record.corrected_text, record.summary, tab]);

    const tabs: Array<{ key: Tab; label: string; available: boolean }> = [
        { key: "summary", label: "总结", available: hasSummary },
        { key: "corrected", label: "润色稿", available: hasCorrected },
        { key: "raw", label: "原始转写", available: true },
    ];

    const canPolish = !isActive && record.status !== "finalizing";
    const staleDraft = !isActive && record.status === "recording";

    const copyCurrentText = () => {
        if (!currentText.trim()) return;
        navigator.clipboard.writeText(currentText);
    };

    return (
        <div className="flex h-full min-h-0 flex-col">
            <div className="flex items-center justify-between border-b border-neutral-200 pb-3">
                <button
                    onClick={onBack}
                    className="inline-flex items-center gap-2 px-2 py-1 text-sm text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-neutral-900"
                >
                    <ArrowLeft className="h-4 w-4" /> 返回列表
                </button>
                <div className="flex items-center gap-1">
                    <button
                        onClick={copyCurrentText}
                        disabled={!currentText.trim()}
                        className="px-2 py-1 text-sm text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-neutral-900 disabled:cursor-not-allowed disabled:opacity-40"
                    >
                        <span className="inline-flex items-center gap-2">
                            <Copy className="h-4 w-4" /> 复制
                        </span>
                    </button>
                    {canPolish && (
                        <button
                            onClick={onPolish}
                            disabled={isPolishing}
                            className="bg-neutral-900 px-3 py-1.5 text-sm font-medium text-neutral-50 transition-opacity hover:opacity-70 disabled:cursor-not-allowed disabled:opacity-50"
                        >
                            <span className="inline-flex items-center gap-2">
                                {isPolishing ? <Loader2 className="h-4 w-4 animate-spin" /> : <Sparkles className="h-4 w-4" />}
                                {isPolishing ? polishStageLabel(polishStage) : "润色并总结"}
                            </span>
                        </button>
                    )}
                    {!isActive && (
                        <button
                            onClick={onDelete}
                            className="px-2 py-1 text-sm text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-red-600"
                        >
                            <span className="inline-flex items-center gap-2">
                                <Trash2 className="h-4 w-4" /> 删除
                            </span>
                        </button>
                    )}
                </div>
            </div>

            <div className="border-b border-neutral-200 py-4">
                <div className="text-xs uppercase tracking-wider text-neutral-400">
                    会议详情
                </div>
                <div className="mt-1 text-lg font-semibold text-neutral-900">
                    {record.summary?.title || "未命名会议"}
                </div>
                <div className="mt-1 text-xs text-neutral-400">
                    {new Date(record.started_at).toLocaleString()} · {formatDuration(record.duration_ms)} · {record.asr_provider}
                </div>
                {record.last_error && (
                    <div className="mt-2 border-l-2 border-red-500 bg-red-50 px-3 py-2 text-xs text-red-700">
                        {record.last_error}
                    </div>
                )}
                {isActive && (
                    <div className="mt-2 border-l-2 border-chinese-indigo bg-neutral-50 px-3 py-2 text-xs text-neutral-600">
                        当前会议仍在录制中，原始转写会随着识别结果持续更新。
                    </div>
                )}
                {staleDraft && (
                    <div className="mt-2 border-l-2 border-amber-500 bg-amber-50 px-3 py-2 text-xs text-amber-800">
                        这是一次未正常结束的会议草稿。当前原始转写和会议音频草稿都已保留，建议先检查原始内容再决定是否继续整理。
                    </div>
                )}
            </div>

            <div className="mt-3 flex gap-4 border-b border-neutral-200">
                {tabs.map((t) => (
                    <button
                        key={t.key}
                        disabled={!t.available}
                        onClick={() => setTab(t.key)}
                        className={`-mb-px border-b border-transparent px-1 pb-2 text-sm transition-colors ${tab === t.key
                            ? "border-chinese-indigo text-neutral-900"
                            : t.available
                                ? "text-neutral-500 hover:text-neutral-900"
                                : "text-neutral-300"
                            }`}
                    >
                        {t.label}
                    </button>
                ))}
            </div>

            <div className="min-h-0 flex-1 overflow-y-auto pt-4">
                {tab === "summary" && record.summary && (
                    <SummaryPanel summary={record.summary} />
                )}
                {tab === "corrected" && record.corrected_text && (
                    <pre className="whitespace-pre-wrap text-sm leading-7 text-neutral-700">
                        {record.corrected_text}
                    </pre>
                )}
                {tab === "raw" && (
                    <pre className="whitespace-pre-wrap text-sm leading-7 text-neutral-700">
                        {effectiveRawText || "（无转写内容）"}
                    </pre>
                )}
            </div>
        </div>
    );
}

function polishStageLabel(stage: string | null): string {
    switch (stage) {
        case "correcting":
            return "润色中";
        case "correction_done":
            return "准备总结";
        case "summarising":
            return "总结中";
        default:
            return "处理中";
    }
}

function SummaryPanel({ summary }: { summary: NonNullable<MeetingRecord["summary"]> }) {
    return (
        <div className="space-y-6">
            <SummarySection title="关键要点" items={summary.key_points} />
            <SummarySection title="决议" items={summary.decisions} />
            <SummarySection title="待办事项" items={summary.todos} />
        </div>
    );
}

function SummarySection({ title, items }: { title: string; items: string[] }) {
    return (
        <div>
            <div className="text-xs uppercase tracking-wider text-neutral-400">{title}</div>
            {items.length === 0 ? (
                <div className="mt-2 text-sm text-neutral-400">—</div>
            ) : (
                <ul className="mt-2 space-y-1.5">
                    {items.map((it, i) => (
                        <li key={i} className="flex gap-2 text-sm leading-6 text-neutral-700">
                            <span className="mt-2 inline-block h-1 w-1 flex-shrink-0 rounded-full bg-chinese-indigo" />
                            <span>{it}</span>
                        </li>
                    ))}
                </ul>
            )}
        </div>
    );
}
