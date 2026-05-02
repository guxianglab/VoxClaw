import { useCallback, useEffect, useMemo, useState } from "react";
import { Loader2, Mic, Square } from "lucide-react";
import {
    api,
    events,
    type MeetingActiveInfo,
    type MeetingRecord,
    type MeetingStatus,
    type MeetingSummaryItem,
} from "../lib/api";
import { AudioVisualizer } from "./AudioVisualizer";
import { MeetingDetail } from "./MeetingDetail";

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

function formatStartedAt(iso: string): string {
    try {
        const d = new Date(iso);
        return d.toLocaleString();
    } catch {
        return iso;
    }
}

function statusLabel(status: MeetingStatus, isActive: boolean): string {
    switch (status) {
        case "recording":
            return isActive ? "录制中" : "未完成草稿";
        case "finalizing":
            return "整理中";
        case "raw_only":
            return "原始转写";
        case "corrected":
            return "已润色";
        case "summarized":
            return "已总结";
        case "failed":
            return "失败";
    }
}

function statusBadgeClass(status: MeetingStatus): string {
    switch (status) {
        case "summarized":
        case "corrected":
            return "bg-emerald-50 text-emerald-700";
        case "recording":
        case "finalizing":
            return "bg-chinese-indigo/10 text-chinese-indigo";
        case "failed":
            return "bg-red-50 text-red-700";
        default:
            return "bg-neutral-100 text-neutral-500";
    }
}

function buildActiveRecord(active: MeetingActiveInfo, partialText: string): MeetingRecord {
    return {
        id: active.session_id,
        started_at: active.started_at,
        ended_at: null,
        duration_ms: 0,
        audio_source: active.include_system_audio ? "mic_and_loopback" : "mic_only",
        asr_provider: active.asr_provider,
        status: "recording",
        segments: partialText.trim()
            ? [{ start_ms: 0, end_ms: 0, text: partialText, speaker: null }]
            : [],
        raw_text: partialText,
        corrected_text: null,
        summary: null,
        last_error: null,
        draft_audio_path: null,
    };
}

export function MeetingView() {
    const [active, setActive] = useState<MeetingActiveInfo | null>(null);
    const [partialText, setPartialText] = useState("");
    const [history, setHistory] = useState<MeetingSummaryItem[]>([]);
    const [selectedId, setSelectedId] = useState<string | null>(null);
    const [detail, setDetail] = useState<MeetingRecord | null>(null);
    const [busyAction, setBusyAction] = useState<"start" | "stop" | null>(null);
    const [polishingId, setPolishingId] = useState<string | null>(null);
    const [polishStage, setPolishStage] = useState<string | null>(null);
    const [errorMsg, setErrorMsg] = useState<string | null>(null);
    const [, setNow] = useState(0);
    const startedAtMs = useMemo(() => (active ? Date.parse(active.started_at) : 0), [active]);
    const selectedRecord = useMemo(() => {
        if (detail && detail.id === selectedId) return detail;
        if (selectedId && active && selectedId === active.session_id) {
            return buildActiveRecord(active, partialText);
        }
        return null;
    }, [active, detail, partialText, selectedId]);
    const archivedHistory = useMemo(
        () => history.filter((item) => item.id !== active?.session_id),
        [active?.session_id, history],
    );

    const refreshHistory = useCallback(async () => {
        try {
            const list = await api.listMeetings();
            setHistory(list);
        } catch (e) {
            console.error("listMeetings failed", e);
        }
    }, []);

    const refreshDetail = useCallback(async (id: string | null) => {
        if (!id) {
            setDetail(null);
            return;
        }
        try {
            const r = await api.getMeeting(id);
            setDetail(r);
        } catch (e) {
            console.error("getMeeting failed", e);
        }
    }, []);

    useEffect(() => {
        api.getActiveMeeting().then(setActive).catch(() => {});
        refreshHistory();
    }, [refreshHistory]);

    useEffect(() => {
        refreshDetail(selectedId);
    }, [selectedId, refreshDetail]);

    // Tick the timer once a second while recording.
    useEffect(() => {
        if (!active) return;
        const id = window.setInterval(() => setNow(Date.now()), 1000);
        return () => window.clearInterval(id);
    }, [active]);

    useEffect(() => {
        const subs: Array<Promise<() => void>> = [
            events.onMeetingStatus((p) => {
                if (p.state === "idle") {
                    setActive(null);
                    setPartialText("");
                } else if (p.state === "recording" && p.session_id) {
                    setActive((prev) => prev ?? {
                        session_id: p.session_id!,
                        started_at: new Date().toISOString(),
                        asr_provider: "",
                        include_system_audio: false,
                    });
                }
            }),
            events.onMeetingPartial((p) => setPartialText(p.text)),
            events.onMeetingFinalized(() => {
                refreshHistory();
                if (selectedId) refreshDetail(selectedId);
            }),
            events.onMeetingLlmProgress((p) => {
                setPolishStage(p.stage);
                if (p.stage === "done" || p.stage === "failed") {
                    setPolishingId(null);
                    refreshHistory();
                    if (selectedId === p.id) refreshDetail(p.id);
                    if (p.stage === "failed" && p.error) setErrorMsg(p.error);
                    if (p.stage === "done") setPolishStage(null);
                }
            }),
        ];
        return () => {
            subs.forEach((s) => s.then((f) => f()));
        };
    }, [refreshHistory, refreshDetail, selectedId]);

    const handleStart = async () => {
        setErrorMsg(null);
        setBusyAction("start");
        try {
            const info = await api.startMeeting(false);
            setActive(info);
            setPartialText("");
            await refreshHistory();
        } catch (e) {
            setErrorMsg(String(e));
        } finally {
            setBusyAction(null);
        }
    };

    const handleStop = async () => {
        setErrorMsg(null);
        setBusyAction("stop");
        try {
            const record = await api.stopMeeting();
            setActive(null);
            setPartialText("");
            await refreshHistory();
            setSelectedId(record.id);
        } catch (e) {
            setErrorMsg(String(e));
        } finally {
            setBusyAction(null);
        }
    };

    const handlePolish = async (id: string) => {
        setErrorMsg(null);
        setPolishingId(id);
        setPolishStage("correcting");
        try {
            await api.polishMeeting(id);
            await refreshHistory();
            await refreshDetail(id);
        } catch (e) {
            setErrorMsg(String(e));
            setPolishingId(null);
            setPolishStage(null);
        }
    };

    const handleDelete = async (id: string) => {
        if (!window.confirm("删除该会议？此操作不可恢复。")) return;
        try {
            await api.deleteMeeting(id);
            if (selectedId === id) setSelectedId(null);
            await refreshHistory();
        } catch (e) {
            setErrorMsg(String(e));
        }
    };

    const elapsedMs = active && startedAtMs ? Date.now() - startedAtMs : 0;

    if (selectedId) {
        return selectedRecord ? (
            <MeetingDetail
                record={selectedRecord}
                isActive={active?.session_id === selectedRecord.id}
                liveRawText={active?.session_id === selectedRecord.id ? partialText : undefined}
                isPolishing={polishingId === selectedRecord.id}
                polishStage={polishStage}
                onBack={() => setSelectedId(null)}
                onPolish={() => handlePolish(selectedRecord.id)}
                onDelete={() => handleDelete(selectedRecord.id)}
            />
        ) : (
            <div className="flex h-full min-h-[240px] flex-col items-center justify-center gap-4 text-sm text-neutral-400">
                <div>正在加载会议详情…</div>
                <button
                    onClick={() => setSelectedId(null)}
                    className="px-2 py-1 text-sm text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-neutral-900"
                >
                    返回列表
                </button>
            </div>
        );
    }

    return (
        <section className="flex h-full min-h-[280px] flex-col overflow-hidden">
            <div className="flex items-center justify-between pb-3">
                <h2 className="text-xs font-medium uppercase tracking-wider text-neutral-400">会议记录</h2>
                <div className="flex items-center gap-2">
                    <span className="text-xs text-neutral-300">{history.length} 场</span>
                    {active ? (
                        <button
                            disabled
                            className="px-2 py-1 text-sm text-neutral-300"
                        >
                            会议进行中
                        </button>
                    ) : (
                        <button
                            onClick={handleStart}
                            disabled={busyAction !== null}
                            className="bg-neutral-900 px-3 py-1.5 text-sm font-medium text-neutral-50 transition-opacity hover:opacity-70 disabled:cursor-not-allowed disabled:opacity-50"
                        >
                            {busyAction === "start" ? "启动中…" : "新建会议"}
                        </button>
                    )}
                </div>
            </div>

            <div className="border-b border-neutral-200" />

            <div className="mt-4 flex-1 overflow-y-auto">
                {errorMsg && (
                    <div className="mb-4 border-l-2 border-red-500 bg-red-50 px-3 py-2 text-xs text-red-700">
                        {errorMsg}
                    </div>
                )}

                {active && (
                    <article className="border-b border-neutral-200 py-4">
                        <div className="flex items-start justify-between gap-4">
                            <button
                                onClick={() => setSelectedId(active.session_id)}
                                className="flex min-w-0 flex-1 items-start gap-4 text-left"
                            >
                                <AudioVisualizer
                                    isRecording
                                    eventName="meeting_audio_level"
                                    className="h-16 w-28 flex-shrink-0"
                                />
                                <div className="min-w-0 flex-1">
                                    <div className="flex items-center gap-2">
                                        <span className={`inline-flex items-center px-2 py-0.5 text-xs ${statusBadgeClass("recording")}`}>
                                            {statusLabel("recording", true)}
                                        </span>
                                        <span className="text-xs text-neutral-400">{formatDuration(elapsedMs)}</span>
                                    </div>
                                    <div className="mt-2 flex items-center gap-2 text-sm text-neutral-900">
                                        <Mic className="h-4 w-4 text-neutral-400" />
                                        <span className="truncate">当前会议</span>
                                    </div>
                                    <div className="mt-1 text-xs text-neutral-400">
                                        {active.asr_provider} · 起始 {formatStartedAt(active.started_at)}
                                    </div>
                                    <p className="mt-2 line-clamp-3 whitespace-pre-wrap text-sm leading-6 text-neutral-600">
                                        {partialText || "录音已开始，等待识别结果…"}
                                    </p>
                                </div>
                            </button>

                            <button
                                onClick={handleStop}
                                disabled={busyAction !== null}
                                className="bg-neutral-900 px-3 py-1.5 text-sm font-medium text-neutral-50 transition-opacity hover:opacity-70 disabled:cursor-not-allowed disabled:opacity-50"
                            >
                                <span className="inline-flex items-center gap-2">
                                    {busyAction === "stop" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Square className="h-4 w-4" />}
                                    {busyAction === "stop" ? "整理中" : "结束会议"}
                                </span>
                            </button>
                        </div>
                    </article>
                )}

                {archivedHistory.length === 0 ? (
                    <div className="flex h-full min-h-[200px] flex-col items-center justify-center text-center">
                        <div className="text-sm text-neutral-400">暂无历史会议</div>
                        <div className="mt-1 text-xs text-neutral-300">新建会议后，原始转写、润色稿和总结会显示在这里</div>
                    </div>
                ) : (
                    <div className="-mt-px divide-y divide-neutral-200">
                        {archivedHistory.map((item) => {
                            const isItemPolishing = polishingId === item.id;
                            return (
                                <article
                                    key={item.id}
                                    className="group py-4 transition-colors hover:bg-neutral-100"
                                >
                                    <button
                                        onClick={() => setSelectedId(item.id)}
                                        className="flex w-full items-start gap-4 px-1 text-left"
                                    >
                                        <span className={`mt-1 inline-flex flex-shrink-0 items-center px-2 py-0.5 text-xs ${statusBadgeClass(item.status)}`}>
                                            {isItemPolishing ? <Loader2 className="mr-1 h-3 w-3 animate-spin" /> : null}
                                            {isItemPolishing ? polishStage ?? "处理中" : statusLabel(item.status, false)}
                                        </span>
                                        <span className="min-w-0 flex-1">
                                            <span className="block truncate text-sm text-neutral-900">
                                                {item.status === "recording" ? "未完成会议草稿" : item.title || "未命名会议"}
                                            </span>
                                            <span className="mt-1 block text-xs text-neutral-400">
                                                {formatStartedAt(item.started_at)} · {formatDuration(item.duration_ms)}
                                            </span>
                                        </span>
                                    </button>
                                </article>
                            );
                        })}
                    </div>
                )}
            </div>
        </section>
    );
}
