import React from "react";
import { ParsedQuery } from "./types";
import { FilterChips } from "./FilterChips";

export interface VoiceInputStateProps {
  isListening: boolean;
  isTranscribing: boolean;
  isWakeWordArmed: boolean;
  audioLevel: number;
  durationSecs: number;
  silenceSecs: number;
  maxSecs: number;
  errorMessage: string | null;
  startListening: () => Promise<void>;
  stopListening: () => Promise<string | null>;
  resetVoice: () => Promise<void>;
}

interface SearchBarProps {
  query: string;
  parsedQuery?: ParsedQuery | null;
  onChange: (query: string) => void;
  onKeyDown: (e: React.KeyboardEvent<HTMLInputElement>) => void;
  inputRef: React.RefObject<HTMLInputElement | null>;
  voice?: VoiceInputStateProps;
}

export const SearchBar: React.FC<SearchBarProps> = ({
  query,
  parsedQuery,
  onChange,
  onKeyDown,
  inputRef,
  voice,
}) => {
  const isListening = voice?.isListening ?? false;
  const isTranscribing = voice?.isTranscribing ?? false;
  const errorMessage = voice?.errorMessage;
  const audioLevel = voice?.audioLevel ?? 0;
  const silenceSecs = voice?.silenceSecs ?? 0;
  const silenceLimit = 15;
  const silenceRemaining = Math.max(0, Math.ceil(silenceLimit - silenceSecs));

  return (
    <div className="search-bar-wrapper">
      <div className={`search-bar-container ${isListening ? "voice-active" : ""}`}>
        <span className="search-icon">
          {isListening ? "🎙️" : isTranscribing ? "⏳" : "🔍"}
        </span>

        <input
          ref={inputRef}
          type="text"
          className="search-input"
          placeholder={
            isListening
              ? "Listening... Speak naturally (click ⏹ Stop or wait for silence)"
              : isTranscribing
              ? "Transcribing your voice locally..."
              : "Search files, code, docs, or ask (e.g. 'rust project', 'pdf in downloads')..."
          }
          value={query}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={onKeyDown}
          autoFocus
          spellCheck={false}
          disabled={isTranscribing}
        />

        {/* Live Audio Level Meter during Active Listening */}
        {isListening && (
          <div className="voice-meter-pill" title={`Listening — Auto-stops after ${silenceRemaining}s of silence`}>
            <div className="voice-wave-bars">
              <span
                className="voice-bar bar-1"
                style={{ transform: `scaleY(${Math.max(0.2, audioLevel * 1.6)})` }}
              />
              <span
                className="voice-bar bar-2"
                style={{ transform: `scaleY(${Math.max(0.3, audioLevel * 2.2)})` }}
              />
              <span
                className="voice-bar bar-3"
                style={{ transform: `scaleY(${Math.max(0.2, audioLevel * 1.4)})` }}
              />
            </div>
            <span className="voice-countdown-text">{silenceRemaining}s</span>
            <button
              type="button"
              className="voice-stop-btn"
              onClick={() => voice?.stopListening()}
              title="Stop listening now (Enter)"
            >
              ⏹ Stop
            </button>
          </div>
        )}

        {/* Transcribing Indicator */}
        {isTranscribing && (
          <div className="voice-transcribing-badge">
            <span className="voice-pulse-dot" />
            <span>Transcribing...</span>
          </div>
        )}

        {/* Error Notification */}
        {errorMessage && !isListening && !isTranscribing && (
          <div className="voice-error-pill" title={errorMessage}>
            <span>⚠️ Mic Error</span>
            <button
              type="button"
              className="voice-error-dismiss"
              onClick={() => voice?.resetVoice()}
              title="Dismiss error"
            >
              ✕
            </button>
          </div>
        )}

        {/* Voice Trigger Button (when idle) */}
        {!isListening && !isTranscribing && voice && (
          <button
            type="button"
            className="voice-mic-btn"
            onClick={() => voice.startListening()}
            title="Start voice search (Offline Whisper)"
            aria-label="Start voice search"
          >
            🎙️
          </button>
        )}

        {/* Clear Search Query Button */}
        {query && !isListening && (
          <button
            type="button"
            className="clear-btn"
            onClick={() => onChange("")}
            title="Clear search"
          >
            ✕
          </button>
        )}
      </div>

      {parsedQuery && <FilterChips parsedQuery={parsedQuery} />}
    </div>
  );
};
