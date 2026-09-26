import React from "react";

interface RecentSearchesProps {
  recentSearches: string[];
  onSelectQuery: (query: string) => void;
  onRemoveItem: (query: string) => void;
  onClearHistory: () => void;
}

export const RecentSearches: React.FC<RecentSearchesProps> = ({
  recentSearches,
  onSelectQuery,
  onRemoveItem,
  onClearHistory,
}) => {
  if (recentSearches.length === 0) return null;

  return (
    <div className="recent-searches-section" aria-label="Recent Searches">
      <div className="recent-searches-header">
        <span className="recent-searches-title">Recent Searches</span>
        <button
          type="button"
          className="btn-clear-history"
          onClick={onClearHistory}
          title="Clear search history"
        >
          Clear
        </button>
      </div>
      <div className="recent-searches-pills">
        {recentSearches.map((item) => (
          <div
            key={item}
            className="recent-search-pill"
            onClick={() => onSelectQuery(item)}
            role="button"
            tabIndex={0}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelectQuery(item);
              }
            }}
          >
            <span className="recent-icon">🕒</span>
            <span className="recent-text">{item}</span>
            <button
              type="button"
              className="recent-remove-btn"
              onClick={(e) => {
                e.stopPropagation();
                onRemoveItem(item);
              }}
              title={`Remove "${item}" from history`}
              aria-label={`Remove ${item}`}
            >
              ×
            </button>
          </div>
        ))}
      </div>
    </div>
  );
};
