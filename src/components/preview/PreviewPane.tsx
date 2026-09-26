import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SearchResult, ProjectRecord, FilePreviewResponse, ImageDetailsResponse } from "../../types";
import { getFileIcon } from "../../fileIcons";

interface PreviewPaneProps {
  item: { type: "file"; result: SearchResult } | { type: "project"; project: ProjectRecord } | null;
  onOpenFile: (path: string) => void;
  onRevealFile: (path: string) => void;
  onClose: () => void;
}

export const PreviewPane: React.FC<PreviewPaneProps> = ({
  item,
  onOpenFile,
  onRevealFile,
  onClose,
}) => {
  const [filePreview, setFilePreview] = useState<FilePreviewResponse | null>(null);
  const [imageDetails, setImageDetails] = useState<ImageDetailsResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!item) {
      setFilePreview(null);
      setImageDetails(null);
      return;
    }

    setLoading(true);
    setCopied(false);

    if (item.type === "project") {
      setFilePreview(null);
      setImageDetails(null);
      setLoading(false);
      return;
    }

    const { result } = item;
    const isImage = result.kind === "image" || ["png", "jpg", "jpeg", "webp", "bmp"].includes(result.ext.toLowerCase());

    // Fetch file preview text
    invoke<FilePreviewResponse | null>("get_file_preview", {
      fileId: result.id > 0 ? result.id : null,
      path: result.path,
    })
      .then((res) => setFilePreview(res))
      .catch((err) => console.error("Failed to load file preview:", err))
      .finally(() => setLoading(false));

    // If image, fetch image tags & details
    if (isImage && result.id > 0) {
      invoke<ImageDetailsResponse>("get_image_details", { fileId: result.id })
        .then((details) => setImageDetails(details))
        .catch((e) => console.error("Failed to get image details:", e));
    } else {
      setImageDetails(null);
    }
  }, [item]);

  if (!item) return null;

  const handleCopyPath = async () => {
    const p = item.type === "project" ? item.project.path : item.result.path;
    try {
      await navigator.clipboard.writeText(p);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      console.error("Failed to copy path:", e);
    }
  };

  const isProject = item.type === "project";
  const title = isProject ? item.project.name : item.result.name;
  const path = isProject ? item.project.path : item.result.path;
  const kind = isProject ? `Project (${item.project.project_type})` : item.result.kind.toUpperCase();
  const ext = isProject ? "folder" : item.result.ext;

  return (
    <aside className="preview-pane" aria-label="File Details and Content Preview">
      {/* Preview Header */}
      <div className="preview-header">
        <div className="preview-title-group">
          <span className="preview-type-icon">{getFileIcon(ext, kind)}</span>
          <div className="preview-title-meta">
            <h2 className="preview-title" title={title}>{title}</h2>
            <div className="preview-meta-badges">
              <span className="meta-badge kind">{kind}</span>
              {!isProject && item.result.size > 0 && (
                <span className="meta-badge size">
                  {(item.result.size / 1024).toFixed(1)} KB
                </span>
              )}
              {filePreview?.line_count !== undefined && filePreview.line_count > 0 && (
                <span className="meta-badge lines">{filePreview.line_count} lines</span>
              )}
            </div>
          </div>
        </div>

        <button
          type="button"
          className="preview-close-btn"
          onClick={onClose}
          title="Close Preview (Esc or Space)"
          aria-label="Close Preview"
        >
          ✕
        </button>
      </div>

      {/* Path Breadcrumb Bar */}
      <div className="preview-path-bar">
        <span className="preview-path-text" title={path}>{path}</span>
        <button
          type="button"
          className={`btn-copy-path ${copied ? "copied" : ""}`}
          onClick={handleCopyPath}
          title="Copy full path to clipboard"
        >
          {copied ? "✓ Copied" : "📋 Copy"}
        </button>
      </div>

      {/* Preview Content Body */}
      <div className="preview-body">
        {loading ? (
          <div className="preview-loading">
            <div className="spinner small"></div>
            <span>Reading preview...</span>
          </div>
        ) : isProject ? (
          <div className="project-preview-content">
            <div className="project-preview-section">
              <h3>Project Information</h3>
              <p><strong>Type:</strong> {item.project.project_type}</p>
              {item.project.manifest_path && (
                <p><strong>Manifest:</strong> {item.project.manifest_path}</p>
              )}
              <p><strong>Last Detected:</strong> {new Date(item.project.last_detected_at).toLocaleString()}</p>
            </div>
            {item.project.readme_summary && (
              <div className="project-readme-box">
                <h4>README Summary</h4>
                <p className="readme-text">{item.project.readme_summary}</p>
              </div>
            )}
          </div>
        ) : imageDetails ? (
          <div className="image-preview-content">
            <div className="image-preview-metadata">
              {imageDetails.width && imageDetails.height && (
                <span className="img-dim">{imageDetails.width} × {imageDetails.height} px</span>
              )}
              {imageDetails.format && <span className="img-fmt">{imageDetails.format}</span>}
              {imageDetails.is_screenshot && (
                <span className="badge badge-accent">📸 Screenshot</span>
              )}
            </div>

            {imageDetails.tags && imageDetails.tags.length > 0 && (
              <div className="image-tags-list">
                <h4>Detected Tags & QR Codes</h4>
                <div className="tags-flex">
                  {imageDetails.tags.map((t, idx) => (
                    <span key={idx} className="qr-tag-pill">
                      {t.tag}: {t.masked_payload || t.raw_payload || "Detected"}
                    </span>
                  ))}
                </div>
              </div>
            )}

            {imageDetails.ocr_text && (
              <div className="image-ocr-box">
                <h4>Extracted OCR Text</h4>
                <pre className="ocr-text-view">{imageDetails.ocr_text}</pre>
              </div>
            )}
          </div>
        ) : filePreview?.content_preview ? (
          <div className="text-preview-content">
            <pre className="code-snippet-view">
              <code>{filePreview.content_preview}</code>
            </pre>
          </div>
        ) : item.result.snippet ? (
          <div className="snippet-preview-box">
            <h4>Matching Snippet</h4>
            <div
              className="matched-snippet-html"
              dangerouslySetInnerHTML={{ __html: item.result.snippet }}
            />
          </div>
        ) : (
          <div className="preview-fallback-box">
            <p className="fallback-msg">No text content preview available for this binary format.</p>
            <p className="fallback-sub">Press <kbd>Enter</kbd> to open with your system default app.</p>
          </div>
        )}
      </div>

      {/* Preview Actions Footer */}
      <div className="preview-footer-actions">
        <button
          type="button"
          className="btn-primary btn-sm"
          onClick={() => onOpenFile(path)}
        >
          Open File (Enter)
        </button>
        <button
          type="button"
          className="btn-secondary btn-sm"
          onClick={() => onRevealFile(path)}
        >
          Reveal in Explorer (Ctrl+Enter)
        </button>
      </div>
    </aside>
  );
};
