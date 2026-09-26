use crate::db::{Database, ProjectRecord};
use crate::error::CoreResult;
use chrono::Utc;
use globset::GlobSet;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};
use walkdir::WalkDir;

/// A detected project entity in the filesystem.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedProject {
    pub path: PathBuf,
    pub name: String,
    pub project_type: String,
    pub manifest_path: Option<PathBuf>,
    pub readme_summary: Option<String>,
}

/// Detect if a single directory is a project root.
pub fn detect_project_at(dir: &Path) -> Option<DetectedProject> {
    if !dir.is_dir() {
        return None;
    }

    let dir_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());

    let mut project_type = None;
    let mut project_name = None;
    let mut manifest_path = None;

    // 1. Rust: Cargo.toml
    let cargo_toml = dir.join("Cargo.toml");
    if cargo_toml.is_file() {
        project_type = Some("rust".to_string());
        manifest_path = Some(cargo_toml.clone());
        if let Ok(content) = fs::read_to_string(&cargo_toml) {
            project_name = extract_toml_field(&content, "name");
        }
    }

    // 2. Node / TypeScript: package.json
    if project_type.is_none() {
        let pkg_json = dir.join("package.json");
        if pkg_json.is_file() {
            let is_ts = dir.join("tsconfig.json").is_file();
            project_type = Some(if is_ts {
                "typescript".to_string()
            } else {
                "javascript".to_string()
            });
            manifest_path = Some(pkg_json.clone());
            if let Ok(content) = fs::read_to_string(&pkg_json)
                && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
                && let Some(name) = json.get("name").and_then(|v| v.as_str())
            {
                let clean_name = name.trim().trim_start_matches('@');
                if let Some(last) = clean_name.split('/').next_back() {
                    project_name = Some(last.to_string());
                } else {
                    project_name = Some(clean_name.to_string());
                }
            }
        }
    }

    // 3. Python: pyproject.toml, setup.py, requirements.txt, Pipfile
    if project_type.is_none() {
        let pyproject = dir.join("pyproject.toml");
        let setup_py = dir.join("setup.py");
        let req_txt = dir.join("requirements.txt");
        let pipfile = dir.join("Pipfile");

        if pyproject.is_file() {
            project_type = Some("python".to_string());
            manifest_path = Some(pyproject.clone());
            if let Ok(content) = fs::read_to_string(&pyproject) {
                project_name = extract_toml_field(&content, "name");
            }
        } else if setup_py.is_file() {
            project_type = Some("python".to_string());
            manifest_path = Some(setup_py);
        } else if req_txt.is_file() {
            project_type = Some("python".to_string());
            manifest_path = Some(req_txt);
        } else if pipfile.is_file() {
            project_type = Some("python".to_string());
            manifest_path = Some(pipfile);
        }
    }

    // 4. Go: go.mod
    if project_type.is_none() {
        let go_mod = dir.join("go.mod");
        if go_mod.is_file() {
            project_type = Some("go".to_string());
            manifest_path = Some(go_mod.clone());
            if let Ok(content) = fs::read_to_string(&go_mod) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("module ") {
                        let mod_name = trimmed.trim_start_matches("module ").trim();
                        if let Some(last) = mod_name.split('/').next_back() {
                            project_name = Some(last.to_string());
                        }
                        break;
                    }
                }
            }
        }
    }

    // 5. Java / Kotlin: pom.xml, build.gradle, build.gradle.kts
    if project_type.is_none() {
        let pom = dir.join("pom.xml");
        let gradle = dir.join("build.gradle");
        let gradle_kts = dir.join("build.gradle.kts");

        if pom.is_file() {
            project_type = Some("java".to_string());
            manifest_path = Some(pom);
        } else if gradle_kts.is_file() {
            project_type = Some("kotlin".to_string());
            manifest_path = Some(gradle_kts);
        } else if gradle.is_file() {
            project_type = Some("java".to_string());
            manifest_path = Some(gradle);
        }
    }

    // 6. C# / .NET: *.sln, *.csproj
    if project_type.is_none()
        && let Ok(entries) = fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext.eq_ignore_ascii_case("sln") || ext.eq_ignore_ascii_case("csproj") {
                    project_type = Some("csharp".to_string());
                    manifest_path = Some(path.clone());
                    if let Some(stem) = path.file_stem() {
                        project_name = Some(stem.to_string_lossy().to_string());
                    }
                    break;
                }
            }
        }
    }

    // 7. Git repository fallback: .git
    if project_type.is_none() {
        let git_dir = dir.join(".git");
        if git_dir.exists() {
            project_type = Some("git".to_string());
        }
    }

    let ptype = project_type?;
    let name = project_name.unwrap_or(dir_name);
    let readme_summary = extract_readme_summary(dir);

    Some(DetectedProject {
        path: dir.to_path_buf(),
        name,
        project_type: ptype,
        manifest_path,
        readme_summary,
    })
}

/// Helper to parse a simple TOML field like `name = "value"`.
fn extract_toml_field(content: &str, field: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(field) {
            let rest = rest.trim();
            if rest.starts_with('=') {
                let val = rest.trim_start_matches('=').trim();
                let clean = val.trim_matches('"').trim_matches('\'').trim();
                if !clean.is_empty() {
                    return Some(clean.to_string());
                }
            }
        }
    }
    None
}

/// Extract and clean summary text from a project's README file.
pub fn extract_readme_summary(dir: &Path) -> Option<String> {
    let readme_names = [
        "README.md",
        "readme.md",
        "README.MD",
        "README.txt",
        "readme.txt",
        "README",
        "readme",
    ];

    let mut readme_path = None;
    for name in &readme_names {
        let p = dir.join(name);
        if p.is_file() {
            readme_path = Some(p);
            break;
        }
    }

    let path = readme_path?;
    let content = fs::read_to_string(path).ok()?;
    if content.trim().is_empty() {
        return None;
    }

    // Clean markdown elements:
    let mut cleaned_lines = Vec::new();
    let mut in_code_block = false;

    for line in content.lines().take(60) {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block || trimmed.is_empty() {
            continue;
        }

        // Skip badge images / shield links
        if trimmed.starts_with("[![") || (trimmed.starts_with("![") && trimmed.contains("badge")) {
            continue;
        }
        // Skip pure image lines
        if trimmed.starts_with("![") && trimmed.ends_with(')') {
            continue;
        }

        // Strip heading hashes
        let no_headings = trimmed.trim_start_matches('#').trim();

        // Strip HTML tags roughly
        let mut no_html = String::new();
        let mut in_tag = false;
        for c in no_headings.chars() {
            if c == '<' {
                in_tag = true;
            } else if c == '>' {
                in_tag = false;
            } else if !in_tag {
                no_html.push(c);
            }
        }

        // Simplify markdown links [text](url) -> text
        let mut simplified = String::new();
        let mut chars = no_html.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '[' {
                let mut link_text = String::new();
                let mut found_close = false;
                for lc in chars.by_ref() {
                    if lc == ']' {
                        found_close = true;
                        break;
                    }
                    link_text.push(lc);
                }
                if found_close && chars.peek() == Some(&'(') {
                    chars.next(); // consume '('
                    for lc in chars.by_ref() {
                        if lc == ')' {
                            break;
                        }
                    }
                    simplified.push_str(&link_text);
                } else {
                    simplified.push('[');
                    simplified.push_str(&link_text);
                    if found_close {
                        simplified.push(']');
                    }
                }
            } else {
                simplified.push(c);
            }
        }

        let line_clean = simplified.trim();
        if !line_clean.is_empty() {
            cleaned_lines.push(line_clean.to_string());
        }

        if cleaned_lines.len() >= 5 {
            break;
        }
    }

    let combined = cleaned_lines.join(" ");
    let words: Vec<&str> = combined.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }

    let full_summary = words.join(" ");
    let truncated = if full_summary.chars().count() > 300 {
        let mut s: String = full_summary.chars().take(300).collect();
        s.push_str("...");
        s
    } else {
        full_summary
    };

    Some(truncated)
}

/// Scan a directory tree for projects, respecting exclusions.
pub fn scan_for_projects(root: &Path, exclusions: Option<&GlobSet>) -> Vec<DetectedProject> {
    let mut projects = Vec::new();

    let walker = WalkDir::new(root).follow_links(false).into_iter();

    for entry in walker.filter_entry(|e| {
        let path = e.path();
        let file_name = e.file_name().to_string_lossy();

        // Skip internal hidden metadata directories during tree traversal, but allow the root itself
        if path != root && file_name.starts_with('.') && file_name != ".git" {
            return false;
        }

        if let Some(excl) = exclusions
            && excl.is_match(path)
        {
            return false;
        }
        true
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entry.file_type().is_dir()
            && let Some(project) = detect_project_at(entry.path())
        {
            debug!(name = %project.name, path = ?project.path, kind = %project.project_type, "detected project");
            projects.push(project);
        }
    }

    projects
}

/// Detect and index all projects under `root` into the database.
pub fn index_projects_in_root(
    root: &Path,
    exclusions: Option<&GlobSet>,
    db: &Database,
) -> CoreResult<usize> {
    let detected = scan_for_projects(root, exclusions);
    let now = Utc::now().to_rfc3339();
    let mut count = 0;

    for p in detected {
        let path_str = p.path.to_string_lossy().replace('\\', "/");
        let manifest_str = p
            .manifest_path
            .map(|m| m.to_string_lossy().replace('\\', "/"));

        let record = ProjectRecord {
            id: 0,
            path: path_str,
            name: p.name,
            project_type: p.project_type,
            manifest_path: manifest_str,
            readme_summary: p.readme_summary,
            last_detected_at: now.clone(),
        };

        db.upsert_project(&record)?;
        count += 1;
    }

    info!(count, root = ?root, "indexed projects in root");
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_detect_rust_project() {
        let dir = tempdir().unwrap();
        let cargo = dir.path().join("Cargo.toml");
        fs::write(
            &cargo,
            r#"
[package]
name = "my-awesome-tool"
version = "0.1.0"
"#,
        )
        .unwrap();

        let readme = dir.path().join("README.md");
        fs::write(
            &readme,
            "# My Awesome Tool\n\nA blazing fast CLI tool for searching files.",
        )
        .unwrap();

        let detected = detect_project_at(dir.path()).unwrap();
        assert_eq!(detected.name, "my-awesome-tool");
        assert_eq!(detected.project_type, "rust");
        assert_eq!(detected.manifest_path, Some(cargo));
        assert!(
            detected
                .readme_summary
                .unwrap()
                .contains("A blazing fast CLI tool")
        );
    }

    #[test]
    fn test_detect_typescript_project() {
        let dir = tempdir().unwrap();
        let pkg = dir.path().join("package.json");
        fs::write(
            &pkg,
            r#"{"name": "@scope/frontend-app", "version": "1.0.0"}"#,
        )
        .unwrap();
        let tsconfig = dir.path().join("tsconfig.json");
        fs::write(&tsconfig, "{}").unwrap();

        let detected = detect_project_at(dir.path()).unwrap();
        assert_eq!(detected.name, "frontend-app");
        assert_eq!(detected.project_type, "typescript");
    }

    #[test]
    fn test_detect_python_project() {
        let dir = tempdir().unwrap();
        let pyproject = dir.path().join("pyproject.toml");
        fs::write(
            &pyproject,
            r#"
[project]
name = "data-pipeline"
version = "0.2.0"
"#,
        )
        .unwrap();

        let detected = detect_project_at(dir.path()).unwrap();
        assert_eq!(detected.name, "data-pipeline");
        assert_eq!(detected.project_type, "python");
    }

    #[test]
    fn test_scan_and_index_projects_db() {
        let dir = tempdir().unwrap();
        let prj1 = dir.path().join("service-a");
        fs::create_dir_all(&prj1).unwrap();
        fs::write(prj1.join("Cargo.toml"), "[package]\nname = \"service-a\"\n").unwrap();

        let prj2 = dir.path().join("service-b");
        fs::create_dir_all(&prj2).unwrap();
        fs::write(prj2.join("go.mod"), "module github.com/user/service-b\n").unwrap();

        let db = Database::open_in_memory().unwrap();
        let count = index_projects_in_root(dir.path(), None, &db).unwrap();
        assert_eq!(count, 2);

        let search = db.search_projects("service", 10).unwrap();
        assert_eq!(search.len(), 2);
    }
}
