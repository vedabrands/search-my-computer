// Maps file extensions to simple SVG/emoji icons.
export function getFileIcon(ext: string, kind: string): string {
  const e = ext.toLowerCase();

  switch (e) {
    // Documents
    case "pdf":
      return "📄";
    case "doc":
    case "docx":
      return "📝";
    case "xls":
    case "xlsx":
    case "csv":
      return "📊";
    case "ppt":
    case "pptx":
      return "📑";
    case "txt":
    case "md":
      return "📃";

    // Code
    case "rs":
      return "🦀";
    case "py":
      return "🐍";
    case "js":
    case "jsx":
      return "🟨";
    case "ts":
    case "tsx":
      return "🔷";
    case "html":
    case "css":
      return "🌐";
    case "json":
    case "toml":
    case "yaml":
    case "yml":
      return "⚙️";

    // Images
    case "png":
    case "jpg":
    case "jpeg":
    case "gif":
    case "svg":
    case "webp":
      return "🖼️";

    // Archives
    case "zip":
    case "tar":
    case "gz":
    case "7z":
    case "rar":
      return "📦";

    default:
      if (kind === "code") return "💻";
      if (kind === "document") return "📄";
      if (kind === "image") return "🖼️";
      if (kind === "archive") return "📦";
      return "📁";
  }
}

// Maps project type to emoji icon
export function getProjectIcon(projectType: string): string {
  switch (projectType.toLowerCase()) {
    case "rust":
      return "🦀";
    case "typescript":
    case "javascript":
    case "node":
      return "⚡";
    case "python":
      return "🐍";
    case "go":
      return "🐹";
    case "java":
    case "kotlin":
      return "☕";
    case "csharp":
    case "dotnet":
      return "🔷";
    case "git":
    default:
      return "📦";
  }
}
