import React, { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ActionsMenuProps {
  isOpen: boolean;
  onClose: () => void;
  selectedItem: { type: "file" | "project"; path: string; name: string } | null;
  onOpenFile: (path: string) => void;
  onRevealFile: (path: string) => void;
  onOpenTerminal: (path: string) => void;
  onCopyPath: (path: string) => void;
  onOpenSettings: () => void;
  onTogglePause: () => void;
  indexingPaused: boolean;
}

interface ActionItem {
  id: string;
  label: string;
  shortcut?: string;
  icon: string;
  category: string;
  action: () => void;
  visible: boolean;
}

export const ActionsMenu: React.FC<ActionsMenuProps> = ({
  isOpen,
  onClose,
  selectedItem,
  onOpenFile,
  onRevealFile,
  onOpenTerminal,
  onCopyPath,
  onOpenSettings,
  onTogglePause,
  indexingPaused,
}) => {
  const [filter, setFilter] = useState("");
  const [selectedIdx, setSelectedIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const actions: ActionItem[] = [
    // File/Selection actions (visible when an item is selected)
    {
      id: "open",
      label: selectedItem ? `Open ${selectedItem.name}` : "Open Selected",
      shortcut: "Enter",
      icon: "📂",
      category: "Selection",
      action: () => selectedItem && onOpenFile(selectedItem.path),
      visible: selectedItem !== null,
    },
    {
      id: "reveal",
      label: "Reveal in File Explorer",
      shortcut: "Ctrl+Enter",
      icon: "📁",
      category: "Selection",
      action: () => selectedItem && onRevealFile(selectedItem.path),
      visible: selectedItem !== null,
    },
    {
      id: "terminal",
      label: "Open Terminal at Location",
      shortcut: "Alt+T",
      icon: "💻",
      category: "Selection",
      action: () => selectedItem && onOpenTerminal(selectedItem.path),
      visible: selectedItem !== null,
    },
    {
      id: "copy-path",
      label: "Copy File Path",
      shortcut: "Ctrl+C",
      icon: "📋",
      category: "Selection",
      action: () => selectedItem && onCopyPath(selectedItem.path),
      visible: selectedItem !== null,
    },
    // Global actions
    {
      id: "settings",
      label: "Open Settings",
      shortcut: "Ctrl+,",
      icon: "⚙️",
      category: "Global",
      action: onOpenSettings,
      visible: true,
    },
    {
      id: "pause-indexing",
      label: indexingPaused ? "Resume Indexing" : "Pause Indexing",
      icon: indexingPaused ? "▶️" : "⏸",
      category: "Global",
      action: onTogglePause,
      visible: true,
    },
    {
      id: "rescan",
      label: "Rescan All Folders",
      icon: "🔄",
      category: "Global",
      action: () => {
        invoke("start_scan").catch((e) => console.error("Failed to start scan:", e));
      },
      visible: true,
    },
  ];

  const filteredActions = actions.filter(
    (a) => a.visible && a.label.toLowerCase().includes(filter.toLowerCase())
  );

  // Reset selection when filter changes
  useEffect(() => {
    setSelectedIdx(0);
  }, [filter]);

  // Focus input when opened
  useEffect(() => {
    if (isOpen) {
      setFilter("");
      setSelectedIdx(0);

      // Slight delay for DOM render
      requestAnimationFrame(() => {
        inputRef.current?.focus();
      });
    }
  }, [isOpen]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
        return;
      }

      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIdx((prev) => Math.min(prev + 1, filteredActions.length - 1));
        return;
      }

      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIdx((prev) => Math.max(prev - 1, 0));
        return;
      }

      if (e.key === "Enter") {
        e.preventDefault();
        const item = filteredActions[selectedIdx];
        if (item) {
          item.action();
          onClose();
        }
        return;
      }
    },
    [filteredActions, selectedIdx, onClose]
  );

  if (!isOpen) return null;

  // Group actions by category for display
  const grouped: Record<string, ActionItem[]> = {};
  filteredActions.forEach((a) => {
    if (!grouped[a.category]) grouped[a.category] = [];
    grouped[a.category]!.push(a);
  });

  let flatIdx = 0;

  return (
    <div className="actions-menu-overlay" onClick={onClose} role="dialog" aria-label="Actions Menu">
      <div
        className="actions-menu-dialog"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <div className="actions-menu-search">
          <span className="cmd-palette-icon" aria-hidden="true">⌘</span>
          <input
            ref={inputRef}
            type="text"
            className="actions-search-input"
            placeholder="Type a command..."
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            aria-label="Filter commands"
          />
        </div>

        <div className="actions-menu-list" role="listbox">
          {Object.entries(grouped).map(([category, items]) => (
            <React.Fragment key={category}>
              <div className="action-category-label">{category}</div>
              {items.map((action) => {
                const thisIdx = flatIdx++;
                return (
                  <div
                    key={action.id}
                    className={`action-item ${thisIdx === selectedIdx ? "selected" : ""}`}
                    role="option"
                    aria-selected={thisIdx === selectedIdx}
                    onClick={() => {
                      action.action();
                      onClose();
                    }}
                    onMouseEnter={() => setSelectedIdx(thisIdx)}
                  >
                    <span className="action-icon" aria-hidden="true">{action.icon}</span>
                    <span className="action-label">{action.label}</span>
                    {action.shortcut && (
                      <kbd className="action-shortcut">{action.shortcut}</kbd>
                    )}
                  </div>
                );
              })}
            </React.Fragment>
          ))}

          {filteredActions.length === 0 && (
            <div className="actions-empty">No matching commands</div>
          )}
        </div>
      </div>
    </div>
  );
};
