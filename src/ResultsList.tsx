import React, { useRef, useState, useEffect } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { SearchResult, ProjectRecord } from "./types";
import { getFileIcon } from "./fileIcons";
import { ProjectCard } from "./ProjectCard";

const ESTIMATED_ROW_HEIGHT = 58;
const VIRTUALIZE_THRESHOLD = 40;
const OVERSCAN = 8;

interface ResultsListProps {
  results: SearchResult[];
  projectResults?: ProjectRecord[];
  selectedIndex: number;
  onSelect: (index: number) => void;
  onOpenFile: (result: SearchResult) => void;
  onOpenProject: (project: ProjectRecord) => void;
  onRevealProject: (project: ProjectRecord) => void;
  onOpenTerminalProject: (project: ProjectRecord) => void;
  onPreviewImage?: (result: SearchResult) => void;
}

function getTypeBadge(ext: string, kind: string): { label: string; className: string } {
  const e = ext.toLowerCase();
  switch (e) {
    case "pdf":
      return { label: "PDF", className: "badge-pdf" };
    case "doc":
    case "docx":
      return { label: "DOCX", className: "badge-docx" };
    case "ppt":
    case "pptx":
      return { label: "PPTX", className: "badge-pptx" };
    case "xls":
    case "xlsx":
    case "csv":
      return { label: e.toUpperCase(), className: "badge-sheet" };
    case "png":
    case "jpg":
    case "jpeg":
    case "webp":
    case "gif":
    case "bmp":
    case "svg":
      return { label: e.toUpperCase(), className: "badge-image" };
    case "rs":
    case "py":
    case "ts":
    case "tsx":
    case "js":
    case "jsx":
    case "c":
    case "cpp":
    case "java":
    case "cs":
    case "go":
      return { label: e.toUpperCase(), className: "badge-code" };
    case "md":
    case "txt":
    case "json":
    case "log":
      return { label: e.toUpperCase(), className: "badge-text" };
    default:
      if (kind === "image") return { label: "IMG", className: "badge-image" };
      if (kind === "code") return { label: "CODE", className: "badge-code" };
      if (kind === "document") return { label: "DOC", className: "badge-docx" };
      return { label: e.toUpperCase() || kind.toUpperCase(), className: "badge-default" };
  }
}

export const ResultsList: React.FC<ResultsListProps> = ({
  results,
  projectResults = [],
  selectedIndex,
  onSelect,
  onOpenFile,
  onOpenProject,
  onRevealProject,
  onOpenTerminalProject,
  onPreviewImage,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(480);

  const hasProjects = projectResults.length > 0;
  const hasFiles = results.length > 0;
  const shouldVirtualize = results.length > VIRTUALIZE_THRESHOLD;

  // Listen to parent scroll container
  useEffect(() => {
    const parent = containerRef.current?.closest(".content-area");
    if (!parent) return;

    const updateDimensions = () => {
      setScrollTop(parent.scrollTop);
      setViewportHeight(parent.clientHeight || 480);
    };

    updateDimensions();
    parent.addEventListener("scroll", updateDimensions, { passive: true });
    window.addEventListener("resize", updateDimensions);

    return () => {
      parent.removeEventListener("scroll", updateDimensions);
      window.removeEventListener("resize", updateDimensions);
    };
  }, [hasFiles, hasProjects]);

  // Ensure selected item is scrolled into view
  useEffect(() => {
    if (selectedIndex < 0) return;
    const parent = containerRef.current?.closest(".content-area");
    if (!parent) return;

    // Small delay to allow layout
    requestAnimationFrame(() => {
      const selectedEl = parent.querySelector(".result-item.selected, .project-card.selected") as HTMLElement | null;
      if (selectedEl) {
        const parentRect = parent.getBoundingClientRect();
        const elRect = selectedEl.getBoundingClientRect();
        if (elRect.top < parentRect.top) {
          parent.scrollTop -= (parentRect.top - elRect.top) + 8;
        } else if (elRect.bottom > parentRect.bottom) {
          parent.scrollTop += (elRect.bottom - parentRect.bottom) + 8;
        }
      }
    });
  }, [selectedIndex]);

  if (!hasProjects && !hasFiles) {
    return null;
  }

  // Calculate virtual window slice for files
  let startIndex = 0;
  let endIndex = results.length;
  let topSpacerHeight = 0;
  let bottomSpacerHeight = 0;

  if (shouldVirtualize) {
    const rawStart = Math.floor(scrollTop / ESTIMATED_ROW_HEIGHT);
    const visibleCount = Math.ceil(viewportHeight / ESTIMATED_ROW_HEIGHT);
    startIndex = Math.max(0, rawStart - OVERSCAN);
    endIndex = Math.min(results.length, rawStart + visibleCount + OVERSCAN);

    // If selectedIndex is a file item outside current range, expand slice to include it
    const selectedFileIdx = selectedIndex - projectResults.length;
    if (selectedFileIdx >= 0 && selectedFileIdx < results.length) {
      if (selectedFileIdx < startIndex) {
        startIndex = Math.max(0, selectedFileIdx - 2);
      } else if (selectedFileIdx >= endIndex) {
        endIndex = Math.min(results.length, selectedFileIdx + 3);
      }
    }

    topSpacerHeight = startIndex * ESTIMATED_ROW_HEIGHT;
    bottomSpacerHeight = Math.max(0, (results.length - endIndex) * ESTIMATED_ROW_HEIGHT);
  }

  const visibleResults = shouldVirtualize
    ? results.slice(startIndex, endIndex).map((r, i) => ({ result: r, originalIdx: startIndex + i }))
    : results.map((r, i) => ({ result: r, originalIdx: i }));

  return (
    <div className="results-list" role="listbox" ref={containerRef}>
      {hasProjects && (
        <div className="projects-section">
          <div className="section-header">
            <span className="section-title">Projects</span>
            <span className="section-count">{projectResults.length}</span>
          </div>
          <div className="projects-grid">
            {projectResults.map((project, idx) => {
              const isSelected = idx === selectedIndex;
              return (
                <ProjectCard
                  key={`proj-${project.id}-${project.path}`}
                  project={project}
                  isSelected={isSelected}
                  onSelect={() => onSelect(idx)}
                  onOpen={onOpenProject}
                  onReveal={onRevealProject}
                  onOpenTerminal={onOpenTerminalProject}
                />
              );
            })}
          </div>
        </div>
      )}

      {hasFiles && (
        <div className="files-section">
          {hasProjects && (
            <div className="section-header">
              <span className="section-title">Files & Documents</span>
              <span className="section-count">{results.length}</span>
            </div>
          )}

          {shouldVirtualize && topSpacerHeight > 0 && (
            <div
              className="virtual-spacer-top"
              style={{ height: `${topSpacerHeight}px` }}
              aria-hidden="true"
            />
          )}

          {visibleResults.map(({ result, originalIdx }) => {
            const globalIdx = projectResults.length + originalIdx;
            const isSelected = globalIdx === selectedIndex;
            const icon = getFileIcon(result.ext, result.kind);
            const badge = getTypeBadge(result.ext, result.kind);
            const isImage = result.kind === "image" || ["png", "jpg", "jpeg", "webp", "bmp"].includes(result.ext.toLowerCase());

            return (
              <div
                key={`file-${result.id}-${result.path}-${originalIdx}`}
                className={`result-item ${isSelected ? "selected" : ""}`}
                role="option"
                aria-selected={isSelected}
                onClick={() => {
                  onSelect(globalIdx);
                  onOpenFile(result);
                }}
                onMouseEnter={() => onSelect(globalIdx)}
              >
                {isImage ? (
                  <div className="thumbnail-wrapper">
                    <img
                      src={convertFileSrc(result.path)}
                      alt={result.name}
                      className="result-thumbnail"
                      onError={(e) => {
                        (e.target as HTMLElement).style.display = "none";
                      }}
                    />
                    <span className="file-icon fallback-icon">{icon}</span>
                  </div>
                ) : (
                  <span className="file-icon">{icon}</span>
                )}

                <div className="file-info">
                  <div className="file-header">
                    <span className="file-name">{result.name}</span>
                    <span className={`type-badge ${badge.className}`}>{badge.label}</span>
                    {result.page !== undefined && result.page !== null && (
                      <span className="page-badge">p. {result.page}</span>
                    )}
                    {result.section && (
                      <span className="section-badge">{result.section}</span>
                    )}
                    {result.symbol && (
                      <span className="symbol-badge">{result.symbol}</span>
                    )}
                    {result.match_type === "hybrid" && (
                      <span className="match-badge">hybrid</span>
                    )}
                    {isImage && (
                      <span
                        className="preview-hint-badge"
                        onClick={(e) => {
                          e.stopPropagation();
                          if (onPreviewImage) onPreviewImage(result);
                        }}
                        title="Press Space for Quick Look"
                      >
                        Space to Preview
                      </span>
                    )}
                  </div>
                  <span className="file-parent">{result.parent_dir}</span>
                  {result.snippet && (
                    <div
                      className="file-snippet"
                      dangerouslySetInnerHTML={{ __html: result.snippet }}
                    />
                  )}
                  {result.matches && result.matches.length > 0 && isSelected && (
                    <div className="secondary-matches">
                      <div className="secondary-matches-header">
                        +{result.matches.length} other match{result.matches.length > 1 ? "es" : ""} in file:
                      </div>
                      {result.matches.map((m) => (
                        <div key={m.chunk_id} className="secondary-match-item">
                          <div className="secondary-match-meta">
                            {m.page !== undefined && m.page !== null && (
                              <span className="page-badge">p. {m.page}</span>
                            )}
                            {m.section && <span className="section-badge">{m.section}</span>}
                            {m.symbol && <span className="symbol-badge">{m.symbol}</span>}
                          </div>
                          <div
                            className="file-snippet secondary-snippet"
                            dangerouslySetInnerHTML={{ __html: m.snippet }}
                          />
                        </div>
                      ))}
                    </div>
                  )}
                </div>
                <span className="file-ext">.{result.ext}</span>
              </div>
            );
          })}

          {shouldVirtualize && bottomSpacerHeight > 0 && (
            <div
              className="virtual-spacer-bottom"
              style={{ height: `${bottomSpacerHeight}px` }}
              aria-hidden="true"
            />
          )}
        </div>
      )}
    </div>
  );
};

