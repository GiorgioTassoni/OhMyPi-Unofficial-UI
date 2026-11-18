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
    let end_idx = after_first
        .find("\n---")
        .or_else(|| after_first.find("\r\n---"));
    let Some(idx) = end_idx else {
        return (String::new(), String::new(), raw.to_string());
    };

    let yaml_block = &after_first[..idx];
    let body_start = idx
        + if after_first[idx..].starts_with("\r\n---") {
            5
        } else {
            4
        };
    let remainder = &after_first[body_start..];
    let body = remainder.trim_start_matches(['\r', '\n']).to_string();

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
    Ok(PathBuf::from(home)
        .join(".omp")
        .join("agent")
        .join("skills"))
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
        let ws =
            workspace.ok_or_else(|| "project scope requires an active workspace".to_string())?;
        Ok(PathBuf::from(ws)
            .join(".omp")
            .join("skills")
            .join(sanitized))
    } else {
        Ok(user_skills_dir()?.join(sanitized))
    }
}

pub fn sanitize_skill_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("skill name cannot be empty".to_string());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(
            "skill name may only contain alphanumeric characters, hyphens, and underscores"
                .to_string(),
        );
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
            let final_name = if name.trim().is_empty() {
                dir_name
            } else {
                name
            };

            let has_scripts = path.join("scripts").is_dir();
            let has_references =
                path.join("reference").is_dir() || path.join("references").is_dir();

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

/// Copy directory recursively, skipping symlinks so untrusted repos cannot copy host files.
pub fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            continue;
        }
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dest_path)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// Normalize GitHub URL or shorthand to a cloneable git URL and a repo name.
/// Rejects empty inputs and strings starting with a dash to prevent git CLI argument injection.
pub fn normalize_github_source(source: &str) -> Result<(String, String), String> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Err("repository source cannot be empty".to_string());
    }
    if trimmed.starts_with('-') {
        return Err("invalid repository source: cannot start with a dash".to_string());
    }
    if trimmed.starts_with("https://github.com/") || trimmed.starts_with("http://github.com/") {
        let stripped = trimmed.trim_end_matches(".git");
        let parts: Vec<&str> = stripped.split('/').collect();
        let name = parts.last().copied().unwrap_or("repository");
        Ok((format!("{stripped}.git"), name.to_string()))
    } else if trimmed.contains('/') && !trimmed.contains(':') && !trimmed.starts_with("http") {
        // e.g. "pbakaus/impeccable"
        let parts: Vec<&str> = trimmed.split('/').collect();
        let name = parts.get(1).copied().unwrap_or("repository");
        Ok((
            format!("https://github.com/{trimmed}.git"),
            name.to_string(),
        ))
    } else {
        Ok((trimmed.to_string(), "repository".to_string()))
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
                root.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "skill".to_string())
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
                    let final_name = if name.is_empty() {
                        dir_name.clone()
                    } else {
                        name
                    };

                    let rel = path
                        .strip_prefix(root)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| dir_name);

                    detected.push(DetectedSkill {
                        name: final_name,
                        description: desc,
                        relative_path: rel,
                        has_scripts: path.join("scripts").is_dir(),
                        has_references: path.join("reference").is_dir()
                            || path.join("references").is_dir(),
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

#[derive(serde::Serialize)]
struct NewSkillFrontmatter<'a> {
    name: &'a str,
    description: &'a str,
}

fn format_skill_markdown(name: &str, description: &str, body: &str) -> Result<String, String> {
    let yaml = serde_yaml::to_string(&NewSkillFrontmatter {
        name,
        description: description.trim(),
    })
    .map_err(|e| format!("failed to serialize skill frontmatter: {e}"))?;
    let yaml_body = yaml.trim_start_matches("---\n").trim_end();
    Ok(format!("---\n{yaml_body}\n---\n\n{}\n", body.trim()))
}

#[tauri::command]
pub async fn list_skills(workspace: Option<String>) -> Result<Vec<SkillInfo>, String> {
    tokio::task::spawn_blocking(move || list_skills_sync(workspace))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

fn list_skills_sync(workspace: Option<String>) -> Result<Vec<SkillInfo>, String> {
    let mut all = Vec::new();

    // 1. User skills
    if let Ok(user_dir) = user_skills_dir() {
        all.extend(scan_skills_in_dir(&user_dir, "user"));
    }

    // 2. Project skills
    if let Some(ws) = &workspace {
        let ws_path = Path::new(ws);
        all.extend(scan_skills_in_dir(
            &ws_path.join(".omp").join("skills"),
            "project",
        ));
        all.extend(scan_skills_in_dir(
            &ws_path.join(".agents").join("skills"),
            "project",
        ));
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
    tokio::task::spawn_blocking(move || create_custom_skill_sync(input))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

fn create_custom_skill_sync(input: CreateSkillInput) -> Result<SkillInfo, String> {
    let name = sanitize_skill_name(&input.name)?;
    let target_dir = resolve_target_skill_dir(&input.scope, &name, input.workspace.as_deref())?;

    fs::create_dir_all(&target_dir)
        .map_err(|e| format!("failed to create skill directory: {e}"))?;

    let full_content = format_skill_markdown(&name, &input.description, &input.content)?;

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

/// Clone a remote repository into a secure, unique temporary directory and execute an action on it.
/// Automatically cleans up the directory on drop (RAII) and uses `--` argument isolation with a timeout.
fn with_cloned_repo<T, F>(source_url: &str, f: F) -> Result<T, String>
where
    F: FnOnce(&Path, &str) -> Result<T, String>,
{
    let (git_url, repo_name) = normalize_github_source(source_url)?;

    let temp_dir = tempfile::Builder::new()
        .prefix("omp-skill-")
        .tempdir()
        .map_err(|e| format!("failed to create temporary directory: {e}"))?;

    let mut child = Command::new("git")
        .args(["clone", "--depth", "1", "--", &git_url])
        .arg(temp_dir.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run git clone: {e}"))?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("git clone timed out after 30 seconds".to_string());
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(format!("failed waiting for git clone: {e}")),
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to collect git clone output: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git clone failed: {stderr}"));
    }

    f(temp_dir.path(), &repo_name)
}

#[tauri::command]
pub async fn inspect_remote_skill(source_url: String) -> Result<InspectSkillResult, String> {
    tokio::task::spawn_blocking(move || {
        with_cloned_repo(&source_url, |temp_path, repo_name| {
            let skills = find_skills_in_tree(temp_path);
            if skills.is_empty() {
                return Err("no skills (SKILL.md) found in repository".to_string());
            }

            Ok(InspectSkillResult {
                repo_name: repo_name.to_string(),
                skills,
            })
        })
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
pub async fn import_remote_skill(input: ImportSkillInput) -> Result<SkillInfo, String> {
    tokio::task::spawn_blocking(move || import_remote_skill_sync(input))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

fn import_remote_skill_sync(input: ImportSkillInput) -> Result<SkillInfo, String> {
    let target_scope = input.scope.clone();
    let target_workspace = input.workspace.clone();
    let target_skill_name = input.skill_name.clone();

    with_cloned_repo(&input.source_url, |temp_path, _| {
        let skills = find_skills_in_tree(temp_path);
        if skills.is_empty() {
            return Err("no skills (SKILL.md) found in repository".to_string());
        }

        let selected = if let Some(target_name) = &target_skill_name {
            skills.into_iter().find(|s| s.name == *target_name)
        } else {
            skills.into_iter().next()
        };

        let Some(skill) = selected else {
            return Err("specified skill was not found in repository".to_string());
        };

        let target_dir =
            resolve_target_skill_dir(&target_scope, &skill.name, target_workspace.as_deref())?;
        let source_dir = if skill.relative_path == "." {
            temp_path.to_path_buf()
        } else {
            temp_path.join(&skill.relative_path)
        };

        let canonical_temp = temp_path
            .canonicalize()
            .map_err(|e| format!("failed to canonicalize temp repo: {e}"))?;
        let canonical_source = source_dir
            .canonicalize()
            .map_err(|e| format!("failed to canonicalize skill source directory: {e}"))?;
        if !canonical_source.starts_with(&canonical_temp) {
            return Err("skill source directory escapes cloned repository".to_string());
        }

        if target_dir.exists() {
            let _ = fs::remove_dir_all(&target_dir);
        }

        copy_dir_all(&canonical_source, &target_dir)
            .map_err(|e| format!("failed to copy skill files to destination: {e}"))?;

        Ok(SkillInfo {
            name: skill.name,
            description: skill.description,
            path: target_dir.join("SKILL.md").to_string_lossy().to_string(),
            scope: target_scope,
            has_scripts: skill.has_scripts,
            has_references: skill.has_references,
        })
    })
}

#[tauri::command]
pub async fn open_skills_folder(scope: String, workspace: Option<String>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let dir = if scope == "project" {
            let ws = workspace
                .ok_or_else(|| "project scope requires an active workspace".to_string())?;
            PathBuf::from(ws).join(".omp").join("skills")
        } else {
            user_skills_dir()?
        };

        fs::create_dir_all(&dir).map_err(|e| format!("failed to create directory: {e}"))?;
        open::that_detached(&dir).map_err(|e| format!("could not open directory: {e}"))
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
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
        let (url1, name1) = normalize_github_source("pbakaus/impeccable").unwrap();
        assert_eq!(url1, "https://github.com/pbakaus/impeccable.git");
        assert_eq!(name1, "impeccable");

        let (url2, name2) =
            normalize_github_source("https://github.com/pbakaus/impeccable").unwrap();
        assert_eq!(url2, "https://github.com/pbakaus/impeccable.git");
        assert_eq!(name2, "impeccable");

        assert!(normalize_github_source("").is_err());
        assert!(normalize_github_source("   ").is_err());
        assert!(normalize_github_source("--upload-pack=evil").is_err());
        assert!(normalize_github_source("-oProxyCommand=calc").is_err());
    }

    #[test]
    fn format_skill_markdown_handles_multiline_descriptions() {
        let md = format_skill_markdown("my-skill", "Line one\nLine \"two\"", "Body text").unwrap();
        let (name, desc, body) = parse_skill_markdown(&md);
        assert_eq!(name, "my-skill");
        assert!(desc.contains("Line one"));
        assert!(desc.contains("Line \"two\""));
        assert_eq!(body.trim(), "Body text");
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_all_skips_symlinks() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), "secret").unwrap();
        fs::write(src.path().join("SKILL.md"), "ok").unwrap();
        std::os::unix::fs::symlink(outside.path(), src.path().join("leak.txt")).unwrap();

        copy_dir_all(src.path(), dst.path()).unwrap();
        assert!(dst.path().join("SKILL.md").is_file());
        assert!(!dst.path().join("leak.txt").exists());
    }
}
