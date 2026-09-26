import React, { useState } from "react";
import { RecentSearches } from "./components/search/RecentSearches";

interface EmptyStateProps {
  onAddFolder: (path: string) => void;
  folderCount: number;
  query?: string;
  recentSearches?: string[];
  onSelectQuery?: (query: string) => void;
  onRemoveRecentSearch?: (query: string) => void;
  onClearRecentSearches?: () => void;
  onOpenSettings?: () => void;
  onOpenActions?: () => void;
}

const EXAMPLE_QUERIES = [
  { icon: "📄", text: "PDF about AI agents downloaded last month", desc: "Temporal & semantic" },
  { icon: "🚀", text: "where is my NutriGrade project", desc: "Project discovery" },
  { icon: "📸", text: "screenshots with a payment QR", desc: "OCR & QR payload" },
  { icon: "💻", text: "kind:code auth token", desc: "Filtered code search" },
];

export const EmptyState: React.FC<EmptyStateProps> = ({
  onAddFolder,
  folderCount,
  query = "",
  recentSearches = [],
  onSelectQuery,
  onRemoveRecentSearch,
  onClearRecentSearches,
  onOpenSettings,
  onOpenActions,
}) => {
  const [folderPath, setFolderPath] = useState("");

  const handleAdd = () => {
    if (folderPath.trim()) {
      onAddFolder(folderPath.trim());
      setFolderPath("");
    }
  };

  // State A: Active search query but 0 results found
  if (query.trim().length > 0) {
    return (
      <div className="empty-state no-results" role="status" aria-live="polite">
        <div className="empty-icon-badge">🔍</div>
        <p className="empty-title">No matches found for &ldquo;{query}&rdquo;</p>
        <p className="empty-subtitle">
          Try broader keywords, adjust natural language phrasing, or check your settings.
        </p>

        <div className="empty-tips-card">
          <span className="empty-tips-heading">Suggestions</span>
          <ul className="empty-tips-list">
            <li>Check for spelling mistakes or typos</li>
            <li>Try searching for file extensions, e.g. <code className="empty-code">ext:pdf</code> or <code className="empty-code">kind:image</code></li>
            <li>
              Trigger a full re-scan or rebuild in{" "}
              <button
                type="button"
                className="empty-link-btn"
                onClick={onOpenActions}
              >
                Actions Menu (Ctrl+K)
              </button>
            </li>
            <li>
              Review folder exclusions in{" "}
              <button
                type="button"
                className="empty-link-btn"
                onClick={onOpenSettings}
              >
                Settings (Ctrl+,)
              </button>
            </li>
          </ul>
        </div>
      </div>
    );
  }

  // State B: Zero folders configured
  if (folderCount === 0) {
    return (
      <div className="empty-state no-folders" role="status">
        <div className="empty-icon-badge">📁</div>
        <p className="empty-title">No folders indexed yet</p>
        <p className="empty-subtitle">
          Add a folder to start searching your files and projects locally
        </p>
        <div className="add-folder-form">
          <input
            type="text"
            className="folder-input"
            placeholder="C:\Users\...\Documents"
            value={folderPath}
            onChange={(e) => setFolderPath(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleAdd()}
            aria-label="Folder path to index"
          />
          <button className="add-btn" onClick={handleAdd}>
            Add Folder
          </button>
        </div>
      </div>
    );
  }

  // State C: Ready to search (zero query, folders indexed)
  return (
    <div className="empty-state idle-state" role="region" aria-label="Search suggestions">
      {recentSearches.length > 0 && onSelectQuery && onRemoveRecentSearch && onClearRecentSearches && (
        <RecentSearches
          recentSearches={recentSearches}
          onSelectQuery={onSelectQuery}
          onRemoveItem={onRemoveRecentSearch}
          onClearHistory={onClearRecentSearches}
        />
      )}

      <div className="empty-suggestions-section">
        <span className="empty-suggestions-title">Try searching for</span>
        <div className="empty-suggestion-grid">
          {EXAMPLE_QUERIES.map((ex) => (
            <button
              key={ex.text}
              type="button"
              className="empty-suggestion-chip"
              onClick={() => onSelectQuery?.(ex.text)}
              title={ex.desc}
            >
              <span className="suggestion-icon">{ex.icon}</span>
              <span className="suggestion-text">{ex.text}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="empty-shortcuts-hint">
        <span><kbd className="kbd-hint">Space</kbd> Preview</span>
        <span><kbd className="kbd-hint">Ctrl+K</kbd> Actions</span>
        <span><kbd className="kbd-hint">Ctrl+,</kbd> Settings</span>
      </div>
    </div>
  );
};
