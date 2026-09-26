import React from "react";

interface StepWelcomeProps {
  onNext: () => void;
}

export const StepWelcome: React.FC<StepWelcomeProps> = ({ onNext }) => {
  return (
    <div className="onboarding-step step-welcome" role="region" aria-labelledby="welcome-title">
      <div className="welcome-hero-badge">
        <span className="hero-icon" aria-hidden="true">🛡️</span>
        <span className="hero-shield-text">100% Local & Private</span>
      </div>

      <h1 id="welcome-title" className="welcome-title">
        Welcome to SearchMyComputer
      </h1>

      <p className="welcome-subtitle">
        Your fast, private, intelligent desktop search launcher. Everything is indexed directly on your computer — no clouds, no tracking, no subscription required.
      </p>

      <div className="privacy-feature-grid">
        <div className="privacy-feature-card">
          <div className="feature-icon" aria-hidden="true">🔒</div>
          <div className="feature-info">
            <h3>Zero Cloud Communication</h3>
            <p>No network calls at runtime. No telemetry or usage tracking. Your files never leave your device.</p>
          </div>
        </div>

        <div className="privacy-feature-card">
          <div className="feature-icon" aria-hidden="true">⚡</div>
          <div className="feature-info">
            <h3>Smart Hybrid Search</h3>
            <p>Combines exact filenames, deep document contents, code symbols, OCR text, and semantic vectors.</p>
          </div>
        </div>

        <div className="privacy-feature-card">
          <div className="feature-icon" aria-hidden="true">🔋</div>
          <div className="feature-info">
            <h3>Eco & Battery Aware</h3>
            <p>Engineered for lightweight CPU execution with automatic background throttling on battery power.</p>
          </div>
        </div>

        <div className="privacy-feature-card">
          <div className="feature-icon" aria-hidden="true">💊</div>
          <div className="feature-info">
            <h3>Floating Status Pill</h3>
            <p>A discreet status pill is always visible while the app is running so you always know it's working, or paused. Click it or press Alt+Space anytime.</p>
          </div>
        </div>
      </div>

      <div className="onboarding-actions">
        <button
          className="btn-primary btn-large"
          onClick={onNext}
          autoFocus
          aria-label="Continue to folder selection"
        >
          Get Started →
        </button>
      </div>
    </div>
  );
};
