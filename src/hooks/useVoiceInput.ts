import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AudioCaptureState } from "../types";

export interface UseVoiceInputProps {
  onTranscription: (text: string) => void;
}

export interface UseVoiceInputReturn {
  voiceState: AudioCaptureState;
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

export function useVoiceInput({ onTranscription }: UseVoiceInputProps): UseVoiceInputReturn {
  const [voiceState, setVoiceState] = useState<AudioCaptureState>({ state: "Idle" });
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const transcriptionCbRef = useRef(onTranscription);
  transcriptionCbRef.current = onTranscription;

  // Poll state when listening or transcribing
  useEffect(() => {
    let timer: ReturnType<typeof setInterval> | null = null;

    const poll = async () => {
      try {
        const state = await invoke<AudioCaptureState>("get_voice_state");
        setVoiceState(state);
      } catch (e) {
        console.error("error polling voice state:", e);
      }
    };

    if (voiceState.state === "Listening" || voiceState.state === "Transcribing") {
      timer = setInterval(poll, 80);
    } else {
      timer = setInterval(poll, 1000);
    }

    return () => {
      if (timer) clearInterval(timer);
    };
  }, [voiceState.state]);

  // Setup Tauri event listeners for background wake word and transcription
  useEffect(() => {
    let unlistenWakeWord: (() => void) | null = null;
    let unlistenTranscribe: (() => void) | null = null;

    listen("voice:wake_word", () => {
      setErrorMessage(null);
      setVoiceState({
        state: "Listening",
        data: {
          duration_secs: 0,
          silence_secs: 0,
          max_secs: 45,
          level: 0,
        },
      });
    }).then((unsub) => {
      unlistenWakeWord = unsub;
    });

    listen<string>("voice:transcription", (event) => {
      const text = event.payload?.trim();
      if (text) {
        transcriptionCbRef.current(text);
      }
      setVoiceState({ state: "Idle" });
    }).then((unsub) => {
      unlistenTranscribe = unsub;
    });

    return () => {
      if (unlistenWakeWord) unlistenWakeWord();
      if (unlistenTranscribe) unlistenTranscribe();
    };
  }, []);

  const startListening = useCallback(async () => {
    setErrorMessage(null);
    try {
      await invoke("start_voice_capture");
      setVoiceState({
        state: "Listening",
        data: {
          duration_secs: 0,
          silence_secs: 0,
          max_secs: 45,
          level: 0,
        },
      });
    } catch (err: unknown) {
      const msg = typeof err === "string" ? err : String(err);
      setErrorMessage(msg);
      setVoiceState({ state: "Error", data: { message: msg } });
    }
  }, []);

  const stopListening = useCallback(async () => {
    try {
      setVoiceState({ state: "Transcribing" });
      const transcription = await invoke<string>("stop_voice_capture");
      const trimmed = transcription.trim();
      if (trimmed) {
        transcriptionCbRef.current(trimmed);
      }
      setVoiceState({ state: "Idle" });
      return trimmed;
    } catch (err: unknown) {
      const msg = typeof err === "string" ? err : String(err);
      setErrorMessage(msg);
      setVoiceState({ state: "Error", data: { message: msg } });
      return null;
    }
  }, []);

  const resetVoice = useCallback(async () => {
    setErrorMessage(null);
    try {
      await invoke("reset_voice_state");
      setVoiceState({ state: "Idle" });
    } catch (e) {
      console.error("error resetting voice state:", e);
    }
  }, []);

  const isListening = voiceState.state === "Listening";
  const isTranscribing = voiceState.state === "Transcribing";
  const isWakeWordArmed = voiceState.state === "WakeWordArmed";

  const audioLevel = isListening && "data" in voiceState ? voiceState.data.level : 0;
  const durationSecs = isListening && "data" in voiceState ? voiceState.data.duration_secs : 0;
  const silenceSecs = isListening && "data" in voiceState ? voiceState.data.silence_secs : 0;
  const maxSecs = isListening && "data" in voiceState ? voiceState.data.max_secs : 45;

  return {
    voiceState,
    isListening,
    isTranscribing,
    isWakeWordArmed,
    audioLevel,
    durationSecs,
    silenceSecs,
    maxSecs,
    errorMessage,
    startListening,
    stopListening,
    resetVoice,
  };
}
