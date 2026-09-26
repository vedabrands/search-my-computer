import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { StatusPill } from "../components/StatusPill";
import { UseVoiceInputReturn } from "../hooks/useVoiceInput";

describe("StatusPill Component", () => {
  const defaultProps = {
    visualState: "idle" as const,
    statusLabel: "1,250 files",
    onExpand: vi.fn(),
    hotkey: "Alt+Space",
  };

  it("renders idle state with label and hotkey badge", () => {
    render(<StatusPill {...defaultProps} />);

    expect(screen.getByText("1,250 files")).toBeInTheDocument();
    expect(screen.getByText("Alt+Space")).toBeInTheDocument();
    const pill = screen.getByRole("button", { name: /SearchMyComputer Status: 1,250 files/i });
    expect(pill).toBeInTheDocument();
    expect(pill).toHaveClass("state-idle");
  });

  it("calls onExpand when clicked", () => {
    const onExpand = vi.fn();
    render(<StatusPill {...defaultProps} onExpand={onExpand} />);

    const pill = screen.getByRole("button", { name: /SearchMyComputer Status:/i });
    fireEvent.click(pill);
    expect(onExpand).toHaveBeenCalledTimes(1);
  });

  it("expands on Enter or Space key press", () => {
    const onExpand = vi.fn();
    render(<StatusPill {...defaultProps} onExpand={onExpand} />);

    const pill = screen.getByRole("button", { name: /SearchMyComputer Status:/i });
    fireEvent.keyDown(pill, { key: "Enter" });
    expect(onExpand).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(pill, { key: " " });
    expect(onExpand).toHaveBeenCalledTimes(2);
  });

  it("renders different visual states correctly", () => {
    const { rerender } = render(
      <StatusPill {...defaultProps} visualState="searching" statusLabel="Searching..." />
    );
    expect(screen.getByText("Searching...")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /SearchMyComputer Status:/i })).toHaveClass("state-searching");

    rerender(<StatusPill {...defaultProps} visualState="results" statusLabel="12 results" />);
    expect(screen.getByText("12 results")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /SearchMyComputer Status:/i })).toHaveClass("state-results");

    rerender(<StatusPill {...defaultProps} visualState="indexing" statusLabel="Indexing (42)" />);
    expect(screen.getByText("Indexing (42)")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /SearchMyComputer Status:/i })).toHaveClass("state-indexing");

    rerender(<StatusPill {...defaultProps} visualState="paused" statusLabel="Paused" />);
    expect(screen.getByText("Paused")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /SearchMyComputer Status:/i })).toHaveClass("state-paused");
  });

  it("renders voice input controls and toggles mic", async () => {
    const mockVoice: UseVoiceInputReturn = {
      voiceState: { state: "Idle" },
      isListening: false,
      isTranscribing: false,
      isWakeWordArmed: false,
      audioLevel: 0,
      durationSecs: 0,
      silenceSecs: 0,
      maxSecs: 45,
      errorMessage: null,
      startListening: vi.fn().mockResolvedValue(undefined),
      stopListening: vi.fn().mockResolvedValue(null),
      resetVoice: vi.fn().mockResolvedValue(undefined),
    };

    const { rerender } = render(
      <StatusPill {...defaultProps} voice={mockVoice} />
    );

    const micBtn = screen.getByTitle("Click to speak");
    expect(micBtn).toBeInTheDocument();

    fireEvent.click(micBtn);
    expect(mockVoice.startListening).toHaveBeenCalledTimes(1);

    // Rerender as listening
    const listeningVoice: UseVoiceInputReturn = {
      ...mockVoice,
      voiceState: {
        state: "Listening",
        data: { duration_secs: 2, silence_secs: 0, max_secs: 45, level: 0.6 },
      },
      isListening: true,
      audioLevel: 0.6,
    };

    rerender(
      <StatusPill
        {...defaultProps}
        visualState="listening"
        statusLabel="Listening..."
        voice={listeningVoice}
      />
    );

    const stopMicBtn = screen.getByTitle("Stop voice listening");
    expect(stopMicBtn).toBeInTheDocument();
    expect(stopMicBtn).toHaveClass("active");

    fireEvent.click(stopMicBtn);
    expect(mockVoice.stopListening).toHaveBeenCalledTimes(1);
  });
});
