import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ResultsList } from "../ResultsList";
import { SearchResult, ProjectRecord } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (p: string) => `asset://${p}`,
}));

describe("ResultsList Component & Virtualization", () => {
  const mockProjects: ProjectRecord[] = [
    {
      id: 1,
      name: "search-my-computer",
      path: "C:\\Users\\dev\\search-my-computer",
      project_type: "rust",
      last_detected_at: "2026-09-26T00:00:00Z",
    },
  ];

  const mockFiles: SearchResult[] = [
    {
      id: 1,
      name: "config.rs",
      path: "C:\\Users\\dev\\search-my-computer\\crates\\smc-core\\src\\config.rs",
      parent_dir: "crates/smc-core/src",
      ext: "rs",
      size: 1024,
      mtime: "2026-09-26T00:00:00Z",
      kind: "code",
      snippet: "pub struct <b>AppConfig</b>",
      score: 0.95,
      match_type: "hybrid",
    },
    {
      id: 2,
      name: "receipt.png",
      path: "C:\\Users\\dev\\Pictures\\receipt.png",
      parent_dir: "Pictures",
      ext: "png",
      size: 2048,
      mtime: "2026-09-26T00:00:00Z",
      kind: "image",
      score: 0.88,
      match_type: "keyword",
    },
  ];

  it("renders project sections and file items with selection and click handling", () => {
    const onSelect = vi.fn();
    const onOpenFile = vi.fn();
    const onOpenProject = vi.fn();
    const onRevealProject = vi.fn();
    const onOpenTerminalProject = vi.fn();

    render(
      <ResultsList
        results={mockFiles}
        projectResults={mockProjects}
        selectedIndex={1}
        onSelect={onSelect}
        onOpenFile={onOpenFile}
        onOpenProject={onOpenProject}
        onRevealProject={onRevealProject}
        onOpenTerminalProject={onOpenTerminalProject}
      />
    );

    expect(screen.getByText("search-my-computer")).toBeInTheDocument();
    expect(screen.getByText("config.rs")).toBeInTheDocument();
    expect(screen.getByText("receipt.png")).toBeInTheDocument();

    // Click project
    const projectCard = screen.getByText("search-my-computer");
    fireEvent.click(projectCard);
    expect(onSelect).toHaveBeenCalled();

    // Click file item
    const fileItem = screen.getByText("config.rs");
    fireEvent.click(fileItem);
    expect(onOpenFile).toHaveBeenCalledWith(mockFiles[0]);
  });

  it("virtualizes large result lists (1,000+ items) capping active DOM nodes to O(1) viewport slice", () => {
    const thousandsResults: SearchResult[] = Array.from({ length: 1200 }, (_, i) => ({
      id: i + 1,
      name: `item_${i + 1}.rs`,
      path: `C:\\Users\\dev\\project\\item_${i + 1}.rs`,
      parent_dir: "src",
      ext: "rs",
      size: 512,
      mtime: "2026-09-26T00:00:00Z",
      kind: "code",
      score: 0.5,
      match_type: "hybrid",
    }));

    const { container } = render(
      <div className="content-area" style={{ height: "480px", overflow: "auto" }}>
        <ResultsList
          results={thousandsResults}
          projectResults={[]}
          selectedIndex={0}
          onSelect={vi.fn()}
          onOpenFile={vi.fn()}
          onOpenProject={vi.fn()}
          onRevealProject={vi.fn()}
          onOpenTerminalProject={vi.fn()}
        />
      </div>
    );

    const renderedItems = container.querySelectorAll(".result-item");
    // With 1200 items, virtualization should limit rendered DOM elements to a small window (approx <= 30 nodes)
    expect(renderedItems.length).toBeLessThanOrEqual(35);
    expect(renderedItems.length).toBeGreaterThan(0);

    // Verify top or bottom spacer exists to maintain scroll dimension
    const bottomSpacer = container.querySelector(".virtual-spacer-bottom");
    expect(bottomSpacer).toBeInTheDocument();
  });
});
