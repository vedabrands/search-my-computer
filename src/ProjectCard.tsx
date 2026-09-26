import React from "react";
import { ProjectRecord } from "./types";
import { getProjectIcon } from "./fileIcons";

interface ProjectCardProps {
  project: ProjectRecord;
  isSelected: boolean;
  onSelect: () => void;
  onOpen: (project: ProjectRecord) => void;
  onReveal: (project: ProjectRecord) => void;
  onOpenTerminal: (project: ProjectRecord) => void;
}

function getProjectTypeBadgeClass(type: string): string {
  switch (type.toLowerCase()) {
    case "rust":
      return "badge-rust";
    case "typescript":
    case "javascript":
    case "node":
      return "badge-ts";
    case "python":
      return "badge-py";
    case "go":
      return "badge-go";
    case "java":
    case "kotlin":
      return "badge-java";
    case "csharp":
    case "dotnet":
      return "badge-cs";
    default:
      return "badge-git";
  }
}

export const ProjectCard: React.FC<ProjectCardProps> = ({
  project,
  isSelected,
  onSelect,
  onOpen,
  onReveal,
  onOpenTerminal,
}) => {
  const icon = getProjectIcon(project.project_type);
  const badgeClass = getProjectTypeBadgeClass(project.project_type);

  return (
    <div
      className={`project-card ${isSelected ? "selected" : ""}`}
      role="option"
      aria-selected={isSelected}
      onClick={() => {
        onSelect();
        onOpen(project);
      }}
      onMouseEnter={onSelect}
    >
      <div className="project-card-header">
        <span className="project-icon">{icon}</span>
        <div className="project-title-area">
          <span className="project-name">{project.name}</span>
          <span className={`project-badge ${badgeClass}`}>
            {project.project_type.toUpperCase()} PROJECT
          </span>
        </div>
        <div className="project-quick-actions">
          <button
            className="action-btn"
            title="Open in Terminal (Alt+T)"
            onClick={(e) => {
              e.stopPropagation();
              onOpenTerminal(project);
            }}
          >
            💻 Terminal
          </button>
          <button
            className="action-btn"
            title="Reveal in Explorer (Alt+O / Ctrl+Enter)"
            onClick={(e) => {
              e.stopPropagation();
              onReveal(project);
            }}
          >
            📁 Folder
          </button>
        </div>
      </div>

      <div className="project-path">{project.path}</div>

      {project.readme_summary && (
        <div className="project-readme-preview">
          <span className="readme-label">README: </span>
          {project.readme_summary}
        </div>
      )}
    </div>
  );
};
