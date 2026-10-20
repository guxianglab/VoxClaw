import { useCallback, useEffect, useRef, useState } from "react";
import { Loader2, Square } from "lucide-react";
import {
    api,
    events,
    type MeetingActiveInfo,
    type MeetingRecord,
    type MeetingStatus,
    type MeetingSummaryItem,
} from "../lib/api";
import { MeetingDetail } from "./MeetingDetail";

export function formatDuration(ms: number): string {
    const totalSeconds = Math.max(0, Math.floor(ms / 1000));
    const h = Math.floor(totalSeconds / 3600);
    const m = Math.floor((totalSeconds % 3600) / 60);
    const s = totalSeconds % 60;
    const mmss = `${m.toString().padStart(h > 0 ? 2 : 1, "0")}:${s.toString().padStart(2, "0")}`;
    return h > 0 ? `${h}:${mmss}` : mmss;
}

const STATUS_LABEL: Record<MeetingStatus, string> = {
    recording: "未完成",
    transcribed: "已转写",
    summarized: "已总结",
    failed: "总结失败",
};

export function MeetingView() {
    const [active, setActive] = useState<MeetingActiveInfo | null>(null);
    const [liveText, setLiveText] = useState("");
    const [level, setLevel] = useState(0);
    const [finalizing, setFinalizing] = useState(false);
    const [starting, setStarting] = useState(false);
    const [history, setHistory] = useState<MeetingSummaryItem[]>([]);
    const [detail, setDetail] = useState<MeetingRecord | null>(null);
    const [summarizing, setSummarizing] = useState<Set<string>>(new Set());
    const [errorMsg, setErrorMsg] = useState<string | null>(null);
    const [, setTick] = useState(0);
    const liveRef = useRef<HTMLDivElement>(null);
    const detailIdRef = useRef<string | null>(null);
    detailIdRef.current = detail?.id ?? null;

    const refreshHistory = useCallback(() => {
        api.listMeetings().then(setHistory).catch((e) => console.error("listMeetings failed", e));
    }, []);

    const openDetail = useCallback(async (id: string) => {
        try {
            setDetail(await api.getMeeting(id));
        } catch (e) {
            setErrorMsg(String(e));
        }
    }, []);

    useEffect(() => {
        api.getActiveMeeting().then(setActive).catch(() => {});
        refreshHistory();

        const subs = [
            events.onMeetingStatus((p) => {
                setFinalizing(p.state === "finalizing");
                if (p.state === "idle") {
                    setActive(null);
                    setLiveText("");
                    setLevel(0);
                }
            }),
            events.onMeetingPartial((p) => setLiveText(p.text)),
            events.onMeetingAudioLevel((l) => setLevel(Math.min(1, l * 5))),
            events.onMeetingUpdated((p) => {
                setSummarizing((prev) => {
                    const next = new Set(prev);
                    if (p.summarizing) next.add(p.id);
                    else next.delete(p.id);
                    return next;
                });
                if (!p.summarizing) {
                    refreshHistory();
                    if (detailIdRef.current === p.id) openDetail(p.id);
                }
            }),
        ];
        return () => subs.forEach((s) => s.then((f) => f()));
    }, [refreshHistory, openDetail]);

    // Tick the timer once a second while recording.
    useEffect(() => {
        if (!active) return;
        const id = window.setInterval(() => setTick((t) => t + 1), 1000);
        return () => window.clearInterval(id);
    }, [active]);

    // Keep the live transcript scrolled to the newest text.
    useEffect(() => {
        liveRef.current?.scrollTo({ top: liveRef.current.scrollHeight });
    }, [liveText]);

    const handleStart = async () => {
        setErrorMsg(null);
        setStarting(true);
        try {
            setActive(await api.startMeeting());
            setLiveText("");
            refreshHistory();
        } catch (e) {
            setErrorMsg(String(e));
        } finally {
            setStarting(false);
        }
    };

    const handleStop = async () => {
        setErrorMsg(null);
        try {
            const record = await api.stopMeeting();
            refreshHistory();
            setDetail(record);
        } catch (e) {
            setErrorMsg(String(e));
        }
    };

    const handleSummarize = async (id: string) => {
        setErrorMsg(null);
        try {
            setDetail(await api.summarizeMeeting(id));
        } catch (e) {
            setErrorMsg(String(e));
            openDetail(id);
        }
    };

    const handleDelete = async (id: string) => {
        if (!window.confirm("删除该会议？此操作不可恢复。")) return;
        try {
            await api.deleteMeeting(id);
            setDetail(null);
            refreshHistory();
        } catch (e) {
            setErrorMsg(String(e));
        }
    };

    if (detail) {
        return (
            <MeetingDetail
                record={detail}
                summarizing={summarizing.has(detail.id)}
                error={errorMsg}
                onBack={() => {
                    setDetail(null);
                    setErrorMsg(null);
                }}
                onSummarize={() => handleSummarize(detail.id)}
                onDelete={() => handleDelete(detail.id)}
            />
        );
    }

    const elapsedMs = active ? Date.now() - Date.parse(active.started_at) : 0;
    const archived = history.filter((item) => item.id !== active?.session_id);

    return (
        <section className="flex h-full min-h-[280px] flex-col overflow-hidden">
            <div className="flex items-center justify-between border-b border-neutral-200 pb-3">
                <h2 className="text-xs font-medium uppercase tracking-wider text-neutral-400">会议记录</h2>
                {active ? (
                    <button
                        onClick={handleStop}
                        disabled={finalizing}
                        className="inline-flex items-center gap-2 bg-neutral-900 px-3 py-1.5 text-sm font-medium text-neutral-50 transition-opacity hover:opacity-70 disabled:cursor-not-allowed disabled:opacity-50"
                    >
                        {finalizing ? <Loader2 className="h-4 w-4 animate-spin" /> : <Square className="h-4 w-4" />}
                        {finalizing ? "整理中" : "结束会议"}
                    </button>
                ) : (
                    <button
                        onClick={handleStart}
                        disabled={starting}
                        className="bg-neutral-900 px-3 py-1.5 text-sm font-medium text-neutral-50 transition-opacity hover:opacity-70 disabled:cursor-not-allowed disabled:opacity-50"
                    >
                        {starting ? "启动中…" : "开始会议"}
                    </button>
                )}
            </div>

            {errorMsg && (
                <div className="mt-4 border-l-2 border-red-500 bg-red-50 px-3 py-2 text-xs text-red-700">{errorMsg}</div>
            )}

            {active && (
                <div className="flex min-h-0 flex-1 flex-col border-b border-neutral-200 py-4">
                    <div className="flex items-center gap-3 text-xs text-neutral-400">
                        <span className="h-2 w-2 rounded-full bg-red-500" />
                        <span className="text-neutral-900">{formatDuration(elapsedMs)}</span>
                        <span>{active.asr_provider} · 麦克风 + 系统声音</span>
                        <div className="ml-auto h-1 w-24 overflow-hidden rounded-full bg-neutral-200">
                            <div className="h-full rounded-full bg-chinese-indigo" style={{ width: `${level * 100}%` }} />
                        </div>
                    </div>
                    <div
                        ref={liveRef}
                        className="custom-scrollbar mt-3 min-h-0 flex-1 overflow-y-auto whitespace-pre-wrap text-sm leading-7 text-neutral-700"
                    >
                        {liveText || <span className="text-neutral-400">正在聆听…</span>}
                    </div>
                </div>
            )}

            {!active && (
                <div className="custom-scrollbar min-h-0 flex-1 overflow-y-auto">
                    {archived.length === 0 ? (
                        <div className="flex h-full min-h-[200px] flex-col items-center justify-center text-center">
                            <div className="text-sm text-neutral-400">暂无会议</div>
                            <div className="mt-1 text-xs text-neutral-300">开始会议后会实时转写，结束时自动生成总结</div>
                        </div>
                    ) : (
                        <div className="divide-y divide-neutral-200">
                            {archived.map((item) => (
                                <button
                                    key={item.id}
                                    onClick={() => openDetail(item.id)}
                                    className="flex w-full items-center gap-4 px-1 py-4 text-left transition-colors hover:bg-neutral-100"
                                >
                                    <span className="min-w-0 flex-1">
                                        <span className="block truncate text-sm text-neutral-900">
                                            {item.title === item.started_at ? "未命名会议" : item.title}
                                        </span>
                                        <span className="mt-1 block text-xs text-neutral-400">
                                            {new Date(item.started_at).toLocaleString()} · {formatDuration(item.duration_ms)}
                                        </span>
                                    </span>
                                    <span className="flex flex-shrink-0 items-center gap-1 text-xs text-neutral-400">
                                        {summarizing.has(item.id) && <Loader2 className="h-3 w-3 animate-spin" />}
                                        {summarizing.has(item.id) ? "总结中" : STATUS_LABEL[item.status]}
                                    </span>
                                </button>
                            ))}
                        </div>
                    )}
                </div>
            )}
        </section>
    );
}
