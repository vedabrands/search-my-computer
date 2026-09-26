import React from "react";
import { LicenseStatus } from "../../types";
import "./License.css";

interface TrialBannerProps {
  licenseStatus: LicenseStatus | null;
  onOpenLicenseModal: () => void;
}

export const TrialBanner: React.FC<TrialBannerProps> = ({
  licenseStatus,
  onOpenLicenseModal,
}) => {
  if (!licenseStatus || licenseStatus.status === "Licensed") {
    return null;
  }

  const isExpired = licenseStatus.status === "TrialExpired";
  const daysRemaining =
    licenseStatus.status === "Trial" ? licenseStatus.days_remaining : 0;

  return (
    <div
      className={`trial-banner ${isExpired ? "expired" : daysRemaining <= 3 ? "warning" : "info"}`}
      role="alert"
    >
      <div className="trial-banner-content">
        <span className="trial-banner-icon" aria-hidden="true">
          {isExpired ? "⚠" : daysRemaining <= 3 ? "⏳" : "✨"}
        </span>
        <span className="trial-banner-text">
          {isExpired ? (
            <>
              <strong>Trial Expired:</strong> Search is restricted to top 3 results.
            </>
          ) : (
            <>
              <strong>Free Trial:</strong> {daysRemaining} day{daysRemaining === 1 ? "" : "s"} remaining.
            </>
          )}
        </span>
      </div>
      <div className="trial-banner-actions">
        <button
          type="button"
          className="trial-banner-btn primary"
          onClick={onOpenLicenseModal}
        >
          Enter License Key
        </button>
      </div>
    </div>
  );
};
