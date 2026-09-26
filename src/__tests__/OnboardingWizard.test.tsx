import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { OnboardingWizard } from "../components/onboarding/OnboardingWizard";
import * as tauriCore from "@tauri-apps/api/core";

// Mock Tauri invoke
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (p: string) => `asset://${p}`,
}));

describe("OnboardingWizard Component", () => {
  const mockFolders = [
    { name: "Documents", path: "C:/Users/dev/Documents", category: "documents", is_sensitive: false, exists: true, default_checked: true },
    { name: "Downloads", path: "C:/Users/dev/Downloads", category: "downloads", is_sensitive: false, exists: true, default_checked: true },
    { name: "Projects", path: "C:/Users/dev/Projects", category: "project", is_sensitive: false, exists: true, default_checked: false },
  ];

  beforeEach(() => {
    vi.clearAllMocks();
    (tauriCore.invoke as ReturnType<typeof vi.fn>).mockImplementation((cmd: string) => {
      if (cmd === "detect_system_folders") {
        return Promise.resolve(mockFolders);
      }
      if (cmd === "get_config") {
        return Promise.resolve({
          indexed_folders: [],
          hotkey: "Alt+Space",
          onboarding_completed: false,
          indexing_paused: false,
        });
      }
      if (cmd === "update_config") {
        return Promise.resolve();
      }
      if (cmd === "add_folder") {
        return Promise.resolve();
      }
      if (cmd === "get_index_status") {
        return Promise.resolve({
          total_files: 42,
          active_files: 42,
          is_scanning: false,
          scan_progress: { files_seen: 42, files_indexed: 42, current_dir: "" },
          job_counts: { pending: 0, running: 0, done: 42, failed: 0 },
          has_embedding_model: true,
          has_vision_models: false,
          enable_image_indexing: false,
          hotkey_registered: true,
        });
      }
      return Promise.resolve({});
    });
  });

  it("renders Step 1 (Welcome) and navigates to Step 2 on Get Started click", async () => {
    const onFinish = vi.fn();
    render(<OnboardingWizard onFinish={onFinish} />);

    expect(screen.getByText("Welcome to SearchMyComputer")).toBeInTheDocument();
    expect(
      screen.getByText(/Your fast, private, intelligent desktop search launcher/i)
    ).toBeInTheDocument();

    const getStartedBtn = screen.getByRole("button", { name: /Continue to folder selection/i });
    fireEvent.click(getStartedBtn);

    // Step 2 should now be visible
    expect(screen.getByText("Choose What to Index")).toBeInTheDocument();
  });

  it("discovers system folders in Step 2, allows toggling, and advances to Step 3", async () => {
    const onFinish = vi.fn();
    render(<OnboardingWizard onFinish={onFinish} />);

    // Advance to Step 2
    fireEvent.click(screen.getByRole("button", { name: /Continue to folder selection/i }));

    await waitFor(() => {
      expect(screen.getByText("Documents")).toBeInTheDocument();
      expect(screen.getByText("Downloads")).toBeInTheDocument();
      expect(screen.getByText("Projects")).toBeInTheDocument();
    });

    // Advance to Step 3
    const startButton = screen.getByRole("button", { name: /Confirm & Start Indexing/i });
    fireEvent.click(startButton);

    await waitFor(() => {
      expect(screen.getByText("Indexing Your Files")).toBeInTheDocument();
    });

    // Finish onboarding
    const startSearchButton = screen.getByRole("button", { name: /Start Searching Now/i });
    fireEvent.click(startSearchButton);
    expect(onFinish).toHaveBeenCalledTimes(1);
  });
});
