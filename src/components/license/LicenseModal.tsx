import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { LicenseStatus, ImportLicenseResult } from "../../types";
import "./License.css";

interface LicenseModalProps {
  isOpen: boolean;
  onClose: () => void;
  licenseStatus: LicenseStatus | null;
  onLicenseUpdated: () => Promise<void>;
}

export const LicenseModal: React.FC<LicenseModalProps> = ({
  isOpen,
  onClose,
  licenseStatus,
  onLicenseUpdated,
}) => {
  const [licenseInput, setLicenseInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);
  const [isDragOver, setIsDragOver] = useState(false);

  useEffect(() => {
    if (isOpen) {
      setLicenseInput("");
      setErrorMsg(null);
      setSuccessMsg(null);
      setLoading(false);
    }
  }, [isOpen]);

  if (!isOpen) return null;

  const handleActivate = async (dataOrPath: string) => {
    const trimmed = dataOrPath.trim();
    if (!trimmed) {
      setErrorMsg("Please enter a license key or select a .lic file.");
      return;
    }

    setLoading(true);
    setErrorMsg(null);
    setSuccessMsg(null);

    try {
      const res = await invoke<ImportLicenseResult>("import_license", {
        licenseDataOrPath: trimmed,
      });

      if (res.success) {
        setSuccessMsg(res.message || "License activated successfully!");
        await onLicenseUpdated();
      } else {
        setErrorMsg(res.message || "Failed to verify license.");
      }
    } catch (e: any) {
      setErrorMsg(typeof e === "string" ? e : e?.message || "Invalid license format or signature.");
    } finally {
      setLoading(false);
    }
  };

  const handleFileDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragOver(false);

    const file = e.dataTransfer.files[0];
    if (file) {
      const reader = new FileReader();
      reader.onload = (event) => {
        const text = event.target?.result as string;
        if (text) {
          setLicenseInput(text);
          handleActivate(text);
        }
      };
      reader.readAsText(file);
    }
  };

  const handleFileSelect = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) {
      const reader = new FileReader();
      reader.onload = (event) => {
        const text = event.target?.result as string;
        if (text) {
          setLicenseInput(text);
          handleActivate(text);
        }
      };
      reader.readAsText(file);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose} role="dialog" aria-modal="true">
      <div
        className="modal-panel license-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-header">
          <div className="modal-title-group">
            <span className="modal-icon">🔑</span>
            <h2 className="modal-title">License & Activation</h2>
          </div>
          <button className="modal-close-btn" onClick={onClose} aria-label="Close">
            ✕
          </button>
        </div>

        <div className="modal-body">
          {/* Current Status Card */}
          <div className="license-status-card">
            <div className="license-status-header">
              <span className="license-status-label">Current Status</span>
              {licenseStatus?.status === "Licensed" ? (
                <span className="license-badge licensed">✓ Licensed</span>
              ) : licenseStatus?.status === "Trial" ? (
                <span className="license-badge trial">
                  {licenseStatus.days_remaining} Days Left
                </span>
              ) : (
                <span className="license-badge expired">⚠ Trial Expired</span>
              )}
            </div>

            {licenseStatus?.status === "Licensed" && (
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
                <div className="license-detail-row">
                  <span className="license-detail-label">Validation:</span>
                  <span className="license-detail-value">
                    100% Offline (Ed25519 Cryptographic Signature)
                  </span>
                </div>
              </div>
            )}
          </div>

          {/* Form */}
          <div className="license-input-section">
            <label className="license-input-label" htmlFor="license-key-input">
              {licenseStatus?.status === "Licensed"
                ? "Replace License Key or File"
                : "Enter License Key or JSON"}
            </label>

            <textarea
              id="license-key-input"
              className="license-textarea"
              rows={4}
              placeholder='Paste license file contents (JSON format: {"customer_name": "...", "signature": "..."}) or file path...'
              value={licenseInput}
              onChange={(e) => setLicenseInput(e.target.value)}
              disabled={loading}
            />

            {/* Drop Zone */}
            <div
              className={`license-dropzone ${isDragOver ? "dragover" : ""}`}
              onDragOver={(e) => {
                e.preventDefault();
                setIsDragOver(true);
              }}
              onDragLeave={() => setIsDragOver(false)}
              onDrop={handleFileDrop}
            >
              <span className="dropzone-icon">📁</span>
              <span className="dropzone-text">
                Drag and drop your <code>.lic</code> file here, or{" "}
                <label className="dropzone-browse-link">
                  browse
                  <input
                    type="file"
                    accept=".lic,.json,text/plain"
                    className="hidden-file-input"
                    onChange={handleFileSelect}
                  />
                </label>
              </span>
            </div>

            {errorMsg && (
              <div className="license-alert error" role="alert">
                <span className="alert-icon">⚠</span>
                <span>{errorMsg}</span>
              </div>
            )}

            {successMsg && (
              <div className="license-alert success" role="alert">
                <span className="alert-icon">✓</span>
                <span>{successMsg}</span>
              </div>
            )}
          </div>

          {/* Offline Assurance */}
          <div className="license-offline-note">
            <span className="offline-dot"></span>
            <span>
              SearchMyComputer uses offline Ed25519 cryptographic signatures. No
              network connection or remote license server is ever contacted.
            </span>
          </div>
        </div>

        <div className="modal-footer">
          <button className="secondary-btn" onClick={onClose} disabled={loading}>
            Close
          </button>
          <button
            className="primary-btn"
            onClick={() => handleActivate(licenseInput)}
            disabled={loading || !licenseInput.trim()}
          >
            {loading ? "Verifying..." : "Activate License"}
          </button>
        </div>
      </div>
    </div>
  );
};
