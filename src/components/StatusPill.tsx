import React from "react";
import { StatusPillVisualState } from "../types";
import { UseVoiceInputReturn } from "../hooks/useVoiceInput";

export interface StatusPillProps {
  visualState: StatusPillVisualState;
  statusLabel: string;
  onExpand: () => void;
  voice?: UseVoiceInputReturn;
  hotkey?: string;
  onTogglePause?: () => void;
  onOpenSettings?: () => void;
}

export const StatusPill: React.FC<StatusPillProps> = ({
  visualState,
  statusLabel,
  onExpand,
  voice,
  hotkey = "Alt+Space",
  onTogglePause,
  onOpenSettings,
}) => {
  const isListening = visualState === "listening";

  const handlePillClick = (e: React.MouseEvent) => {
    // Prevent expanding if clicking specific action buttons
    if ((e.target as HTMLElement).closest(".pill-action-btn")) {
      return;
    }
    onExpand();
  };

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    if (onOpenSettings) {
      onOpenSettings();
    }
  };

  const handleMicClick = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (!voice) return;
    if (voice.isListening) {
      await voice.stopListening();
    } else {
      await voice.startListening();
    }
  };

  return (
    <div
      className={`status-pill-shell state-${visualState}`}
      onClick={handlePillClick}
      onContextMenu={handleContextMenu}
      data-tauri-drag-region
      role="button"
      tabIndex={0}
      aria-label={`SearchMyComputer Status: ${statusLabel}. Click to search.`}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onExpand();
        } else if (e.key === "p" && (e.ctrlKey || e.altKey)) {
          e.preventDefault();
          onTogglePause?.();
        } else if (e.key === "," && (e.ctrlKey || e.altKey)) {
          e.preventDefault();
          onOpenSettings?.();
        }
      }}
    >
      {/* Drag handle / App Brand Dot */}
      <div className="status-pill-indicator-wrap" data-tauri-drag-region>
        <div className={`status-pill-dot dot-${visualState}`} />
      </div>

      {/* State Label / Audio Visualizer */}
      <div className="status-pill-content" data-tauri-drag-region>
        {isListening && voice ? (
          <div className="status-pill-voice-viz">
            <span className="voice-viz-bar bar-1" style={{ transform: `scaleY(${Math.max(0.2, voice.audioLevel * 2)})` }} />
            <span className="voice-viz-bar bar-2" style={{ transform: `scaleY(${Math.max(0.3, voice.audioLevel * 3)})` }} />
            <span className="voice-viz-bar bar-3" style={{ transform: `scaleY(${Math.max(0.2, voice.audioLevel * 1.8)})` }} />
            <span className="status-pill-text voice-text">Listening...</span>
          </div>
        ) : (
          <span className="status-pill-text" title={statusLabel}>
            {statusLabel}
          </span>
        )}
      </div>

      {/* Action triggers: Mic & Hotkey hint */}
      <div className="status-pill-actions">
        {voice && (
          <button
            type="button"
            className={`pill-action-btn pill-mic-btn ${voice.isListening ? "active" : ""}`}
            onClick={handleMicClick}
            title={voice.isListening ? "Stop voice listening" : "Click to speak"}
            aria-label={voice.isListening ? "Stop voice listening" : "Click to speak"}
          >
            🎙️
          </button>
        )}
        <kbd className="status-pill-hotkey" title={`Press ${hotkey} to search`}>
          {hotkey}
        </kbd>
      </div>
    </div>
  );
};
