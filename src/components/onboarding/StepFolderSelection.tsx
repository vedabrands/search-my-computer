import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DetectedFolder } from "../../types";

interface StepFolderSelectionProps {
  onNext: (selectedFolders: string[], imageIndexing: boolean, launchAtLogin: boolean) => void;
  onBack: () => void;
}

export const StepFolderSelection: React.FC<StepFolderSelectionProps> = ({ onNext, onBack }) => {
  const [detectedFolders, setDetectedFolders] = useState<DetectedFolder[]>([]);
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [enableImageIndexing, setEnableImageIndexing] = useState(false);
  const [launchAtLogin, setLaunchAtLogin] = useState(true);
  const [customPath, setCustomPath] = useState("");
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let mounted = true;
    invoke<DetectedFolder[]>("detect_system_folders")
      .then((folders) => {
        if (!mounted) return;
        setDetectedFolders(folders);
        const defaults = new Set<string>();
        folders.forEach((f) => {
          if (f.default_checked && f.exists) {
            defaults.add(f.path);
          }
        });
        setSelectedPaths(defaults);
      })
      .catch((e) => console.error("Failed to detect system folders:", e))
      .finally(() => {
        if (mounted) setLoading(false);
      });

    return () => {
      mounted = false;
    };
  }, []);

  const handleToggleFolder = (path: string) => {
    const next = new Set(selectedPaths);
    if (next.has(path)) {
      next.delete(path);
    } else {
      next.add(path);
    }
    setSelectedPaths(next);
  };

  const handleAddCustomFolder = () => {
    const trimmed = customPath.trim().replace(/\\/g, "/");
    if (!trimmed) return;

    if (!selectedPaths.has(trimmed)) {
      const next = new Set(selectedPaths);
      next.add(trimmed);
      setSelectedPaths(next);

      // Add to detected folders list if not present
      if (!detectedFolders.some((f) => f.path.toLowerCase() === trimmed.toLowerCase())) {
        const parts = trimmed.split("/").filter(Boolean);
        const name = parts[parts.length - 1] || trimmed;
        setDetectedFolders((prev) => [
          ...prev,
          {
            name: `Custom (${name})`,
            path: trimmed,
            category: "custom",
            is_sensitive: false,
            exists: true,
            default_checked: true,
          },
        ]);
      }
    }
    setCustomPath("");
  };

  const handleContinue = () => {
    onNext(Array.from(selectedPaths), enableImageIndexing, launchAtLogin);
  };

  return (
    <div className="onboarding-step step-folder-selection" role="region" aria-labelledby="folder-title">
      <h2 id="folder-title" className="step-title">
        Choose What to Index
      </h2>
      <p className="step-subtitle">
        Select the folders you want to search. You can change these anytime in Settings.
      </p>

      {loading ? (
        <div className="loading-spinner-box">
          <div className="spinner"></div>
          <span>Discovering local directories...</span>
        </div>
      ) : (
        <div className="folder-selection-container">
          <div className="folder-checklist" role="group" aria-label="Detected Folders List">
            {detectedFolders.map((folder) => {
              const isChecked = selectedPaths.has(folder.path);
              return (
                <label
                  key={folder.path}
                  className={`folder-item-card ${isChecked ? "checked" : ""} ${folder.is_sensitive ? "sensitive" : ""}`}
                >
                  <input
                    type="checkbox"
                    checked={isChecked}
                    onChange={() => handleToggleFolder(folder.path)}
                    aria-label={`Index ${folder.name} folder`}
                  />
                  <div className="folder-meta">
                    <div className="folder-header-row">
                      <span className="folder-name">{folder.name}</span>
                      {folder.is_sensitive && (
                        <span className="badge badge-warning" title="Contains photos or media">Media</span>
                      )}
                      {folder.category === "project" && (
                        <span className="badge badge-accent">Code & Projects</span>
                      )}
                    </div>
                    <span className="folder-path" title={folder.path}>{folder.path}</span>
                  </div>
                </label>
              );
            })}
          </div>

          <div className="custom-folder-row">
            <input
              type="text"
              placeholder="Or enter custom folder path (e.g. C:/Work)..."
              value={customPath}
              onChange={(e) => setCustomPath(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  handleAddCustomFolder();
                }
              }}
              className="custom-path-input"
              aria-label="Add custom folder path"
            />
            <button
              type="button"
              className="btn-secondary"
              onClick={handleAddCustomFolder}
              disabled={!customPath.trim()}
            >
              + Add
            </button>
          </div>

          <div className="onboarding-options-section">
            <h3 className="options-heading">Optional Enhancements</h3>

            <label className="option-toggle-item">
              <input
                type="checkbox"
                checked={enableImageIndexing}
                onChange={(e) => setEnableImageIndexing(e.target.checked)}
              />
              <div className="option-toggle-content">
                <span className="option-title">Image & Multimodal Indexing</span>
                <span className="option-desc">
                  Extract OCR text and scan QR codes in images locally using CPU.
                </span>
              </div>
            </label>

            <label className="option-toggle-item">
              <input
                type="checkbox"
                checked={launchAtLogin}
                onChange={(e) => setLaunchAtLogin(e.target.checked)}
              />
              <div className="option-toggle-content">
                <span className="option-title">Launch at System Login</span>
                <span className="option-desc">
                  Start SearchMyComputer in the background when you turn on your PC.
                </span>
              </div>
            </label>
          </div>
        </div>
      )}

      <div className="onboarding-actions split">
        <button type="button" className="btn-secondary" onClick={onBack}>
          ← Back
        </button>
        <button
          type="button"
          className="btn-primary"
          onClick={handleContinue}
          disabled={selectedPaths.size === 0}
        >
          Confirm & Start Indexing ({selectedPaths.size} folders) →
        </button>
      </div>
    </div>
  );
};
