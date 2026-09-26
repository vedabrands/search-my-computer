import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AppConfig, IndexStatus } from "./types";

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  status: IndexStatus | null;
  onRefreshStatus: () => Promise<void>;
}

export const SettingsModal: React.FC<SettingsModalProps> = ({
  isOpen,
  onClose,
  status,
  onRefreshStatus,
}) => {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [newImageFolder, setNewImageFolder] = useState("");
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    if (isOpen) {
      invoke<AppConfig>("get_config")
        .then((cfg) => setConfig(cfg))
        .catch((err) => console.error("failed to get config:", err));
    }
  }, [isOpen]);

  if (!isOpen || !config) return null;

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    setMessage(null);
    try {
      await invoke("update_config", { newConfig: config });
      await onRefreshStatus();
      setMessage("Settings saved successfully.");
      setTimeout(() => setMessage(null), 3000);
    } catch (err) {
      setMessage(`Failed to save: ${err}`);
    } finally {
      setSaving(false);
    }
  };

  const handleToggleImageIndexing = () => {
    setConfig((prev) =>
      prev ? { ...prev, enable_image_indexing: !prev.enable_image_indexing } : null
    );
  };

  const handleAddImageFolder = () => {
    const trimmed = newImageFolder.trim().replace(/\\/g, "/");
    if (!trimmed || !config) return;

    if (!config.image_folders.includes(trimmed)) {
      setConfig({
        ...config,
        image_folders: [...config.image_folders, trimmed],
      });
    }
    setNewImageFolder("");
  };

  const handleRemoveImageFolder = (folder: string) => {
    if (!config) return;
    setConfig({
      ...config,
      image_folders: config.image_folders.filter((f) => f !== folder),
    });
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">Preferences & Indexing</h2>
          <button className="modal-close-btn" onClick={onClose} title="Close">
            ✕
          </button>
        </div>

        <div className="modal-body">
          {message && <div className="modal-message-banner">{message}</div>}

          <div className="settings-section">
            <h3 className="section-heading">Image & Multimodal Indexing</h3>
            <p className="section-desc">
              Extract OCR text, scan QR codes (payment, links, Wi-Fi), and generate visual embeddings.
              Runs 100% locally on CPU at lowest job priority.
            </p>

            <div className="setting-toggle-row">
              <label className="toggle-switch">
                <input
                  type="checkbox"
                  checked={config.enable_image_indexing}
                  onChange={handleToggleImageIndexing}
                />
                <span className="toggle-slider"></span>
              </label>
              <div className="toggle-label-block">
                <span className="toggle-title">Enable Image Indexing</span>
                <span className="toggle-sub">
                  {config.enable_image_indexing
                    ? "Active — Images in indexed folders will be processed for OCR & QR tags"
                    : "Disabled — Image files are only indexed by filename"}
                </span>
              </div>
            </div>

            {config.enable_image_indexing && (
              <div className="image-folder-restrictions">
                <h4 className="sub-heading">Restricted Image Folders</h4>
                <p className="sub-desc">
                  Limit image indexing to specific directories (e.g., Screenshots, Photos). If empty,
                  all indexed folders are included.
                </p>

                <div className="folder-add-row">
                  <input
                    type="text"
                    className="folder-input"
                    placeholder="C:/Users/name/Pictures/Screenshots"
                    value={newImageFolder}
                    onChange={(e) => setNewImageFolder(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && handleAddImageFolder()}
                  />
                  <button className="add-folder-btn" onClick={handleAddImageFolder}>
                    Add
                  </button>
                </div>

                <div className="folder-tags-list">
                  {config.image_folders.length === 0 ? (
                    <span className="empty-folders-hint">
                      No specific folder restriction (all indexed folders enabled)
                    </span>
                  ) : (
                    config.image_folders.map((folder) => (
                      <div key={folder} className="folder-tag-item">
                        <span className="folder-tag-path">{folder}</span>
                        <button
                          className="folder-tag-remove"
                          onClick={() => handleRemoveImageFolder(folder)}
                          title="Remove restriction"
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
            <h3 className="section-heading">Indexed Folders</h3>
            <p className="section-desc">Folders currently being indexed for search:</p>
            <div className="folder-tags-list">
              {config.indexed_folders.map((f) => (
                <div key={f} className="folder-tag-item">
                  <span className="folder-tag-path">{f}</span>
                </div>
              ))}
            </div>
          </div>

          <div className="settings-section">
            <h3 className="section-heading">System & Models</h3>
            <div className="system-info-grid">
              <div className="info-row">
                <span className="info-label">Hotkey</span>
                <span className="info-val">{config.hotkey}</span>
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
                    : "Not installed (Metadata, OCR & QR still active)"}
                </span>
              </div>
            </div>
          </div>
        </div>

        <div className="modal-footer">
          <button className="secondary-btn" onClick={onClose}>
            Cancel
          </button>
          <button className="primary-btn" onClick={handleSave} disabled={saving}>
            {saving ? "Saving..." : "Save Preferences"}
          </button>
        </div>
      </div>
    </div>
  );
};
