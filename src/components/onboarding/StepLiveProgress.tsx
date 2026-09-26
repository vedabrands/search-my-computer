import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { IndexStatus } from "../../types";

interface StepLiveProgressProps {
  onComplete: () => void;
  folderCount: number;
}

export const StepLiveProgress: React.FC<StepLiveProgressProps> = ({ onComplete, folderCount }) => {
  const [status, setStatus] = useState<IndexStatus | null>(null);

  useEffect(() => {
    let timer: any;
    const fetchStatus = async () => {
      try {
        const s = await invoke<IndexStatus>("get_index_status");
        setStatus(s);
      } catch (e) {
        console.error("Failed to fetch index status in onboarding:", e);
      }
    };

    fetchStatus();
    timer = setInterval(fetchStatus, 500);

    return () => {
      if (timer) clearInterval(timer);
    };
  }, []);

  const filesSeen = status?.scan_progress.files_seen ?? 0;
  const filesIndexed = status?.scan_progress.files_indexed ?? 0;
  const pendingJobs = status?.job_counts.pending ?? 0;
  const runningJobs = status?.job_counts.running ?? 0;
  const doneJobs = status?.job_counts.done ?? 0;
  const isScanning = status?.is_scanning ?? true;

  return (
    <div className="onboarding-step step-live-progress" role="region" aria-labelledby="progress-title">
      <div className="progress-hero-icon" aria-hidden="true">
        <div className="pulse-ring"></div>
        <span className="hero-emoji">⚡</span>
      </div>

      <h2 id="progress-title" className="step-title">
        Indexing Your Files
      </h2>
      <p className="step-subtitle">
        SearchMyComputer is cataloging your {folderCount} chosen folders in the background. You can start searching right away!
      </p>

      <div className="live-progress-card">
        <div className="live-stat-grid">
          <div className="stat-box">
            <span className="stat-label">Files Discovered</span>
            <span className="stat-value">{filesSeen.toLocaleString()}</span>
          </div>

          <div className="stat-box">
            <span className="stat-label">Indexed in DB</span>
            <span className="stat-value accent">{filesIndexed.toLocaleString()}</span>
          </div>

          <div className="stat-box">
            <span className="stat-label">Background Jobs</span>
            <span className="stat-value">
              {doneJobs.toLocaleString()} / {(doneJobs + pendingJobs + runningJobs).toLocaleString()}
            </span>
          </div>
        </div>

        <div className="progress-status-bar">
          <div className="status-indicator-dot active"></div>
          <span className="status-text">
            {isScanning
              ? "Scanning file tree and building FTS5 full-text index..."
              : "Initial file scan complete! Processing document embeddings in background."}
          </span>
        </div>
      </div>

      <div className="shortcut-tip-box">
        <span className="tip-badge">Pro Tip</span>
        <span className="tip-text">
          Press <kbd>Alt</kbd> + <kbd>Space</kbd> anytime anywhere to toggle the search launcher.
        </span>
      </div>

      <div className="onboarding-actions center">
        <button
          type="button"
          className="btn-primary btn-large"
          onClick={onComplete}
          autoFocus
        >
          🚀 Start Searching Now
        </button>
      </div>
    </div>
  );
};
