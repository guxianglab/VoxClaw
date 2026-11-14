import { useCallback, useEffect, useRef, useState } from "react";
import { api, AppConfig } from "../../lib/api";

export type SaveState = "idle" | "saving" | "saved" | "error";

export function isSpeechReady(config: AppConfig): boolean {
  return config.asr.provider === "zipformer_streaming"
    ? !!config.asr.zipformer.model_dir
    : !!config.asr.volcengine.api_key?.trim();
}

export function isAiReady(config: AppConfig): boolean {
  const llm = config.llm_config;
  return !!llm.base_url.trim() && !!llm.api_key.trim() && !!llm.model.trim();
}

/**
 * Loads the config while `active` and auto-saves edits after a short debounce.
 * `saveError` is a hard failure (config not persisted); `saveNotice` is a
 * non-fatal hint such as "engine selected but model not downloaded yet".
 */
export function useConfigDraft(active: boolean) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveNotice, setSaveNotice] = useState<string | null>(null);
  const timerRef = useRef<number | null>(null);
  const pendingRef = useRef<AppConfig | null>(null);

  useEffect(() => {
    if (!active) return;
    api.getConfig().then(setConfig);
    setSaveError(null);
    setSaveNotice(null);
    setSaveState("idle");
  }, [active]);

  const persist = useCallback(async () => {
    const next = pendingRef.current;
    pendingRef.current = null;
    if (!next) return;
    try {
      const notice = await api.saveConfig(next);
      setSaveError(null);
      setSaveNotice(notice ?? null);
      setSaveState("saved");
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
      setSaveState("error");
    }
  }, []);

  const update = useCallback(
    (next: AppConfig) => {
      setConfig(next);
      pendingRef.current = next;
      setSaveState("saving");
      if (timerRef.current) window.clearTimeout(timerRef.current);
      timerRef.current = window.setTimeout(persist, 300);
    },
    [persist],
  );

  const flush = useCallback(async () => {
    if (timerRef.current) window.clearTimeout(timerRef.current);
    await persist();
  }, [persist]);

  useEffect(() => () => {
    if (timerRef.current) window.clearTimeout(timerRef.current);
  }, []);

  return { config, setConfig, update, flush, saveState, saveError, saveNotice };
}
