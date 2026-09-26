import React, { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { StepWelcome } from "./StepWelcome";
import { StepFolderSelection } from "./StepFolderSelection";
import { StepLiveProgress } from "./StepLiveProgress";
import { AppConfig } from "../../types";

interface OnboardingWizardProps {
  onFinish: () => void;
}

export const OnboardingWizard: React.FC<OnboardingWizardProps> = ({ onFinish }) => {
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [selectedFolderCount, setSelectedFolderCount] = useState(0);

  const handleFoldersSelected = async (
    folders: string[],
    enableImageIndexing: boolean,
    launchAtLogin: boolean
  ) => {
    setSelectedFolderCount(folders.length);
    try {
      // 1. Get current config
      const currentConfig = await invoke<AppConfig>("get_config");

      // 2. Merge onboarding preferences
      const updatedConfig: AppConfig = {
        ...currentConfig,
        indexed_folders: folders,
        enable_image_indexing: enableImageIndexing,
        launch_at_login: launchAtLogin,
        onboarding_completed: true,
      };

      await invoke("update_config", { newConfig: updatedConfig });

      // 3. Set launch at login OS setting
      if (launchAtLogin) {
        await invoke("set_launch_at_login", { enable: true }).catch(() => {});
      }

      // 4. Trigger initial index scan across configured folders
      await invoke("start_scan").catch((e) => console.error("Failed to start scan:", e));

      // 5. Advance to live progress screen
      setStep(3);
    } catch (err) {
      console.error("Failed to save onboarding settings:", err);
      setStep(3);
    }
  };

  return (
    <div className="onboarding-overlay" role="dialog" aria-modal="true" aria-label="First-Run Setup">
      <div className="onboarding-container">
        {/* Progress Stepper Header */}
        <div className="onboarding-stepper" aria-label="Setup Steps">
          <div className={`step-node ${step >= 1 ? "active" : ""} ${step > 1 ? "completed" : ""}`}>
            <span className="step-circle">1</span>
            <span className="step-label">Welcome</span>
          </div>
          <div className="step-connector"></div>
          <div className={`step-node ${step >= 2 ? "active" : ""} ${step > 2 ? "completed" : ""}`}>
            <span className="step-circle">2</span>
            <span className="step-label">Folders</span>
          </div>
          <div className="step-connector"></div>
          <div className={`step-node ${step >= 3 ? "active" : ""}`}>
            <span className="step-circle">3</span>
            <span className="step-label">Indexing</span>
          </div>
        </div>

        {/* Step Presenters */}
        <div className="onboarding-content-card">
          {step === 1 && <StepWelcome onNext={() => setStep(2)} />}
          {step === 2 && (
            <StepFolderSelection
              onNext={handleFoldersSelected}
              onBack={() => setStep(1)}
            />
          )}
          {step === 3 && (
            <StepLiveProgress
              onComplete={onFinish}
              folderCount={selectedFolderCount}
            />
          )}
        </div>
      </div>
    </div>
  );
};
