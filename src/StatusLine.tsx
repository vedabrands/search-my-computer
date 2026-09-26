import React from "react";
import { invoke } from "@tauri-apps/api/core";
import { IndexStatus } from "./types";

interface StatusLineProps {
  status: IndexStatus | null;
  hotkeyWarning?: boolean;
  onOpenSettings?: () => void;
  onRefreshStatus?: () => Promise<void>;
}

export const StatusLine: React.FC<StatusLineProps> = ({
  status,
  hotkeyWarning,
  onOpenSettings,
  onRefreshStatus,
}) => {
  if (!status) {
    return <div className="status-line">Connecting...</div>;
  }

  const handlePauseWakeWord = async (mins: number) => {
    try {
      await invoke("pause_wake_word", { durationSecs: mins * 60 });
      if (onRefreshStatus) await onRefreshStatus();
    } catch (e) {
      console.error("Failed to pause wake word:", e);
    }
  };

  const handleResumeWakeWord = async () => {
    try {
      await invoke("resume_wake_word");
      if (onRefreshStatus) await onRefreshStatus();
    } catch (e) {
      console.error("Failed to resume wake word:", e);
    }
  };

  return (
    <div className="status-line ready">
      {hotkeyWarning ? (
        <span className="warning-text">
          ⚠️ Alt+Space conflict — check settings to change hotkey
        </span>
      ) : status.is_scanning ? (
        <span className="scanning-text">
          <span className="spinner">⏳</span> Indexing {status.scan_progress.files_seen.toLocaleString()} files...
        </span>
      ) : (
        <div className="status-meta-group">
          <span>
            {status.active_files.toLocaleString()} files · {status.indexed_folders.length} {status.indexed_folders.length === 1 ? "folder" : "folders"}
          </span>

          {/* Wake word status indicator */}
          {status.wake_word_enabled && (
            <div className="status-wake-word-badge">
              {status.is_wake_word_paused ? (
                <span className="wake-paused-tag" title="Wake word is paused. Click to resume.">
                  <span className="wake-dot paused" />
                  <span>Wake Word Paused</span>
                  <button
                    type="button"
                    className="wake-action-btn"
                    onClick={handleResumeWakeWord}
                    title="Resume wake word listening"
                  >
                    ▶ Resume
                  </button>
                </span>
              ) : (
                <span className="wake-armed-tag" title={`Listening for wake word "${status.wake_word_phrase || "Kira"}" (Offline local STT)`}>
                  <span className="wake-dot active" />
                  <span>"{status.wake_word_phrase || "Kira"}" listening</span>
                  <button
                    type="button"
                    className="wake-action-btn"
                    onClick={() => handlePauseWakeWord(60)}
                    title="Pause wake word for 1 hour"
                  >
                    ⏸ 1h
                  </button>
                </span>
              )}
            </div>
          )}
        </div>
      )}

      <div className="status-actions">
        <span className="shortcuts-hint">
          ↵ Open · Ctrl+↵ Reveal · Space Preview
        </span>
        {onOpenSettings && (
          <button
            className="settings-gear-btn"
            onClick={onOpenSettings}
            title="Settings & Indexing Preferences"
          >
            ⚙️
          </button>
        )}
      </div>
    </div>
  );
};

