import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SettingsModal } from "../components/settings/SettingsModal";
import * as tauriCore from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (p: string) => `asset://${p}`,
}));

describe("SettingsModal Component", () => {
  const mockConfig = {
    indexed_folders: ["C:/Users/dev/Documents"],
    exclusions: ["node_modules", "*.tmp"],
    hotkey: "Alt+Space",
    theme: "dark",
    language: "en",
    launch_at_login: false,
    index_documents: true,
    index_code: true,
    index_spreadsheets: true,
    index_archives: false,
    enable_image_indexing: true,
    image_folders: [],
    max_file_size: 32 * 1024 * 1024,
    index_threads: 2,
    max_ram_mb: 512,
    battery_policy: "eco",
    onboarding_completed: true,
    indexing_paused: false,
  };

  beforeEach(() => {
    vi.clearAllMocks();
    (tauriCore.invoke as ReturnType<typeof vi.fn>).mockImplementation((cmd: string) => {
      if (cmd === "get_config") {
        return Promise.resolve(mockConfig);
      }
      if (cmd === "test_exclusion_pattern") {
        return Promise.resolve({ matches: true, error: null });
      }
      if (cmd === "get_problem_files") {
        return Promise.resolve([
          {
            id: 1,
            path: "C:/corrupt.pdf",
            error_kind: "CorruptedPDF",
            error_message: "Trailer not found",
            attempts: 3,
            last_failed_at: "2026-09-26T00:00:00Z",
          },
        ]);
      }
      if (cmd === "get_governor_status") {
        return Promise.resolve({
          is_on_ac: true,
          battery_percentage: 100,
          is_low_battery: false,
          state: "Normal",
          active_threads: 2,
          is_thermal_throttled: false,
        });
      }
      return Promise.resolve({});
    });
  });

  it("renders modal with 6 tabs and allows tab switching", async () => {
    render(
      <SettingsModal
        isOpen={true}
        onClose={vi.fn()}
        status={null}
        onRefreshStatus={vi.fn().mockResolvedValue(undefined)}
      />
    );

    await waitFor(() => {
      expect(screen.getByRole("tab", { name: /General/i })).toBeInTheDocument();
      expect(screen.getByRole("tab", { name: /Folders/i })).toBeInTheDocument();
      expect(screen.getByRole("tab", { name: /File Types/i })).toBeInTheDocument();
      expect(screen.getByRole("tab", { name: /Performance/i })).toBeInTheDocument();
      expect(screen.getByRole("tab", { name: /Health/i })).toBeInTheDocument();
      expect(screen.getByRole("tab", { name: /Privacy/i })).toBeInTheDocument();
    });

    // Default tab is General
    expect(screen.getByText("Hotkey")).toBeInTheDocument();
    expect(screen.getByText("Alt+Space")).toBeInTheDocument();

    // Switch to Folders tab
    fireEvent.click(screen.getByRole("tab", { name: /Folders/i }));
    expect(screen.getByText("Indexed Folders")).toBeInTheDocument();
    expect(screen.getByText("Exclusion Patterns")).toBeInTheDocument();
  });

  it("runs the Glob Tester on the Folders tab", async () => {
    render(
      <SettingsModal
        isOpen={true}
        onClose={vi.fn()}
        status={null}
        onRefreshStatus={vi.fn().mockResolvedValue(undefined)}
      />
    );

    // Navigate to Folders tab
    await waitFor(() => screen.getByRole("tab", { name: /Folders/i }));
    fireEvent.click(screen.getByRole("tab", { name: /Folders/i }));

    const patternInput = screen.getByLabelText("Pattern");
    const pathInput = screen.getByLabelText("Test Path");
    const testBtn = screen.getByRole("button", { name: /Test Match/i });

    fireEvent.change(patternInput, { target: { value: "**/*.log" } });
    fireEvent.change(pathInput, { target: { value: "C:/app/error.log" } });
    fireEvent.click(testBtn);

    await waitFor(() => {
      expect(screen.getByText(/Pattern MATCHES/i)).toBeInTheDocument();
    });
  });

  it("renders Health tab problem files and Privacy tab data management controls", async () => {
    render(
      <SettingsModal
        isOpen={true}
        onClose={vi.fn()}
        status={null}
        onRefreshStatus={vi.fn().mockResolvedValue(undefined)}
      />
    );

    // Switch to Health tab
    await waitFor(() => screen.getByRole("tab", { name: /Health/i }));
    fireEvent.click(screen.getByRole("tab", { name: /Health/i }));

    await waitFor(() => {
      expect(screen.getByText("C:/corrupt.pdf")).toBeInTheDocument();
      expect(screen.getByText("CorruptedPDF")).toBeInTheDocument();
    });

    // Switch to Privacy tab
    fireEvent.click(screen.getByRole("tab", { name: /Privacy/i }));
    expect(screen.getByText("100% Offline — No data ever leaves your device")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Delete All Index Data/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Rebuild Index From Scratch/i })).toBeInTheDocument();
  });
});
