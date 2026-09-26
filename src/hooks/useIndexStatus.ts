import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { IndexStatus, ScanSnapshot } from "../types";

export function useIndexStatus() {
  const [status, setStatus] = useState<IndexStatus | null>(null);

  const refreshStatus = async () => {
    try {
      const s = await invoke<IndexStatus>("get_index_status");
      setStatus(s);
    } catch {
      // Ignore errors when backend is starting up.
    }
  };

  useEffect(() => {
    refreshStatus();

    // Listen to scan progress events streamed from Rust.
    const unlistenProgress = listen<ScanSnapshot>("scan-progress", (event) => {
      setStatus((prev) => {
        if (!prev) return prev;
        return {
          ...prev,
          is_scanning: true,
          scan_progress: event.payload,
        };
      });
    });

    const unlistenComplete = listen<ScanSnapshot>("scan-complete", () => {
      refreshStatus();
    });

    const unlistenAllComplete = listen("scan-all-complete", () => {
      refreshStatus();
    });

    // Periodic poll as a fallback (every 3 seconds).
    const interval = setInterval(refreshStatus, 3000);

    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenComplete.then((fn) => fn());
      unlistenAllComplete.then((fn) => fn());
      clearInterval(interval);
    };
  }, []);

  return { status, refreshStatus };
}
