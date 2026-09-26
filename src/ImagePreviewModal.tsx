import React, { useState, useEffect } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { SearchResult, ImageDetailsResponse } from "./types";

interface ImagePreviewModalProps {
  result: SearchResult;
  onClose: () => void;
  onOpenFile: (result: SearchResult) => void;
}

export const ImagePreviewModal: React.FC<ImagePreviewModalProps> = ({
  result,
  onClose,
  onOpenFile,
}) => {
  const [details, setDetails] = useState<ImageDetailsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [revealedPayloads, setRevealedPayloads] = useState<Record<number, boolean>>({});
  const [copiedOcr, setCopiedOcr] = useState(false);

  useEffect(() => {
    let isMounted = true;
    setLoading(true);

    invoke<ImageDetailsResponse | null>("get_image_details", { fileId: result.id })
      .then((res) => {
        if (isMounted) {
          setDetails(res);
          setLoading(false);
        }
      })
      .catch((err) => {
        console.error("Failed to load image details:", err);
        if (isMounted) {
          setLoading(false);
        }
      });

    return () => {
      isMounted = false;
    };
  }, [result.id]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" || e.code === "Space") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      } else if (e.key === "Enter") {
        e.preventDefault();
        onOpenFile(result);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [onClose, onOpenFile, result]);

  const togglePayload = (idx: number) => {
    setRevealedPayloads((prev) => ({
      ...prev,
      [idx]: !prev[idx],
    }));
  };

  const handleCopyOcr = async () => {
    if (details?.ocr_text) {
      try {
        await navigator.clipboard.writeText(details.ocr_text);
        setCopiedOcr(true);
        setTimeout(() => setCopiedOcr(false), 2000);
      } catch (err) {
        console.error("Failed to copy OCR text:", err);
      }
    }
  };

  const imgSrc = convertFileSrc(result.path);

  return (
    <div className="preview-overlay" onClick={onClose}>
      <div className="preview-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="preview-header">
          <div className="preview-title-block">
            <span className="preview-filename">{result.name}</span>
            <span className="preview-path">{result.path}</span>
          </div>
          <button className="preview-close-btn" onClick={onClose} title="Close (Esc or Space)">
            ✕
          </button>
        </div>

        <div className="preview-body">
          <div className="preview-image-container">
            <img
              src={imgSrc}
              alt={result.name}
              className="preview-image"
              onError={(e) => {
                // If direct local file fails in preview, fallback to placeholder
                (e.target as HTMLImageElement).style.display = "none";
              }}
            />
          </div>

          <div className="preview-details-pane">
            <div className="preview-section">
              <div className="preview-section-title">Properties</div>
              <div className="metadata-grid">
                {details?.width && details?.height ? (
                  <div className="metadata-item">
                    <span className="metadata-label">Dimensions</span>
                    <span className="metadata-value">
                      {details.width} × {details.height}
                    </span>
                  </div>
                ) : null}

                {details?.format && (
                  <div className="metadata-item">
                    <span className="metadata-label">Format</span>
                    <span className="metadata-value">{details.format.toUpperCase()}</span>
                  </div>
                )}

                {details?.is_screenshot && (
                  <div className="metadata-item">
                    <span className="metadata-label">Type</span>
                    <span className="badge-screenshot">SCREENSHOT</span>
                  </div>
                )}

                {details?.camera_make || details?.camera_model ? (
                  <div className="metadata-item">
                    <span className="metadata-label">Camera</span>
                    <span className="metadata-value">
                      {[details.camera_make, details.camera_model].filter(Boolean).join(" ")}
                    </span>
                  </div>
                ) : null}

                {details?.exif_date && (
                  <div className="metadata-item">
                    <span className="metadata-label">Date Taken</span>
                    <span className="metadata-value">{details.exif_date}</span>
                  </div>
                )}

                <div className="metadata-item">
                  <span className="metadata-label">File Size</span>
                  <span className="metadata-value">
                    {(result.size / 1024).toFixed(1)} KB
                  </span>
                </div>
              </div>
            </div>

            {details && details.tags && details.tags.length > 0 && (
              <div className="preview-section">
                <div className="preview-section-title">Tags & Barcodes</div>
                <div className="tags-list">
                  {details.tags.map((tag, idx) => {
                    const isRevealed = !!revealedPayloads[idx];
                    const isSensitive =
                      tag.tag.startsWith("qr:payment") ||
                      tag.tag.startsWith("qr:wifi") ||
                      tag.tag.startsWith("qr:url") ||
                      tag.tag.startsWith("qr:contact") ||
                      tag.tag.startsWith("qr:text");

                    return (
                      <div key={idx} className="tag-item-box">
                        <div className="tag-header">
                          <span className="tag-pill">{tag.tag}</span>
                          {isSensitive && (
                            <button
                              className="reveal-btn"
                              onClick={() => togglePayload(idx)}
                              title={isRevealed ? "Hide payload for privacy" : "Reveal full payload"}
                            >
                              {isRevealed ? "Hide" : "Click to reveal"}
                            </button>
                          )}
                        </div>
                        {isSensitive && (
                          <div className="tag-payload">
                            {isRevealed
                              ? tag.raw_payload || "(empty payload)"
                              : tag.masked_payload || "(empty payload)"}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            )}

            {details?.ocr_text && (
              <div className="preview-section">
                <div className="preview-section-header">
                  <span className="preview-section-title">Extracted Text (OCR)</span>
                  <button
                    className="copy-ocr-btn"
                    onClick={handleCopyOcr}
                    title="Copy extracted text to clipboard"
                  >
                    {copiedOcr ? "Copied!" : "Copy"}
                  </button>
                </div>
                <div className="ocr-text-box">
                  <pre className="ocr-text-content">{details.ocr_text}</pre>
                </div>
              </div>
            )}

            {loading && (
              <div className="preview-loading">
                <span>Loading image details...</span>
              </div>
            )}
          </div>
        </div>

        <div className="preview-footer">
          <div className="preview-shortcuts">
            <span><kbd>Space</kbd> or <kbd>Esc</kbd> Close</span>
            <span><kbd>Enter</kbd> Open File</span>
          </div>
          <button className="primary-open-btn" onClick={() => onOpenFile(result)}>
            Open File
          </button>
        </div>
      </div>
    </div>
  );
};
