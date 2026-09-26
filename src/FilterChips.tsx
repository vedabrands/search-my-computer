import React from "react";
import { ParsedQuery } from "./types";

interface FilterChipsProps {
  parsedQuery: ParsedQuery | null;
}

export const FilterChips: React.FC<FilterChipsProps> = ({ parsedQuery }) => {
  if (!parsedQuery) return null;

  const chips: Array<{ id: string; icon: string; label: string; type: string }> = [];

  // File type chips
  for (const ft of parsedQuery.file_types) {
    chips.push({
      id: `type-${ft}`,
      icon: "📄",
      label: ft.toUpperCase(),
      type: "type",
    });
  }

  // Location hint chips
  for (const loc of parsedQuery.location_hints) {
    chips.push({
      id: `loc-${loc}`,
      icon: "📁",
      label: `in ${loc}`,
      type: "location",
    });
  }

  // Date range chips
  if (parsedQuery.after || parsedQuery.before) {
    let dateLabel = "Date filter";
    if (parsedQuery.after && parsedQuery.before) {
      const afterStr = new Date(parsedQuery.after).toLocaleDateString(undefined, {
        month: "short",
        day: "numeric",
        year: "numeric",
      });
      const beforeStr = new Date(parsedQuery.before).toLocaleDateString(undefined, {
        month: "short",
        day: "numeric",
        year: "numeric",
      });
      dateLabel = `${afterStr} – ${beforeStr}`;
    } else if (parsedQuery.after) {
      const d = new Date(parsedQuery.after).toLocaleDateString(undefined, {
        month: "short",
        day: "numeric",
        year: "numeric",
      });
      dateLabel = `After ${d}`;
    } else if (parsedQuery.before) {
      const d = new Date(parsedQuery.before).toLocaleDateString(undefined, {
        month: "short",
        day: "numeric",
        year: "numeric",
      });
      dateLabel = `Before ${d}`;
    }

    chips.push({
      id: "date-filter",
      icon: "📅",
      label: dateLabel,
      type: "date",
    });
  }

  // Intent chips
  if (parsedQuery.is_project_query) {
    chips.push({
      id: "intent-project",
      icon: "🚀",
      label: "Projects",
      type: "intent",
    });
  }

  if (parsedQuery.is_screenshot_query) {
    chips.push({
      id: "intent-screenshot",
      icon: "📸",
      label: "Screenshots",
      type: "intent",
    });
  }

  if (chips.length === 0) return null;

  return (
    <div className="filter-chips-container">
      {chips.map((chip) => (
        <span key={chip.id} className={`filter-chip chip-${chip.type}`}>
          <span className="filter-chip-icon">{chip.icon}</span>
          <span className="filter-chip-label">{chip.label}</span>
        </span>
      ))}
    </div>
  );
};
