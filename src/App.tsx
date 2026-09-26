import React, { useState, useRef, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath } from "@tauri-apps/plugin-opener";
import { SearchBar } from "./SearchBar";
import { ResultsList } from "./ResultsList";
import { StatusLine } from "./StatusLine";
import { EmptyState } from "./EmptyState";
import { ImagePreviewModal } from "./ImagePreviewModal";
import { SettingsModal } from "./components/settings/SettingsModal";
import { OnboardingWizard } from "./components/onboarding/OnboardingWizard";
import { PreviewPane } from "./components/preview/PreviewPane";
import { ActionsMenu } from "./components/actions/ActionsMenu";
import { TrialBanner } from "./components/license/TrialBanner";
import { LicenseModal } from "./components/license/LicenseModal";
import { StatusPill } from "./components/StatusPill";
import { useSearch } from "./hooks/useSearch";
import { useIndexStatus } from "./hooks/useIndexStatus";
import { useVoiceInput } from "./hooks/useVoiceInput";
import { SearchResult, ProjectRecord, AppConfig, LicenseStatus, StatusPillVisualState } from "./types";
import "./App.css";

export const App: React.FC = () => {
  const [isExpanded, setIsExpanded] = useState(false);
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [previewImage, setPreviewImage] = useState<SearchResult | null>(null);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [showLicenseModal, setShowLicenseModal] = useState(false);
  const [licenseStatus, setLicenseStatus] = useState<LicenseStatus | null>(null);
  const [showOnboarding, setShowOnboarding] = useState<boolean | null>(null);
  const [showPreviewPane, setShowPreviewPane] = useState(false);
  const [showActionsMenu, setShowActionsMenu] = useState(false);
  const [indexingPaused, setIndexingPaused] = useState(false);
  const [recentSearches, setRecentSearches] = useState<string[]>(() => {
    try {
      const stored = localStorage.getItem("smc_recent_searches");
      return stored ? JSON.parse(stored) : [];
    } catch {
      return [];
    }
  });
  const inputRef = useRef<HTMLInputElement>(null);

  const saveRecentQuery = useCallback((q: string) => {
    const trimmed = q.trim();
    if (!trimmed) return;
    setRecentSearches((prev) => {
      const updated = [
        trimmed,
        ...prev.filter((item) => item.toLowerCase() !== trimmed.toLowerCase()),
      ].slice(0, 8);
      try {
        localStorage.setItem("smc_recent_searches", JSON.stringify(updated));
      } catch {
        // ignore storage errors
      }
      return updated;
    });
  }, []);

  const handleSelectRecentQuery = useCallback((q: string) => {
    setQuery(q);
    requestAnimationFrame(() => {
      inputRef.current?.focus();
    });
  }, []);

  const handleRemoveRecentSearch = useCallback((itemToRemove: string) => {
    setRecentSearches((prev) => {
      const updated = prev.filter((item) => item !== itemToRemove);
      try {
        localStorage.setItem("smc_recent_searches", JSON.stringify(updated));
      } catch {
        // ignore
      }
      return updated;
    });
  }, []);

  const handleClearRecentSearches = useCallback(() => {
    setRecentSearches([]);
    try {
      localStorage.removeItem("smc_recent_searches");
    } catch {
      // ignore
    }
  }, []);

  const { results, projectResults, parsedQuery } = useSearch(query);
  const totalItems = results.length + projectResults.length;
  const { status, refreshStatus } = useIndexStatus();

  const handleExpand = useCallback(async () => {
    try {
      await invoke("expand_launcher_window", { previewOpen: showPreviewPane });
      setIsExpanded(true);
      requestAnimationFrame(() => {
        inputRef.current?.focus();
      });
    } catch (e) {
      console.error("failed to expand launcher window:", e);
      setIsExpanded(true);
    }
  }, [showPreviewPane]);

  const handleCollapse = useCallback(async () => {
    try {
      setShowActionsMenu(false);
      setShowPreviewPane(false);
      setPreviewImage(null);
      setIsSettingsOpen(false);
      setShowLicenseModal(false);
      await invoke("collapse_launcher_window");
      setIsExpanded(false);
    } catch (e) {
      console.error("failed to collapse launcher window:", e);
      setIsExpanded(false);
    }
  }, []);

  const voice = useVoiceInput({
    onTranscription: (text) => {
      setQuery(text);
      handleExpand();
      requestAnimationFrame(() => {
        inputRef.current?.focus();
      });
    },
  });

  const loadLicenseStatus = useCallback(async () => {
    try {
      const s = await invoke<LicenseStatus>("get_license_status");
      setLicenseStatus(s);
    } catch (e) {
      console.error("failed to get license status:", e);
    }
  }, []);

  // Check onboarding state and license status on mount
  useEffect(() => {
    invoke<AppConfig>("get_config")
      .then((cfg) => {
        setShowOnboarding(!cfg.onboarding_completed);
        setIndexingPaused(cfg.indexing_paused);
        if (!cfg.onboarding_completed) {
          invoke("expand_launcher_window", { previewOpen: false }).catch(() => {});
        }
      })
      .catch(() => setShowOnboarding(false));

    loadLicenseStatus();
  }, [loadLicenseStatus]);

  // Sync paused state from status
  useEffect(() => {
    if (status) {
      setIndexingPaused(status.indexing_paused);
    }
  }, [status]);

  // Reset selection index when results change.
  useEffect(() => {
    setSelectedIndex(0);
  }, [projectResults, results]);

  // Focus search input when window is expanded.
  useEffect(() => {
    if (isExpanded && !showOnboarding) {
      inputRef.current?.focus();
    }
  }, [isExpanded, showOnboarding]);

  // Setup Tauri event listeners for toggle, collapse, and voice events
  useEffect(() => {
    let unlistenToggle: (() => void) | null = null;
    let unlistenCollapse: (() => void) | null = null;
    let unlistenWakeWord: (() => void) | null = null;
    let unlistenTranscription: (() => void) | null = null;

    listen("launcher-toggle", () => {
      setIsExpanded((prev) => {
        if (prev) {
          handleCollapse();
          return false;
        } else {
          handleExpand();
          return true;
        }
      });
    }).then((unsub) => {
      unlistenToggle = unsub;
    });

    listen("launcher-collapse", () => {
      handleCollapse();
    }).then((unsub) => {
      unlistenCollapse = unsub;
    });

    listen("voice:wake_word", () => {
      handleExpand();
    }).then((unsub) => {
      unlistenWakeWord = unsub;
    });

    listen<string>("voice:transcription", (event) => {
      const text = event.payload?.trim();
      if (text) {
        setQuery(text);
        handleExpand();
      }
    }).then((unsub) => {
      unlistenTranscription = unsub;
    });

    return () => {
      if (unlistenToggle) unlistenToggle();
      if (unlistenCollapse) unlistenCollapse();
      if (unlistenWakeWord) unlistenWakeWord();
      if (unlistenTranscription) unlistenTranscription();
    };
  }, [handleExpand, handleCollapse]);

  const getSelectedItem = useCallback((): {
    type: "project";
    item: ProjectRecord;
  } | {
    type: "file";
    item: SearchResult;
  } | null => {
    if (selectedIndex < projectResults.length) {
      const p = projectResults[selectedIndex];
      return p ? { type: "project", item: p } : null;
    }
    const fileIdx = selectedIndex - projectResults.length;
    const f = results[fileIdx];
    return f ? { type: "file", item: f } : null;
  }, [selectedIndex, projectResults, results]);

  // Derive the preview pane item from the current selection
  const getPreviewPaneItem = useCallback(() => {
    const sel = getSelectedItem();
    if (!sel) return null;
    if (sel.type === "project") {
      return { type: "project" as const, project: sel.item };
    }
    return { type: "file" as const, result: sel.item };
  }, [getSelectedItem]);

  // Derive the actions menu selected-item shape
  const getActionsSelectedItem = useCallback(() => {
    const sel = getSelectedItem();
    if (!sel) return null;
    return {
      type: sel.type,
      path: sel.item.path,
      name: sel.type === "project" ? sel.item.name : sel.item.name,
    };
  }, [getSelectedItem]);

  const handleOpenFile = async (result: SearchResult) => {
    saveRecentQuery(query);
    try {
      await openPath(result.path);
      await handleCollapse();
    } catch (e) {
      console.error("failed to open file:", e);
    }
  };

  const handleRevealFile = async (result: SearchResult) => {
    saveRecentQuery(query);
    try {
      await invoke("reveal_in_folder", { path: result.path });
      await handleCollapse();
    } catch (e) {
      console.error("failed to reveal file:", e);
    }
  };

  const handleOpenTerminalFile = async (result: SearchResult) => {
    saveRecentQuery(query);
    try {
      await invoke("open_terminal_in_folder", { path: result.path });
      await handleCollapse();
    } catch (e) {
      console.error("failed to open terminal for file:", e);
    }
  };

  const handleOpenProject = async (project: ProjectRecord) => {
    saveRecentQuery(query);
    try {
      await openPath(project.path);
      await handleCollapse();
    } catch (e) {
      console.error("failed to open project:", e);
    }
  };

  const handleRevealProject = async (project: ProjectRecord) => {
    saveRecentQuery(query);
    try {
      await invoke("reveal_in_folder", { path: project.path });
      await handleCollapse();
    } catch (e) {
      console.error("failed to reveal project:", e);
    }
  };

  const handleOpenTerminalProject = async (project: ProjectRecord) => {
    saveRecentQuery(query);
    try {
      await invoke("open_terminal_in_folder", { path: project.path });
      await handleCollapse();
    } catch (e) {
      console.error("failed to open terminal for project:", e);
    }
  };

  // Path-based action helpers for ActionsMenu/PreviewPane
  const handleOpenByPath = async (path: string) => {
    try {
      await openPath(path);
      await handleCollapse();
    } catch (e) {
      console.error("failed to open path:", e);
    }
  };

  const handleRevealByPath = async (path: string) => {
    try {
      await invoke("reveal_in_folder", { path });
      await handleCollapse();
    } catch (e) {
      console.error("failed to reveal path:", e);
    }
  };

  const handleOpenTerminalByPath = async (path: string) => {
    try {
      await invoke("open_terminal_in_folder", { path });
      await handleCollapse();
    } catch (e) {
      console.error("failed to open terminal:", e);
    }
  };

  const handleCopyPath = async (path: string) => {
    try {
      await navigator.clipboard.writeText(path);
    } catch (e) {
      console.error("failed to copy path:", e);
    }
  };

  const handleTogglePause = async () => {
    try {
      const newPaused = !indexingPaused;
      const currentConfig = await invoke<AppConfig>("get_config");
      await invoke("update_config", {
        newConfig: { ...currentConfig, indexing_paused: newPaused },
      });
      setIndexingPaused(newPaused);
      await refreshStatus();
    } catch (e) {
      console.error("failed to toggle pause:", e);
    }
  };

  const handleKeyDown = async (e: React.KeyboardEvent<HTMLInputElement>) => {
    // Ctrl+K opens actions menu
    if (e.key === "k" && e.ctrlKey) {
      e.preventDefault();
      setShowActionsMenu(true);
      return;
    }

    // Ctrl+, opens settings
    if (e.key === "," && e.ctrlKey) {
      e.preventDefault();
      setIsSettingsOpen(true);
      return;
    }

    if (e.key === "Escape") {
      e.preventDefault();
      if (showActionsMenu) {
        setShowActionsMenu(false);
        return;
      }
      if (showPreviewPane) {
        setShowPreviewPane(false);
        await invoke("expand_launcher_window", { previewOpen: false });
        return;
      }
      if (previewImage) {
        setPreviewImage(null);
        return;
      }
      if (isSettingsOpen) {
        setIsSettingsOpen(false);
        return;
      }
      await handleCollapse();
      return;
    }

    // Space or Tab toggles preview pane (when results exist)
    if ((e.key === " " || e.key === "Tab") && !e.ctrlKey && !e.altKey && !e.shiftKey && totalItems > 0) {
      // Only intercept Space if input is empty (don't block typing spaces in queries)
      if (e.key === " " && query.trim().length > 0) {
        return;
      }
      e.preventDefault();
      const next = !showPreviewPane;
      setShowPreviewPane(next);
      await invoke("expand_launcher_window", { previewOpen: next });
      return;
    }

    // Ctrl+Space / Shift+Space for image preview
    if (e.key === " " && (e.ctrlKey || e.shiftKey)) {
      const sel = getSelectedItem();
      if (sel && sel.type === "file") {
        const isImg = sel.item.kind === "image" || ["png", "jpg", "jpeg", "webp", "bmp"].includes(sel.item.ext.toLowerCase());
        if (isImg) {
          e.preventDefault();
          setPreviewImage(sel.item);
          return;
        }
      }
    }

    if (totalItems === 0) return;

    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((prev) => Math.min(prev + 1, totalItems - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex((prev) => Math.max(prev - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const sel = getSelectedItem();
      if (!sel) return;

      if (e.ctrlKey) {
        if (sel.type === "project") handleRevealProject(sel.item);
        else handleRevealFile(sel.item);
      } else {
        if (sel.type === "project") handleOpenProject(sel.item);
        else handleOpenFile(sel.item);
      }
    } else if (e.altKey && (e.key === "o" || e.key === "O")) {
      e.preventDefault();
      const sel = getSelectedItem();
      if (!sel) return;
      if (sel.type === "project") handleRevealProject(sel.item);
      else handleRevealFile(sel.item);
    } else if (e.altKey && (e.key === "t" || e.key === "T")) {
      e.preventDefault();
      const sel = getSelectedItem();
      if (!sel) return;
      if (sel.type === "project") handleOpenTerminalProject(sel.item);
      else handleOpenTerminalFile(sel.item);
    } else if (e.key === "c" && e.ctrlKey) {
      const sel = getSelectedItem();
      if (sel) {
        try {
          await navigator.clipboard.writeText(sel.item.path);
        } catch (err) {
          console.error("failed to copy path:", err);
        }
      }
    }
  };

  const handleAddFolder = async (folderPath: string) => {
    try {
      await invoke("add_folder", { folderPath });
      await refreshStatus();
    } catch (e) {
      alert(`Failed to add folder: ${e}`);
    }
  };

  const handleFinishOnboarding = () => {
    setShowOnboarding(false);
    handleCollapse();
  };

  const isIndexingInProgress = Boolean(
    status?.is_scanning ||
    (status?.job_counts && (status.job_counts.running > 0 || status.job_counts.pending > 0))
  );

  // Derive StatusPill visual state & label
  const visualState: StatusPillVisualState = (() => {
    if (voice.isListening) return "listening";
    if (voice.isTranscribing) return "searching";
    if (query.trim().length > 0) return totalItems > 0 ? "results" : "searching";
    if (indexingPaused) return "paused";
    if (isIndexingInProgress) return "indexing";
    return "idle";
  })();

  const statusLabel: string = (() => {
    if (voice.isListening) return "Listening...";
    if (voice.isTranscribing) return "Transcribing...";
    if (query.trim().length > 0) {
      return totalItems > 0 ? `${totalItems} results` : "Searching...";
    }
    if (indexingPaused) return "Paused";
    if (isIndexingInProgress) {
      const pending = status?.job_counts?.pending ?? 0;
      return `Indexing (${pending})`;
    }
    return status ? `${status.total_files.toLocaleString()} files` : "SearchMyComputer";
  })();

  // Loading state — haven't checked onboarding yet
  if (showOnboarding === null) {
    return (
      <div className="launcher-window">
        <div className="status-pill-shell state-idle">
          <div className="status-pill-indicator-wrap">
            <div className="status-pill-dot dot-idle" />
          </div>
          <div className="status-pill-content">
            <span className="status-pill-text">SearchMyComputer</span>
          </div>
        </div>
      </div>
    );
  }

  // Onboarding flow
  if (showOnboarding) {
    return <OnboardingWizard onFinish={handleFinishOnboarding} />;
  }

  // Collapsed Status Pill View
  if (!isExpanded) {
    return (
      <StatusPill
        visualState={visualState}
        statusLabel={statusLabel}
        onExpand={handleExpand}
        voice={voice}
        hotkey="Alt+Space"
        onTogglePause={handleTogglePause}
        onOpenSettings={() => {
          handleExpand();
          setIsSettingsOpen(true);
        }}
      />
    );
  }

  const folderCount = status?.indexed_folders.length ?? 0;
  const showResults = query.trim().length > 0 && totalItems > 0;
  const showEmpty = query.trim().length === 0 || totalItems === 0;

  return (
    <div className="launcher-window">
      <TrialBanner
        licenseStatus={licenseStatus}
        onOpenLicenseModal={() => setShowLicenseModal(true)}
      />
      <div className={`launcher-panel ${showPreviewPane ? "with-preview" : ""}`}>
        <div className="launcher-main">
          <SearchBar
            query={query}
            parsedQuery={parsedQuery}
            onChange={setQuery}
            onKeyDown={handleKeyDown}
            inputRef={inputRef}
            voice={voice}
          />

          <div className="content-area">
            {showResults && (
              <ResultsList
                results={results}
                projectResults={projectResults}
                selectedIndex={selectedIndex}
                onSelect={setSelectedIndex}
                onOpenFile={handleOpenFile}
                onOpenProject={handleOpenProject}
                onRevealProject={handleRevealProject}
                onOpenTerminalProject={handleOpenTerminalProject}
                onPreviewImage={(res) => setPreviewImage(res)}
              />
            )}

            {showEmpty && (
              <EmptyState
                onAddFolder={handleAddFolder}
                folderCount={folderCount}
                query={query}
                recentSearches={recentSearches}
                onSelectQuery={handleSelectRecentQuery}
                onRemoveRecentSearch={handleRemoveRecentSearch}
                onClearRecentSearches={handleClearRecentSearches}
                onOpenSettings={() => setIsSettingsOpen(true)}
                onOpenActions={() => setShowActionsMenu(true)}
              />
            )}
          </div>

          <StatusLine
            status={status}
            hotkeyWarning={status ? !status.hotkey_registered : false}
            onOpenSettings={() => setIsSettingsOpen(true)}
            onRefreshStatus={refreshStatus}
          />
        </div>

        {showPreviewPane && (
          <PreviewPane
            item={getPreviewPaneItem()}
            onOpenFile={handleOpenByPath}
            onRevealFile={handleRevealByPath}
            onClose={async () => {
              setShowPreviewPane(false);
              await invoke("expand_launcher_window", { previewOpen: false });
            }}
          />
        )}
      </div>

      {previewImage && (
        <ImagePreviewModal
          result={previewImage}
          onClose={() => setPreviewImage(null)}
          onOpenFile={handleOpenFile}
        />
      )}

      {isSettingsOpen && (
        <SettingsModal
          isOpen={isSettingsOpen}
          onClose={() => {
            setIsSettingsOpen(false);
            loadLicenseStatus();
          }}
          status={status}
          onRefreshStatus={refreshStatus}
        />
      )}

      {showLicenseModal && (
        <LicenseModal
          isOpen={showLicenseModal}
          onClose={() => setShowLicenseModal(false)}
          licenseStatus={licenseStatus}
          onLicenseUpdated={async () => {
            await loadLicenseStatus();
            await refreshStatus();
            setShowLicenseModal(false);
          }}
        />
      )}

      {showActionsMenu && (
        <ActionsMenu
          isOpen={showActionsMenu}
          onClose={() => {
            setShowActionsMenu(false);
            requestAnimationFrame(() => inputRef.current?.focus());
          }}
          selectedItem={getActionsSelectedItem()}
          onOpenFile={handleOpenByPath}
          onRevealFile={handleRevealByPath}
          onOpenTerminal={handleOpenTerminalByPath}
          onCopyPath={handleCopyPath}
          onOpenSettings={() => {
            setShowActionsMenu(false);
            setIsSettingsOpen(true);
          }}
          onTogglePause={handleTogglePause}
          indexingPaused={indexingPaused}
        />
      )}
    </div>
  );
};
