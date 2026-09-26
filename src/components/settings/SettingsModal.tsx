import React, { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  AppConfig,
  IndexStatus,
  ProblemFileRecord,
  ExclusionTestResult,
  DiagnosticsExportResult,
  GovernorStatus,
  LicenseStatus,
  UpdateCheckResult,
  PillPositionMode,
} from "../../types";
import { LicenseModal } from "../license/LicenseModal";

type TabId = "general" | "folders" | "filetypes" | "voice" | "performance" | "health" | "privacy";

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  status: IndexStatus | null;
  onRefreshStatus: () => Promise<void>;
}

const TABS: { id: TabId; label: string; icon: string }[] = [
  { id: "general", label: "General", icon: "⚙" },
  { id: "folders", label: "Folders", icon: "📁" },
  { id: "filetypes", label: "File Types", icon: "📄" },
  { id: "voice", label: "Voice", icon: "🎙" },
  { id: "performance", label: "Performance", icon: "⚡" },
  { id: "health", label: "Health", icon: "🩺" },
  { id: "privacy", label: "Privacy & About", icon: "🔒" },
];

export const SettingsModal: React.FC<SettingsModalProps> = ({
  isOpen,
  onClose,
  status,
  onRefreshStatus,
}) => {
  const [activeTab, setActiveTab] = useState<TabId>("general");
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{ text: string; type: "success" | "error" } | null>(null);
  const [dirty, setDirty] = useState(false);

  // Folders & Exclusions state
  const [newFolder, setNewFolder] = useState("");
  const [newExclusion, setNewExclusion] = useState("");
  const [exclusionTestPath, setExclusionTestPath] = useState("");
  const [exclusionTestPattern, setExclusionTestPattern] = useState("");
  const [exclusionTestResult, setExclusionTestResult] = useState<ExclusionTestResult | null>(null);
  const [testingExclusion, setTestingExclusion] = useState(false);

  // File Types state
  const [newImageFolder, setNewImageFolder] = useState("");

  // Health state
  const [problemFiles, setProblemFiles] = useState<ProblemFileRecord[]>([]);
  const [loadingProblems, setLoadingProblems] = useState(false);

  // Privacy & About state
  const [confirmAction, setConfirmAction] = useState<"delete" | "rebuild" | null>(null);
  const [exportResult, setExportResult] = useState<DiagnosticsExportResult | null>(null);
  const [actionInProgress, setActionInProgress] = useState(false);
  const [licenseStatus, setLicenseStatus] = useState<LicenseStatus | null>(null);
  const [showLicenseModal, setShowLicenseModal] = useState(false);
  const [updateResult, setUpdateResult] = useState<UpdateCheckResult | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);

  // Performance state
  const [governor, setGovernor] = useState<GovernorStatus | null>(null);

  const tabListRef = useRef<HTMLDivElement>(null);

  const loadLicenseStatus = useCallback(async () => {
    try {
      const s = await invoke<LicenseStatus>("get_license_status");
      setLicenseStatus(s);
    } catch (e) {
      console.error("Failed to load license status:", e);
    }
  }, []);

  // Load config and license on open
  useEffect(() => {
    if (!isOpen) return;
    setDirty(false);
    setMessage(null);
    setConfirmAction(null);
    setExportResult(null);
    setExclusionTestResult(null);
    setUpdateResult(null);

    invoke<AppConfig>("get_config")
      .then((cfg) => setConfig(cfg))
      .catch((err) => console.error("Failed to load config:", err));

    loadLicenseStatus();
  }, [isOpen, loadLicenseStatus]);

  // Load problem files when health tab is active
  useEffect(() => {
    if (activeTab !== "health" || !isOpen) return;
    setLoadingProblems(true);
    invoke<ProblemFileRecord[]>("get_problem_files")
      .then((files) => setProblemFiles(files))
      .catch((err) => console.error("Failed to load problem files:", err))
      .finally(() => setLoadingProblems(false));
  }, [activeTab, isOpen]);

  // Load governor status when performance tab is active
  useEffect(() => {
    if (activeTab !== "performance" || !isOpen) return;
    invoke<GovernorStatus>("get_governor_status")
      .then((g) => setGovernor(g))
      .catch(() => {}); // Governor may not be available
  }, [activeTab, isOpen]);

  const updateConfig = useCallback((patch: Partial<AppConfig>) => {
    setConfig((prev) => (prev ? { ...prev, ...patch } : null));
    setDirty(true);
  }, []);

  const showMessage = useCallback((text: string, type: "success" | "error") => {
    setMessage({ text, type });
    setTimeout(() => setMessage(null), 4000);
  }, []);

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    try {
      await invoke("update_config", { newConfig: config });
      if (config.wake_word_enabled !== undefined) {
        await invoke("set_wake_word_enabled", { enabled: config.wake_word_enabled }).catch(() => {});
      }
      if (config.launch_at_login) {
        await invoke("set_launch_at_login", { enable: true }).catch(() => {});
      }
      await onRefreshStatus();
      setDirty(false);
      showMessage("Settings saved.", "success");
    } catch (err) {
      showMessage(`Failed to save: ${err}`, "error");
    } finally {
      setSaving(false);
    }
  };

  // ── Folder management ──
  const handleAddFolder = () => {
    const trimmed = newFolder.trim().replace(/\\/g, "/");
    if (!trimmed || !config) return;
    if (!config.indexed_folders.includes(trimmed)) {
      updateConfig({ indexed_folders: [...config.indexed_folders, trimmed] });
    }
    setNewFolder("");
  };

  const handleRemoveFolder = (folder: string) => {
    if (!config) return;
    updateConfig({ indexed_folders: config.indexed_folders.filter((f) => f !== folder) });
  };

  // ── Exclusion management ──
  const handleAddExclusion = () => {
    const trimmed = newExclusion.trim();
    if (!trimmed || !config) return;
    if (!config.exclusions.includes(trimmed)) {
      updateConfig({ exclusions: [...config.exclusions, trimmed] });
    }
    setNewExclusion("");
  };

  const handleRemoveExclusion = (excl: string) => {
    if (!config) return;
    updateConfig({ exclusions: config.exclusions.filter((e) => e !== excl) });
  };

  const handleTestExclusion = async () => {
    if (!exclusionTestPattern.trim() || !exclusionTestPath.trim()) return;
    setTestingExclusion(true);
    try {
      const result = await invoke<ExclusionTestResult>("test_exclusion_pattern", {
        pattern: exclusionTestPattern.trim(),
        testPath: exclusionTestPath.trim(),
      });
      setExclusionTestResult(result);
    } catch (err) {
      setExclusionTestResult({ matches: false, error: String(err) });
    } finally {
      setTestingExclusion(false);
    }
  };

  // ── Image folder management ──
  const handleAddImageFolder = () => {
    const trimmed = newImageFolder.trim().replace(/\\/g, "/");
    if (!trimmed || !config) return;
    if (!config.image_folders.includes(trimmed)) {
      updateConfig({ image_folders: [...config.image_folders, trimmed] });
    }
    setNewImageFolder("");
  };

  const handleRemoveImageFolder = (folder: string) => {
    if (!config) return;
    updateConfig({ image_folders: config.image_folders.filter((f) => f !== folder) });
  };

  // ── Health actions ──
  const handleRetryProblemFiles = async () => {
    try {
      await invoke("retry_problem_files");
      showMessage("Retrying failed files...", "success");
      const files = await invoke<ProblemFileRecord[]>("get_problem_files");
      setProblemFiles(files);
    } catch (err) {
      showMessage(`Retry failed: ${err}`, "error");
    }
  };

  const handleClearProblemFiles = async () => {
    try {
      await invoke("clear_problem_files");
      setProblemFiles([]);
      showMessage("Problem file records cleared.", "success");
    } catch (err) {
      showMessage(`Clear failed: ${err}`, "error");
    }
  };

  // ── Privacy actions ──
  const handleDeleteAllData = async () => {
    setActionInProgress(true);
    try {
      await invoke("delete_all_data");
      setConfirmAction(null);
      showMessage("All index data has been deleted.", "success");
      await onRefreshStatus();
    } catch (err) {
      showMessage(`Delete failed: ${err}`, "error");
    } finally {
      setActionInProgress(false);
    }
  };

  const handleRebuildIndex = async () => {
    setActionInProgress(true);
    try {
      await invoke("rebuild_index");
      setConfirmAction(null);
      showMessage("Index rebuild started.", "success");
      await onRefreshStatus();
    } catch (err) {
      showMessage(`Rebuild failed: ${err}`, "error");
    } finally {
      setActionInProgress(false);
    }
  };

  const handleExportDiagnostics = async () => {
    setActionInProgress(true);
    try {
      const result = await invoke<DiagnosticsExportResult>("export_diagnostics");
      setExportResult(result);
      showMessage("Diagnostics exported.", "success");
    } catch (err) {
      showMessage(`Export failed: ${err}`, "error");
    } finally {
      setActionInProgress(false);
    }
  };

  const handleCheckUpdates = async () => {
    setCheckingUpdate(true);
    try {
      const result = await invoke<UpdateCheckResult>("check_for_updates");
      setUpdateResult(result);
    } catch (err) {
      showMessage(`Update check failed: ${err}`, "error");
    } finally {
      setCheckingUpdate(false);
    }
  };

  // ── Tab keyboard nav ──
  const handleTabKeyDown = (e: React.KeyboardEvent) => {
    const tabIds = TABS.map((t) => t.id);
    const idx = tabIds.indexOf(activeTab);
    if (e.key === "ArrowRight" || e.key === "ArrowDown") {
      e.preventDefault();
      const nextIdx = (idx + 1) % tabIds.length;
      setActiveTab(tabIds[nextIdx]);
    } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
      e.preventDefault();
      const prevIdx = (idx - 1 + tabIds.length) % tabIds.length;
      setActiveTab(tabIds[prevIdx]);
    } else if (e.key === "Home") {
      e.preventDefault();
      setActiveTab(tabIds[0]);
    } else if (e.key === "End") {
      e.preventDefault();
      setActiveTab(tabIds[tabIds.length - 1]);
    }
  };

  if (!isOpen || !config) return null;

  return (
    <div className="modal-overlay" onClick={onClose} role="dialog" aria-modal="true" aria-label="Settings">
      <div
        className="modal-dialog settings-dialog"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="modal-header">
          <h2 className="modal-title">Settings</h2>
          <button className="modal-close-btn" onClick={onClose} title="Close (Esc)" aria-label="Close Settings">
            ✕
          </button>
        </div>

        {/* Tab Bar */}
        <div
          className="settings-tab-bar"
          role="tablist"
          aria-label="Settings sections"
          ref={tabListRef}
          onKeyDown={handleTabKeyDown}
        >
          {TABS.map((tab) => (
            <button
              key={tab.id}
              role="tab"
              id={`tab-${tab.id}`}
              className={`settings-tab ${activeTab === tab.id ? "active" : ""}`}
              aria-selected={activeTab === tab.id}
              aria-controls={`panel-${tab.id}`}
              tabIndex={activeTab === tab.id ? 0 : -1}
              onClick={() => setActiveTab(tab.id)}
            >
              <span className="settings-tab-icon" aria-hidden="true">{tab.icon}</span>
              <span className="settings-tab-label">{tab.label}</span>
            </button>
          ))}
        </div>

        {/* Banner */}
        {message && (
          <div className={`settings-banner ${message.type}`} role="status">
            {message.text}
          </div>
        )}

        {/* Tab Panels */}
        <div className="settings-panel-area">
          {/* ────────────── GENERAL ────────────── */}
          {activeTab === "general" && (
            <div role="tabpanel" id="panel-general" aria-labelledby="tab-general" className="settings-panel">
              <div className="settings-section">
                <h3 className="section-heading">Hotkey</h3>
                <p className="section-desc">Global shortcut to toggle the search launcher.</p>
                <div className="settings-hotkey-display">
                  <kbd className="settings-hotkey-badge">{config.hotkey || "Alt+Space"}</kbd>
                  <span className="settings-hotkey-hint">Change in the OS keyboard settings or config file</span>
                </div>
              </div>

              <div className="settings-section">
                <h3 className="section-heading">Appearance</h3>
                <div className="settings-select-row">
                  <label htmlFor="theme-select" className="settings-label">Theme</label>
                  <select
                    id="theme-select"
                    className="settings-select"
                    value={config.theme}
                    onChange={(e) => updateConfig({ theme: e.target.value })}
                  >
                    <option value="dark">Dark</option>
                    <option value="light">Light</option>
                    <option value="system">System</option>
                  </select>
                </div>

                <div className="settings-select-row">
                  <label htmlFor="pill-position-select" className="settings-label">Status Pill Position</label>
                  <select
                    id="pill-position-select"
                    className="settings-select"
                    value={config.pill_position || "bottom-right"}
                    onChange={async (e) => {
                      const pos = e.target.value as PillPositionMode;
                      updateConfig({ pill_position: pos });
                      try {
                        await invoke("set_pill_position", { position: pos });
                      } catch (err) {
                        console.error("Failed to update pill position:", err);
                      }
                    }}
                  >
                    <option value="bottom-right">Bottom Right (Default)</option>
                    <option value="bottom-left">Bottom Left</option>
                    <option value="top-right">Top Right</option>
                    <option value="top-left">Top Left</option>
                    <option value="custom">Custom (Draggable)</option>
                  </select>
                </div>

                <div className="settings-select-row">
                  <label htmlFor="language-select" className="settings-label">Language</label>
                  <select
                    id="language-select"
                    className="settings-select"
                    value={config.language}
                    onChange={(e) => updateConfig({ language: e.target.value })}
                  >
                    <option value="en">English</option>
                    <option value="es">Español</option>
                    <option value="fr">Français</option>
                    <option value="de">Deutsch</option>
                    <option value="ja">日本語</option>
                    <option value="zh">中文</option>
                  </select>
                </div>
              </div>

              <div className="settings-section">
                <h3 className="section-heading">Startup</h3>
                <div className="setting-toggle-row">
                  <label className="toggle-switch">
                    <input
                      type="checkbox"
                      checked={config.launch_at_login}
                      onChange={(e) => updateConfig({ launch_at_login: e.target.checked })}
                    />
                    <span className="toggle-slider"></span>
                  </label>
                  <div className="toggle-label-block">
                    <span className="toggle-title">Launch at System Login</span>
                    <span className="toggle-sub">Start SearchMyComputer in the background when you sign in</span>
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* ────────────── FOLDERS & EXCLUSIONS ────────────── */}
          {activeTab === "folders" && (
            <div role="tabpanel" id="panel-folders" aria-labelledby="tab-folders" className="settings-panel">
              <div className="settings-section">
                <h3 className="section-heading">Indexed Folders</h3>
                <p className="section-desc">Folders to scan and index for search results.</p>

                <div className="folder-add-row">
                  <input
                    type="text"
                    className="folder-input"
                    placeholder="C:/Users/name/Documents"
                    value={newFolder}
                    onChange={(e) => setNewFolder(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && handleAddFolder()}
                    aria-label="Add folder path"
                  />
                  <button
                    className="add-folder-btn"
                    onClick={handleAddFolder}
                    disabled={!newFolder.trim()}
                  >
                    + Add
                  </button>
                </div>

                <div className="settings-folder-list">
                  {config.indexed_folders.length === 0 ? (
                    <span className="empty-folders-hint">No folders configured. Add folders above to start indexing.</span>
                  ) : (
                    config.indexed_folders.map((folder) => (
                      <div key={folder} className="folder-tag-item">
                        <span className="folder-tag-path" title={folder}>{folder}</span>
                        <button
                          className="folder-tag-remove"
                          onClick={() => handleRemoveFolder(folder)}
                          title="Remove folder"
                          aria-label={`Remove ${folder}`}
                        >
                          ✕
                        </button>
                      </div>
                    ))
                  )}
                </div>
              </div>

              <div className="settings-section">
                <h3 className="section-heading">Exclusion Patterns</h3>
                <p className="section-desc">
                  Glob patterns for files and folders to skip (e.g. <code>node_modules</code>, <code>*.tmp</code>, <code>.git</code>).
                </p>

                <div className="folder-add-row">
                  <input
                    type="text"
                    className="folder-input"
                    placeholder="*.log, node_modules, .cache"
                    value={newExclusion}
                    onChange={(e) => setNewExclusion(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && handleAddExclusion()}
                    aria-label="Add exclusion pattern"
                  />
                  <button
                    className="add-folder-btn"
                    onClick={handleAddExclusion}
                    disabled={!newExclusion.trim()}
                  >
                    + Add
                  </button>
                </div>

                <div className="settings-exclusion-chips">
                  {config.exclusions.map((excl) => (
                    <div key={excl} className="settings-excl-chip">
                      <code>{excl}</code>
                      <button
                        className="folder-tag-remove"
                        onClick={() => handleRemoveExclusion(excl)}
                        title="Remove exclusion"
                        aria-label={`Remove exclusion ${excl}`}
                      >
                        ✕
                      </button>
                    </div>
                  ))}
                </div>
              </div>

              <div className="settings-section">
                <h3 className="section-heading">Glob Tester</h3>
                <p className="section-desc">Test whether a glob pattern matches a file path.</p>

                <div className="settings-glob-tester">
                  <div className="glob-tester-row">
                    <label className="settings-label" htmlFor="glob-pattern">Pattern</label>
                    <input
                      id="glob-pattern"
                      type="text"
                      className="folder-input"
                      placeholder="**/*.log"
                      value={exclusionTestPattern}
                      onChange={(e) => setExclusionTestPattern(e.target.value)}
                    />
                  </div>
                  <div className="glob-tester-row">
                    <label className="settings-label" htmlFor="glob-test-path">Test Path</label>
                    <input
                      id="glob-test-path"
                      type="text"
                      className="folder-input"
                      placeholder="C:/Users/name/project/debug.log"
                      value={exclusionTestPath}
                      onChange={(e) => setExclusionTestPath(e.target.value)}
                      onKeyDown={(e) => e.key === "Enter" && handleTestExclusion()}
                    />
                  </div>
                  <button
                    className="secondary-btn"
                    onClick={handleTestExclusion}
                    disabled={testingExclusion || !exclusionTestPattern.trim() || !exclusionTestPath.trim()}
                  >
                    {testingExclusion ? "Testing..." : "Test Match"}
                  </button>

                  {exclusionTestResult && (
                    <div className={`glob-test-result ${exclusionTestResult.error ? "error" : exclusionTestResult.matches ? "match" : "no-match"}`}>
                      {exclusionTestResult.error
                        ? `Error: ${exclusionTestResult.error}`
                        : exclusionTestResult.matches
                          ? "✓ Pattern MATCHES — this path would be excluded"
                          : "✗ Pattern does NOT match — this path would be indexed"}
                    </div>
                  )}
                </div>
              </div>
            </div>
          )}

          {/* ────────────── FILE TYPES ────────────── */}
          {activeTab === "filetypes" && (
            <div role="tabpanel" id="panel-filetypes" aria-labelledby="tab-filetypes" className="settings-panel">
              <div className="settings-section">
                <h3 className="section-heading">Indexable Content Types</h3>
                <p className="section-desc">Choose which file categories to include in the search index.</p>

                <div className="settings-toggle-group">
                  <div className="setting-toggle-row">
                    <label className="toggle-switch">
                      <input
                        type="checkbox"
                        checked={config.index_documents}
                        onChange={(e) => updateConfig({ index_documents: e.target.checked })}
                      />
                      <span className="toggle-slider"></span>
                    </label>
                    <div className="toggle-label-block">
                      <span className="toggle-title">Documents</span>
                      <span className="toggle-sub">PDFs, Word, EPUB, Markdown, text files</span>
                    </div>
                  </div>

                  <div className="setting-toggle-row">
                    <label className="toggle-switch">
                      <input
                        type="checkbox"
                        checked={config.index_code}
                        onChange={(e) => updateConfig({ index_code: e.target.checked })}
                      />
                      <span className="toggle-slider"></span>
                    </label>
                    <div className="toggle-label-block">
                      <span className="toggle-title">Source Code</span>
                      <span className="toggle-sub">Rust, JS/TS, Python, Java, C/C++, and other code files</span>
                    </div>
                  </div>

                  <div className="setting-toggle-row">
                    <label className="toggle-switch">
                      <input
                        type="checkbox"
                        checked={config.index_spreadsheets}
                        onChange={(e) => updateConfig({ index_spreadsheets: e.target.checked })}
                      />
                      <span className="toggle-slider"></span>
                    </label>
                    <div className="toggle-label-block">
                      <span className="toggle-title">Spreadsheets</span>
                      <span className="toggle-sub">CSV, Excel (XLSX), and structured data files</span>
                    </div>
                  </div>

                  <div className="setting-toggle-row">
                    <label className="toggle-switch">
                      <input
                        type="checkbox"
                        checked={config.index_archives}
                        onChange={(e) => updateConfig({ index_archives: e.target.checked })}
                      />
                      <span className="toggle-slider"></span>
                    </label>
                    <div className="toggle-label-block">
                      <span className="toggle-title">Archives</span>
                      <span className="toggle-sub">ZIP, tar, and compressed archives (filename-only indexing)</span>
                    </div>
                  </div>
                </div>
              </div>

              <div className="settings-section">
                <h3 className="section-heading">Image & Multimodal Indexing</h3>
                <p className="section-desc">
                  Extract OCR text, scan QR codes, and generate visual embeddings.
                  Runs 100% locally on CPU at lowest priority.
                </p>

                <div className="setting-toggle-row">
                  <label className="toggle-switch">
                    <input
                      type="checkbox"
                      checked={config.enable_image_indexing}
                      onChange={(e) => updateConfig({ enable_image_indexing: e.target.checked })}
                    />
                    <span className="toggle-slider"></span>
                  </label>
                  <div className="toggle-label-block">
                    <span className="toggle-title">Enable Image Indexing</span>
                    <span className="toggle-sub">
                      {config.enable_image_indexing
                        ? "Active — images are processed for OCR & QR tags"
                        : "Disabled — images indexed by filename only"}
                    </span>
                  </div>
                </div>

                {config.enable_image_indexing && (
                  <div className="settings-subsection">
                    <h4 className="sub-heading">Restrict to Specific Folders</h4>
                    <p className="sub-desc">
                      Only process images in these folders. Leave empty to include all indexed folders.
                    </p>

                    <div className="folder-add-row">
                      <input
                        type="text"
                        className="folder-input"
                        placeholder="C:/Users/name/Pictures"
                        value={newImageFolder}
                        onChange={(e) => setNewImageFolder(e.target.value)}
                        onKeyDown={(e) => e.key === "Enter" && handleAddImageFolder()}
                        aria-label="Add image folder restriction"
                      />
                      <button
                        className="add-folder-btn"
                        onClick={handleAddImageFolder}
                        disabled={!newImageFolder.trim()}
                      >
                        + Add
                      </button>
                    </div>

                    <div className="folder-tags-list">
                      {config.image_folders.length === 0 ? (
                        <span className="empty-folders-hint">No restriction — all indexed folders enabled</span>
                      ) : (
                        config.image_folders.map((folder) => (
                          <div key={folder} className="folder-tag-item">
                            <span className="folder-tag-path" title={folder}>{folder}</span>
                            <button
                              className="folder-tag-remove"
                              onClick={() => handleRemoveImageFolder(folder)}
                              title="Remove restriction"
                              aria-label={`Remove image folder ${folder}`}
                            >
                              ✕
                            </button>
                          </div>
                        ))
                      )}
                    </div>
                  </div>
                )}
              </div>

              <div className="settings-section">
                <h3 className="section-heading">Max File Size</h3>
                <p className="section-desc">Files larger than this limit are indexed by filename only.</p>
                <div className="settings-range-row">
                  <input
                    type="range"
                    className="settings-range"
                    min={1}
                    max={256}
                    step={1}
                    value={config.max_file_size / (1024 * 1024)}
                    onChange={(e) => updateConfig({ max_file_size: Number(e.target.value) * 1024 * 1024 })}
                    aria-label="Max file size in MB"
                  />
                  <span className="settings-range-value">{(config.max_file_size / (1024 * 1024)).toFixed(0)} MB</span>
                </div>
              </div>
            </div>
          )}

          {/* ────────────── VOICE & WAKE-WORD ────────────── */}
          {activeTab === "voice" && (
            <div role="tabpanel" id="panel-voice" aria-labelledby="tab-voice" className="settings-panel">
              <div className="settings-section">
                <h3 className="section-heading">Always-On Wake Word Activation</h3>
                <p className="section-desc">
                  Allow SearchMyComputer to wake and start listening when you speak your chosen wake phrase.
                  Runs continuously offline on CPU using a low-overhead openWakeWord model (&lt;0.05% CPU).
                </p>

                <div className="setting-toggle-row">
                  <label className="toggle-switch">
                    <input
                      type="checkbox"
                      checked={config.wake_word_enabled ?? false}
                      onChange={(e) => updateConfig({ wake_word_enabled: e.target.checked })}
                    />
                    <span className="toggle-slider"></span>
                  </label>
                  <div className="toggle-label-block">
                    <span className="toggle-title">Enable Wake-Word Listening</span>
                    <span className="toggle-sub">
                      {config.wake_word_enabled
                        ? `Active — listening for "${config.wake_word_phrase || "Kira"}"`
                        : "Disabled — microphone is only activated when you click the mic button or press hotkey"}
                    </span>
                  </div>
                </div>

                {config.wake_word_enabled && (
                  <div className="settings-subsection">
                    <h4 className="sub-heading">Wake Word Phrase</h4>
                    <p className="sub-desc">Select which phrase activates voice search.</p>
                    <select
                      className="settings-select"
                      value={config.wake_word_phrase || "Kira"}
                      onChange={(e) => updateConfig({ wake_word_phrase: e.target.value })}
                      aria-label="Wake Word Phrase"
                    >
                      <option value="Kira">"Kira" (Default Custom Wake Word)</option>
                      <option value="Hey Jarvis">"Hey Jarvis" (Built-in Fallback)</option>
                      <option value="Alexa">"Alexa" (Built-in Fallback)</option>
                    </select>

                    <h4 className="sub-heading" style={{ marginTop: "1rem" }}>Detection Sensitivity</h4>
                    <p className="sub-desc">
                      Higher sensitivity triggers more easily; lower sensitivity prevents false activations.
                    </p>
                    <div className="settings-range-row">
                      <input
                        type="range"
                        className="settings-range"
                        min={0.2}
                        max={0.9}
                        step={0.05}
                        value={config.wake_word_threshold ?? 0.5}
                        onChange={(e) => updateConfig({ wake_word_threshold: parseFloat(e.target.value) })}
                        aria-label="Wake word threshold sensitivity"
                      />
                      <span className="settings-range-value">
                        {((config.wake_word_threshold ?? 0.5) * 100).toFixed(0)}%
                      </span>
                    </div>
                  </div>
                )}
              </div>

              <div className="settings-section">
                <h3 className="section-heading">Voice Capture & Speech-to-Text</h3>
                <p className="section-desc">
                  Voice is transcribed completely offline using an int8 ONNX Whisper model.
                </p>

                <h4 className="sub-heading">Silence Auto-Stop Timeout</h4>
                <p className="sub-desc">
                  Automatically stops recording after this many seconds of silence (default: 15s).
                </p>
                <div className="settings-range-row">
                  <input
                    type="range"
                    className="settings-range"
                    min={5}
                    max={30}
                    step={1}
                    value={config.voice_silence_timeout_secs ?? 15}
                    onChange={(e) => updateConfig({ voice_silence_timeout_secs: parseInt(e.target.value, 10) })}
                    aria-label="Voice silence timeout in seconds"
                  />
                  <span className="settings-range-value">
                    {config.voice_silence_timeout_secs ?? 15}s
                  </span>
                </div>

                <h4 className="sub-heading" style={{ marginTop: "1rem" }}>Maximum Recording Duration</h4>
                <p className="sub-desc">
                  Hard duration limit for a single speech recording to protect memory (default: 45s).
                </p>
                <div className="settings-range-row">
                  <input
                    type="range"
                    className="settings-range"
                    min={15}
                    max={60}
                    step={5}
                    value={config.voice_max_duration_secs ?? 45}
                    onChange={(e) => updateConfig({ voice_max_duration_secs: parseInt(e.target.value, 10) })}
                    aria-label="Maximum voice recording duration in seconds"
                  />
                  <span className="settings-range-value">
                    {config.voice_max_duration_secs ?? 45}s
                  </span>
                </div>
              </div>

              <div className="settings-section">
                <h3 className="section-heading">Privacy & Architecture Guarantees</h3>
                <div className="settings-info-card">
                  <p><strong>🔒 100% Local &amp; Zero Network Calls:</strong> All acoustic processing, wake-word classification, and Whisper transcription execute entirely on your local CPU.</p>
                  <p><strong>⚡ Memory Optimization:</strong> The Whisper speech model is lazy-loaded on demand and automatically unloaded from RAM after 5 minutes of idle time.</p>
                  <p><strong>🗑 Zero Disk Footprint:</strong> Microphone audio samples are captured into temporary volatile RAM and immediately discarded after transcription.</p>
                </div>
              </div>
            </div>
          )}

          {/* ────────────── PERFORMANCE ────────────── */}
          {activeTab === "performance" && (
            <div role="tabpanel" id="panel-performance" aria-labelledby="tab-performance" className="settings-panel">
              <div className="settings-section">
                <h3 className="section-heading">Indexing Threads</h3>
                <p className="section-desc">
                  Number of background threads for file parsing and embeddings (default: 2).
                </p>
                <div className="settings-range-row">
                  <input
                    type="range"
                    className="settings-range"
                    min={1}
                    max={8}
                    step={1}
                    value={config.index_threads}
                    onChange={(e) => updateConfig({ index_threads: Number(e.target.value) })}
                    aria-label="Number of indexing threads"
                  />
                  <span className="settings-range-value">{config.index_threads} threads</span>
                </div>
              </div>

              <div className="settings-section">
                <h3 className="section-heading">Memory Limit</h3>
                <p className="section-desc">
                  Max RAM the indexer may use for batch processing. Lower values reduce system impact.
                </p>
                <div className="settings-range-row">
                  <input
                    type="range"
                    className="settings-range"
                    min={64}
                    max={1024}
                    step={64}
                    value={config.max_ram_mb}
                    onChange={(e) => updateConfig({ max_ram_mb: Number(e.target.value) })}
                    aria-label="Max RAM in megabytes"
                  />
                  <span className="settings-range-value">{config.max_ram_mb} MB</span>
                </div>
              </div>

              <div className="settings-section">
                <h3 className="section-heading">Battery Policy</h3>
                <p className="section-desc">
                  How aggressively to index when running on battery power.
                </p>
                <div className="settings-select-row">
                  <label htmlFor="battery-select" className="settings-label">Policy</label>
                  <select
                    id="battery-select"
                    className="settings-select"
                    value={config.battery_policy}
                    onChange={(e) => updateConfig({ battery_policy: e.target.value })}
                  >
                    <option value="normal">Normal — Full speed regardless of power</option>
                    <option value="eco">Eco — Reduce threads on battery</option>
                    <option value="aggressive">Aggressive — Pause indexing on battery</option>
                  </select>
                </div>
              </div>

              {governor && (
                <div className="settings-section">
                  <h3 className="section-heading">Current System State</h3>
                  <div className="system-info-grid">
                    <div className="info-row">
                      <span className="info-label">Power Source</span>
                      <span className="info-val">{governor.is_on_ac ? "⚡ AC Power" : "🔋 Battery"}</span>
                    </div>
                    {governor.battery_percentage !== undefined && governor.battery_percentage !== null && (
                      <div className="info-row">
                        <span className="info-label">Battery Level</span>
                        <span className={`info-val ${governor.is_low_battery ? "settings-val-warn" : ""}`}>
                          {governor.battery_percentage}%
                        </span>
                      </div>
                    )}
                    <div className="info-row">
                      <span className="info-label">Governor State</span>
                      <span className="info-val">{governor.state}</span>
                    </div>
                    <div className="info-row">
                      <span className="info-label">Active Threads</span>
                      <span className="info-val">{governor.active_threads}</span>
                    </div>
                    {governor.is_thermal_throttled && (
                      <div className="info-row">
                        <span className="info-label">Thermal</span>
                        <span className="info-val settings-val-warn">⚠ Throttled</span>
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>
          )}

          {/* ────────────── HEALTH ────────────── */}
          {activeTab === "health" && (
            <div role="tabpanel" id="panel-health" aria-labelledby="tab-health" className="settings-panel">
              <div className="settings-section">
                <h3 className="section-heading">Problem Files</h3>
                <p className="section-desc">
                  Files that failed during indexing due to parsing errors, corruption, or unsupported formats.
                </p>

                {loadingProblems ? (
                  <div className="settings-loading">
                    <div className="spinner small"></div>
                    <span>Loading problem files...</span>
                  </div>
                ) : problemFiles.length === 0 ? (
                  <div className="settings-empty-state">
                    <span className="settings-empty-icon" aria-hidden="true">✓</span>
                    <span className="settings-empty-text">No problem files — everything indexed cleanly.</span>
                  </div>
                ) : (
                  <>
                    <div className="settings-problem-count">
                      {problemFiles.length} file{problemFiles.length !== 1 ? "s" : ""} with errors
                    </div>

                    <div className="settings-problem-list" role="list">
                      {problemFiles.map((pf) => (
                        <div key={pf.id} className="settings-problem-item" role="listitem">
                          <div className="settings-problem-path" title={pf.path}>{pf.path}</div>
                          <div className="settings-problem-details">
                            <span className="settings-problem-kind">{pf.error_kind}</span>
                            <span className="settings-problem-msg">{pf.error_message}</span>
                            <span className="settings-problem-meta">
                              {pf.attempts} attempt{pf.attempts !== 1 ? "s" : ""} · Last failed {new Date(pf.last_failed_at).toLocaleString()}
                            </span>
                          </div>
                        </div>
                      ))}
                    </div>

                    <div className="settings-problem-actions">
                      <button className="secondary-btn" onClick={handleRetryProblemFiles}>
                        🔄 Retry All
                      </button>
                      <button className="secondary-btn" onClick={handleClearProblemFiles}>
                        🗑 Clear Records
                      </button>
                    </div>
                  </>
                )}
              </div>

              {status && (
                <div className="settings-section">
                  <h3 className="section-heading">Index Health</h3>
                  <div className="system-info-grid">
                    <div className="info-row">
                      <span className="info-label">Total Files</span>
                      <span className="info-val">{status.total_files.toLocaleString()}</span>
                    </div>
                    <div className="info-row">
                      <span className="info-label">Active Files</span>
                      <span className="info-val">{status.active_files.toLocaleString()}</span>
                    </div>
                    <div className="info-row">
                      <span className="info-label">Scanning</span>
                      <span className="info-val">{status.is_scanning ? "In Progress" : "Idle"}</span>
                    </div>
                    <div className="info-row">
                      <span className="info-label">Pending Jobs</span>
                      <span className="info-val">{status.job_counts.pending.toLocaleString()}</span>
                    </div>
                  </div>
                </div>
              )}
            </div>
          )}

          {/* ────────────── PRIVACY & ABOUT ────────────── */}
          {activeTab === "privacy" && (
            <div role="tabpanel" id="panel-privacy" aria-labelledby="tab-privacy" className="settings-panel">
              <div className="settings-section">
                <div className="settings-offline-badge" aria-label="Offline-only application">
                  <span className="settings-offline-dot"></span>
                  <span className="settings-offline-text">100% Offline — No data ever leaves your device</span>
                </div>
              </div>

              <div className="settings-section">
                <h3 className="section-heading">Data Management</h3>
                <p className="section-desc">
                  All search data is stored locally. These actions affect only the local index database.
                </p>

                <div className="settings-destructive-actions">
                  {/* Delete All Data */}
                  {confirmAction === "delete" ? (
                    <div className="settings-confirm-box destructive">
                      <p className="settings-confirm-text">
                        ⚠ This will permanently delete the entire search index, all embeddings, and cached metadata.
                        Your actual files are NOT affected. Are you sure?
                      </p>
                      <div className="settings-confirm-buttons">
                        <button
                          className="settings-btn-destructive"
                          onClick={handleDeleteAllData}
                          disabled={actionInProgress}
                        >
                          {actionInProgress ? "Deleting..." : "Yes, Delete Everything"}
                        </button>
                        <button className="secondary-btn" onClick={() => setConfirmAction(null)}>
                          Cancel
                        </button>
                      </div>
                    </div>
                  ) : (
                    <button
                      className="settings-btn-destructive-outline"
                      onClick={() => setConfirmAction("delete")}
                    >
                      🗑 Delete All Index Data
                    </button>
                  )}

                  {/* Rebuild Index */}
                  {confirmAction === "rebuild" ? (
                    <div className="settings-confirm-box">
                      <p className="settings-confirm-text">
                        This will clear the current index and re-scan all configured folders from scratch.
                        It may take a while depending on the number of files.
                      </p>
                      <div className="settings-confirm-buttons">
                        <button
                          className="primary-btn"
                          onClick={handleRebuildIndex}
                          disabled={actionInProgress}
                        >
                          {actionInProgress ? "Rebuilding..." : "Confirm Rebuild"}
                        </button>
                        <button className="secondary-btn" onClick={() => setConfirmAction(null)}>
                          Cancel
                        </button>
                      </div>
                    </div>
                  ) : (
                    <button
                      className="secondary-btn settings-action-btn"
                      onClick={() => setConfirmAction("rebuild")}
                    >
                      🔄 Rebuild Index From Scratch
                    </button>
                  )}

                  {/* Export Diagnostics */}
                  <button
                    className="secondary-btn settings-action-btn"
                    onClick={handleExportDiagnostics}
                    disabled={actionInProgress}
                  >
                    {actionInProgress && !confirmAction ? "Exporting..." : "📦 Export Diagnostics (Anonymized)"}
                  </button>

                  {exportResult && (
                    <div className="settings-export-result">
                      <span className="settings-export-path" title={exportResult.file_path}>
                        Saved to: {exportResult.file_path}
                      </span>
                      <span className="settings-export-size">
                        {(exportResult.size_bytes / 1024).toFixed(1)} KB
                      </span>
                      {exportResult.summary && (
                        <span className="settings-export-summary">{exportResult.summary}</span>
                      )}
                    </div>
                  )}
                </div>
              </div>

              {/* License & Activation */}
              <div className="settings-section">
                <h3 className="section-heading">License & Activation</h3>
                <p className="section-desc">
                  SearchMyComputer uses offline cryptographic licenses. No internet connection is ever required.
                </p>

                <div className="license-status-card" style={{ marginBottom: "12px" }}>
                  <div className="license-status-header">
                    <span className="license-status-label">Current Status</span>
                    {licenseStatus?.status === "Licensed" ? (
                      <span className="license-badge licensed">✓ Licensed</span>
                    ) : licenseStatus?.status === "Trial" ? (
                      <span className="license-badge trial">
                        {licenseStatus.days_remaining} Days Remaining
                      </span>
                    ) : (
                      <span className="license-badge expired">⚠ Trial Expired</span>
                    )}
                  </div>

                  {licenseStatus?.status === "Licensed" ? (
                    <div className="license-details-grid">
                      <div className="license-detail-row">
                        <span className="license-detail-label">Licensed to:</span>
                        <span className="license-detail-value">
                          {licenseStatus.customer_name}
                        </span>
                      </div>
                      <div className="license-detail-row">
                        <span className="license-detail-label">License ID:</span>
                        <span className="license-detail-value code">
                          {licenseStatus.license_id}
                        </span>
                      </div>
                    </div>
                  ) : licenseStatus?.status === "TrialExpired" ? (
                    <p style={{ margin: 0, fontSize: "0.82rem", color: "#fca5a5" }}>
                      Your 14-day evaluation period has ended. Search results are restricted to the top 3 matches.
                    </p>
                  ) : (
                    <p style={{ margin: 0, fontSize: "0.82rem", color: "var(--text-secondary)" }}>
                      You are using the 14-day full-featured evaluation trial.
                    </p>
                  )}
                </div>

                <button
                  className="secondary-btn settings-action-btn"
                  onClick={() => setShowLicenseModal(true)}
                >
                  🔑 {licenseStatus?.status === "Licensed" ? "Manage License" : "Enter License Key"}
                </button>
              </div>

              {/* Updates */}
              <div className="settings-section">
                <h3 className="section-heading">Updates & Releases</h3>
                <p className="section-desc">
                  SearchMyComputer operates 100% locally with zero background telemetry or auto-updater pings. Check releases manually below.
                </p>

                <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
                    <button
                      className="secondary-btn settings-action-btn"
                      onClick={handleCheckUpdates}
                      disabled={checkingUpdate}
                    >
                      {checkingUpdate ? "Checking..." : "🔍 Check for Updates"}
                    </button>
                    {updateResult && (
                      <button
                        className="secondary-btn settings-action-btn"
                        onClick={() => openPath(updateResult.release_url)}
                      >
                        🌐 Open GitHub Releases
                      </button>
                    )}
                  </div>

                  {updateResult && (
                    <div className="settings-export-result">
                      <span className="settings-export-path">
                        Current Version: <strong>v{updateResult.current_version}</strong>
                      </span>
                      <span className="settings-export-summary">
                        Releases are published at: {updateResult.release_url}
                      </span>
                    </div>
                  )}
                </div>
              </div>

              <div className="settings-section">
                <h3 className="section-heading">System & Models</h3>
                <div className="system-info-grid">
                  <div className="info-row">
                    <span className="info-label">App Version</span>
                    <span className="info-val">v1.0.0</span>
                  </div>
                  <div className="info-row">
                    <span className="info-label">Hotkey Registered</span>
                    <span className="info-val">{status?.hotkey_registered ? "✓ Active" : "✗ Inactive"}</span>
                  </div>
                  <div className="info-row">
                    <span className="info-label">Text Embedder</span>
                    <span className="info-val">
                      {status?.has_embedding_model
                        ? status.embedding_model_id || "Active"
                        : "Not installed"}
                    </span>
                  </div>
                  <div className="info-row">
                    <span className="info-label">Visual CLIP Model</span>
                    <span className="info-val">
                      {status?.has_vision_models
                        ? "clip-vit-b32 (MIT)"
                        : "Not installed"}
                    </span>
                  </div>
                  <div className="info-row">
                    <span className="info-label">Image Indexing</span>
                    <span className="info-val">
                      {status?.enable_image_indexing ? "Enabled" : "Disabled"}
                    </span>
                  </div>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* License Modal sub-dialog */}
        <LicenseModal
          isOpen={showLicenseModal}
          onClose={() => setShowLicenseModal(false)}
          licenseStatus={licenseStatus}
          onLicenseUpdated={async () => {
            await loadLicenseStatus();
            await onRefreshStatus();
            setShowLicenseModal(false);
            showMessage("License verified and activated!", "success");
          }}
        />

        {/* Footer */}
        <div className="modal-footer">
          {dirty && <span className="settings-unsaved-dot" title="Unsaved changes" aria-label="You have unsaved changes"></span>}
          <button className="secondary-btn" onClick={onClose}>
            {dirty ? "Discard" : "Close"}
          </button>
          <button
            className="primary-btn"
            onClick={handleSave}
            disabled={saving || !dirty}
          >
            {saving ? "Saving..." : "Save Changes"}
          </button>
        </div>
      </div>
    </div>
  );
};
