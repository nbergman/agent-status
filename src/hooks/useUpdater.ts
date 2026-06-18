import { useCallback, useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

type Phase = "idle" | "checking" | "available" | "downloading" | "ready" | "error";

interface UpdaterState {
  phase: Phase;
  version: string | null;
  error: string | null;
}

/**
 * Polls the configured update endpoint once on mount. When an update exists it
 * surfaces the new version; `install()` downloads + applies it and relaunches.
 *
 * In dev (no bundle / unsigned) `check()` throws — we swallow that to `idle`
 * so the banner simply never appears.
 */
export function useUpdater() {
  const [state, setState] = useState<UpdaterState>({
    phase: "idle",
    version: null,
    error: null,
  });
  const [update, setUpdate] = useState<Update | null>(null);

  useEffect(() => {
    let cancelled = false;
    setState((s) => ({ ...s, phase: "checking" }));
    check()
      .then((found) => {
        if (cancelled) return;
        if (found) {
          setUpdate(found);
          setState({ phase: "available", version: found.version, error: null });
        } else {
          setState({ phase: "idle", version: null, error: null });
        }
      })
      .catch(() => {
        // No updater in dev / offline — stay silent.
        if (!cancelled) setState({ phase: "idle", version: null, error: null });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const install = useCallback(async () => {
    if (!update) return;
    try {
      setState((s) => ({ ...s, phase: "downloading" }));
      await update.downloadAndInstall();
      setState((s) => ({ ...s, phase: "ready" }));
      await relaunch();
    } catch (e) {
      const error = e instanceof Error ? e.message : String(e);
      setState((s) => ({ ...s, phase: "error", error }));
    }
  }, [update]);

  return { ...state, install };
}
