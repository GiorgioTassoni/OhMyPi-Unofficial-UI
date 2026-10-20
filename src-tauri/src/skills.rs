//! Skills management: discovery, creation, and remote importing.
//!
//! Handles Agent Skills conforming to the standard specification (`SKILL.md` with YAML frontmatter).
//! Supports:
//! - Local discovery from user (`~/.omp/agent/skills`), project (`<workspace>/.omp/skills` & `<workspace>/.agents/skills`), and Claude user (`~/.claude/skills`).
//! - Creating custom skills with name, description, and instructions.
//! - Inspecting and importing skills from GitHub repositories (e.g. `pbakaus/impeccable`) or direct URLs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub path: String,
    pub scope: String, // "user", "project", "claude"
    pub has_scripts: bool,
    pub has_references: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DetectedSkill {
    pub name: String,
    pub description: String,
    pub relative_path: String,
    pub has_scripts: bool,
    pub has_references: bool,
    pub content_preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectSkillResult {
    pub repo_name: String,
    pub skills: Vec<DetectedSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSkillInput {
    pub name: String,
    pub description: String,
    pub content: String,
    pub scope: String, // "user" or "project"
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSkillInput {
    pub source_url: String,
    pub skill_name: Option<String>,
    pub scope: String, // "user" or "project"
    pub workspace: Option<String>,
}

#[derive(Deserialize, Default)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
}

/// Parse YAML frontmatter between leading `---` delimiters.
pub fn parse_skill_markdown(raw: &str) -> (String, String, String) {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return (String::new(), String::new(), raw.to_string());
    }

    let after_first = &trimmed[3..];
    let end_idx = after_first.find("\n---").or_else(|| after_first.find("\r\n---"));
    let Some(idx) = end_idx else {
        return (String::new(), String::new(), raw.to_string());
    };

    let yaml_block = &after_first[..idx];
    let body_start = idx + if after_first[idx..].starts_with("\r\n---") { 5 } else { 4 };
    let remainder = &after_first[body_start..];
    let body = remainder.trim_start_matches(|c| c == '\r' || c == '\n').to_string();

    let meta: SkillFrontmatter = serde_yaml::from_str(yaml_block).unwrap_or_default();
    (
        meta.name.unwrap_or_default(),
        meta.description.unwrap_or_default(),
        body,
    )
}

/// Resolve user-level agent skills directory: `~/.omp/agent/skills`.
pub fn user_skills_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "could not determine user home directory".to_string())?;
    Ok(PathBuf::from(home).join(".omp").join("agent").join("skills"))
}

/// Resolve Claude user skills directory: `~/.claude/skills`.
pub fn claude_user_skills_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "could not determine user home directory".to_string())?;
    Ok(PathBuf::from(home).join(".claude").join("skills"))
}

/// Resolve target directory for a skill name and scope.
pub fn resolve_target_skill_dir(
    scope: &str,
    skill_name: &str,
    workspace: Option<&str>,
) -> Result<PathBuf, String> {
    let sanitized = sanitize_skill_name(skill_name)?;
    if scope == "project" {
        let ws = workspace.ok_or_else(|| "project scope requires an active workspace".to_string())?;
        Ok(PathBuf::from(ws).join(".omp").join("skills").join(sanitized))
    } else {
        Ok(user_skills_dir()?.join(sanitized))
    }
}

pub fn sanitize_skill_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("skill name cannot be empty".to_string());
    }
    if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("skill name may only contain alphanumeric characters, hyphens, and underscores".to_string());
    }
    Ok(trimmed.to_lowercase())
}

/// Scan a directory for `SKILL.md` entries in immediate subdirectories.
pub fn scan_skills_in_dir(base_dir: &Path, scope: &str) -> Vec<SkillInfo> {
    let mut skills = Vec::new();
    let Ok(entries) = fs::read_dir(base_dir) else {
        return skills;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }

        if let Ok(content) = fs::read_to_string(&skill_md) {
            let dir_name = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let (name, description, _) = parse_skill_markdown(&content);
            let final_name = if name.trim().is_empty() { dir_name } else { name };

            let has_scripts = path.join("scripts").is_dir();
            let has_references = path.join("reference").is_dir() || path.join("references").is_dir();

            skills.push(SkillInfo {
                name: final_name,
                description,
                path: skill_md.to_string_lossy().to_string(),
                scope: scope.to_string(),
                has_scripts,
                has_references,
            });
        }
    }

    skills
}

/// Copy directory recursively.
pub fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// Normalize GitHub URL or shorthand to a cloneable git URL and a repo name.
pub fn normalize_github_source(source: &str) -> (String, String) {
    let trimmed = source.trim();
    if trimmed.starts_with("https://github.com/") || trimmed.starts_with("http://github.com/") {
        let stripped = trimmed.trim_end_matches(".git");
        let parts: Vec<&str> = stripped.split('/').collect();
        let name = parts.last().copied().unwrap_or("repository");
        (format!("{stripped}.git"), name.to_string())
    } else if trimmed.contains('/') && !trimmed.contains(':') && !trimmed.starts_with("http") {
        // e.g. "pbakaus/impeccable"
        let parts: Vec<&str> = trimmed.split('/').collect();
        let name = parts.get(1).copied().unwrap_or("repository");
        (format!("https://github.com/{trimmed}.git"), name.to_string())
    } else {
        (trimmed.to_string(), "repository".to_string())
    }
}

/// Find all skills within a local directory tree (e.g. cloned repo).
pub fn find_skills_in_tree(root: &Path) -> Vec<DetectedSkill> {
    let mut detected = Vec::new();

    // 1. Check root SKILL.md
    let root_skill = root.join("SKILL.md");
    if root_skill.is_file() {
        if let Ok(content) = fs::read_to_string(&root_skill) {
            let (name, desc, body) = parse_skill_markdown(&content);
            let skill_name = if name.is_empty() {
                root.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "skill".to_string())
            } else {
                name
            };
            detected.push(DetectedSkill {
                name: skill_name,
                description: desc,
                relative_path: ".".to_string(),
                has_scripts: root.join("scripts").is_dir(),
                has_references: root.join("reference").is_dir() || root.join("references").is_dir(),
                content_preview: body.chars().take(300).collect(),
            });
        }
    }

    // 2. Check candidate subdirectories: `.agents/skills/*`, `.omp/skills/*`, `skills/*`
    let candidates = [
        root.join(".agents").join("skills"),
        root.join(".agent").join("skills"),
        root.join(".omp").join("skills"),
        root.join("skills"),
    ];

    for candidate_dir in candidates {
        if !candidate_dir.is_dir() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&candidate_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let skill_md = path.join("SKILL.md");
                if !skill_md.is_file() {
                    continue;
                }
                if let Ok(content) = fs::read_to_string(&skill_md) {
                    let dir_name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let (name, desc, body) = parse_skill_markdown(&content);
                    let final_name = if name.is_empty() { dir_name.clone() } else { name };

                    let rel = path
                        .strip_prefix(root)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| dir_name);

                    detected.push(DetectedSkill {
                        name: final_name,
                        description: desc,
                        relative_path: rel,
                        has_scripts: path.join("scripts").is_dir(),
                        has_references: path.join("reference").is_dir() || path.join("references").is_dir(),
                        content_preview: body.chars().take(300).collect(),
                    });
                }
            }
        }
    }

    detected
}

// -----------------------------------------------------------------------------
// Tauri Commands
// -----------------------------------------------------------------------------

#[tauri::command]
pub async fn list_skills(workspace: Option<String>) -> Result<Vec<SkillInfo>, String> {
    let mut all = Vec::new();

    // 1. User skills
    if let Ok(user_dir) = user_skills_dir() {
        all.extend(scan_skills_in_dir(&user_dir, "user"));
    }

    // 2. Project skills
    if let Some(ws) = &workspace {
        let ws_path = Path::new(ws);
        all.extend(scan_skills_in_dir(&ws_path.join(".omp").join("skills"), "project"));
        all.extend(scan_skills_in_dir(&ws_path.join(".agents").join("skills"), "project"));
    }

    // 3. Claude user skills
    if let Ok(claude_dir) = claude_user_skills_dir() {
        all.extend(scan_skills_in_dir(&claude_dir, "claude"));
    }

    // Deduplicate by name (user/project takes precedence over claude)
    let mut seen = std::collections::HashSet::new();
    all.retain(|s| seen.insert(s.name.clone()));

    all.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(all)
}

#[tauri::command]
pub async fn create_custom_skill(input: CreateSkillInput) -> Result<SkillInfo, String> {
    let name = sanitize_skill_name(&input.name)?;
    let target_dir = resolve_target_skill_dir(&input.scope, &name, input.workspace.as_deref())?;

    fs::create_dir_all(&target_dir)
        .map_err(|e| format!("failed to create skill directory: {e}"))?;

    let description_clean = input.description.trim().replace('"', "\\\"");
    let full_content = format!(
        "---\nname: {name}\ndescription: \"{description_clean}\"\n---\n\n{body}\n",
        name = name,
        description_clean = description_clean,
        body = input.content.trim()
    );

    let skill_md_path = target_dir.join("SKILL.md");
    fs::write(&skill_md_path, full_content)
        .map_err(|e| format!("failed to write SKILL.md: {e}"))?;

    Ok(SkillInfo {
        name,
        description: input.description,
        path: skill_md_path.to_string_lossy().to_string(),
        scope: input.scope,
        has_scripts: false,
        has_references: false,
    })
}

#[tauri::command]
pub async fn inspect_remote_skill(source_url: String) -> Result<InspectSkillResult, String> {
    let (git_url, repo_name) = normalize_github_source(&source_url);

    let temp_dir = std::env::temp_dir().join(format!("omp-skill-inspect-{}", std::process::id()));
    if temp_dir.exists() {
        let _ = fs::remove_dir_all(&temp_dir);
    }

    let output = Command::new("git")
        .args(["clone", "--depth", "1", &git_url])
        .arg(&temp_dir)
        .output()
        .map_err(|e| format!("failed to run git clone: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = fs::remove_dir_all(&temp_dir);
        return Err(format!("git clone failed: {stderr}"));
    }

    let skills = find_skills_in_tree(&temp_dir);
    let _ = fs::remove_dir_all(&temp_dir);

    if skills.is_empty() {
        return Err("no skills (SKILL.md) found in repository".to_string());
    }

    Ok(InspectSkillResult { repo_name, skills })
}

#[tauri::command]
pub async fn import_remote_skill(input: ImportSkillInput) -> Result<SkillInfo, String> {
    let (git_url, _) = normalize_github_source(&input.source_url);

    let temp_dir = std::env::temp_dir().join(format!("omp-skill-import-{}", std::process::id()));
    if temp_dir.exists() {
        let _ = fs::remove_dir_all(&temp_dir);
    }

    let output = Command::new("git")
        .args(["clone", "--depth", "1", &git_url])
        .arg(&temp_dir)
        .output()
        .map_err(|e| format!("failed to clone repository: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = fs::remove_dir_all(&temp_dir);
        return Err(format!("git clone failed: {stderr}"));
    }

    let skills = find_skills_in_tree(&temp_dir);
    if skills.is_empty() {
        let _ = fs::remove_dir_all(&temp_dir);
        return Err("no skills (SKILL.md) found in repository".to_string());
    }

    let selected = if let Some(target_name) = &input.skill_name {
        skills.into_iter().find(|s| s.name == *target_name)
    } else {
        skills.into_iter().next()
    };

    let Some(skill) = selected else {
        let _ = fs::remove_dir_all(&temp_dir);
        return Err("specified skill was not found in repository".to_string());
    };

    let target_dir = resolve_target_skill_dir(&input.scope, &skill.name, input.workspace.as_deref())?;
    let source_dir = if skill.relative_path == "." {
        temp_dir.clone()
    } else {
        temp_dir.join(&skill.relative_path)
    };

    if target_dir.exists() {
        let _ = fs::remove_dir_all(&target_dir);
    }

    copy_dir_all(&source_dir, &target_dir).map_err(|e| {
        let _ = fs::remove_dir_all(&temp_dir);
        format!("failed to copy skill files to destination: {e}")
    })?;

    let _ = fs::remove_dir_all(&temp_dir);

    Ok(SkillInfo {
        name: skill.name,
        description: skill.description,
        path: target_dir.join("SKILL.md").to_string_lossy().to_string(),
        scope: input.scope,
        has_scripts: skill.has_scripts,
        has_references: skill.has_references,
    })
}

#[tauri::command]
pub async fn open_skills_folder(scope: String, workspace: Option<String>) -> Result<(), String> {
    let dir = if scope == "project" {
        let ws = workspace.ok_or_else(|| "project scope requires an active workspace".to_string())?;
        PathBuf::from(ws).join(".omp").join("skills")
    } else {
        user_skills_dir()?
    };

    fs::create_dir_all(&dir).map_err(|e| format!("failed to create directory: {e}"))?;
    open::that_detached(&dir).map_err(|e| format!("could not open directory: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frontmatter_extracts_name_and_description() {
        let raw = r#"---
name: code-review
description: "Detailed code auditing rules"
---

# Code Review
Check everything carefully.
"#;
        let (name, desc, body) = parse_skill_markdown(raw);
        assert_eq!(name, "code-review");
        assert_eq!(desc, "Detailed code auditing rules");
        assert!(body.contains("Check everything carefully."));
    }

    #[test]
    fn sanitize_rejects_invalid_names() {
        assert!(sanitize_skill_name("").is_err());
        assert!(sanitize_skill_name("invalid name with spaces").is_err());
        assert!(sanitize_skill_name("invalid/slash").is_err());
        assert_eq!(sanitize_skill_name("My-Skill_123").unwrap(), "my-skill_123");
    }

    #[test]
    fn normalize_github_sources() {
        let (url1, name1) = normalize_github_source("pbakaus/impeccable");
        assert_eq!(url1, "https://github.com/pbakaus/impeccable.git");
        assert_eq!(name1, "impeccable");

        let (url2, name2) = normalize_github_source("https://github.com/pbakaus/impeccable");
        assert_eq!(url2, "https://github.com/pbakaus/impeccable.git");
        assert_eq!(name2, "impeccable");
    }
}
