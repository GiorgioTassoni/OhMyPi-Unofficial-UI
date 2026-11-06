use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub const SERVER_NAME: &str = "design-choice";

pub fn resolve_user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub fn candidate_mcp_config_paths(workspace: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(ws) = workspace {
        paths.push(ws.join(".mcp.json"));
        paths.push(ws.join("mcp.json"));
    }
    if let Some(home) = resolve_user_home() {
        paths.push(home.join(".omp").join("agent").join("mcp.json"));
        paths.push(home.join(".omp").join("agent").join(".mcp.json"));
        paths.push(home.join(".config").join("omp").join("mcp.json"));
        paths.push(home.join(".omp").join("mcp.json"));
        paths.push(home.join(".cursor").join("mcp.json"));
    }
    paths
}

pub fn resolve_script_path(workspace: Option<&Path>) -> Option<PathBuf> {
    if let Some(home) = resolve_user_home() {
        let in_home = home
            .join(".omp")
            .join("tools")
            .join("mcp-design-choice")
            .join("src")
            .join("index.ts");
        if in_home.exists() {
            return Some(in_home);
        }
    }
    if let Some(ws) = workspace {
        let in_ws = ws
            .join("tools")
            .join("mcp-design-choice")
            .join("src")
            .join("index.ts");
        if in_ws.exists() {
            return Some(in_ws.canonicalize().unwrap_or(in_ws));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let in_cwd = cwd
            .join("tools")
            .join("mcp-design-choice")
            .join("src")
            .join("index.ts");
        if in_cwd.exists() {
            return Some(in_cwd.canonicalize().unwrap_or(in_cwd));
        }
    }
    resolve_user_home().map(|h| {
        h.join(".omp")
            .join("tools")
            .join("mcp-design-choice")
            .join("src")
            .join("index.ts")
    })
}

pub fn update_mcp_file(
    path: &Path,
    enabled: bool,
    script_path: Option<&Path>,
) -> Result<(), String> {
    let mut root: Value = if path.exists() {
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    if !root.is_object() {
        root = json!({});
    }

    let servers = root
        .as_object_mut()
        .ok_or_else(|| "invalid json root".to_string())?
        .entry("mcpServers")
        .or_insert_with(|| json!({}));

    if !servers.is_object() {
        *servers = json!({});
    }

    let servers_obj = servers
        .as_object_mut()
        .ok_or_else(|| "mcpServers is not an object".to_string())?;

    if enabled {
        let script = script_path
            .map(|p| p.to_string_lossy().to_string())
            .or_else(|| resolve_script_path(None).map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| "tools/mcp-design-choice/src/index.ts".to_string());
        servers_obj.insert(
            SERVER_NAME.to_string(),
            json!({
                "command": "bun",
                "args": ["run", script],
                "timeout": 3600000
            }),
        );
    } else {
        servers_obj.remove(SERVER_NAME);
    }

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let formatted = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    fs::write(path, formatted).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn sync_all_configs(enabled: bool, workspace: Option<&Path>) -> Result<(), String> {
    let script = resolve_script_path(workspace);
    let mut updated_any = false;
    for path in candidate_mcp_config_paths(workspace) {
        if enabled || path.exists() {
            if let Ok(()) = update_mcp_file(&path, enabled, script.as_deref()) {
                updated_any = true;
            }
        }
    }
    if !updated_any && enabled {
        if let Some(home) = resolve_user_home() {
            let primary = home.join(".omp").join("agent").join("mcp.json");
            update_mcp_file(&primary, true, script.as_deref())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn sync_mcp_design_choice(
    enabled: bool,
    workspace: Option<String>,
) -> Result<(), String> {
    let ws_path = workspace.as_deref().map(Path::new);
    sync_all_configs(enabled, ws_path)
}

#[tauri::command]
pub fn get_mcp_design_choice_status(workspace: Option<String>) -> Result<bool, String> {
    let ws_path = workspace.as_deref().map(Path::new);
    for path in candidate_mcp_config_paths(ws_path) {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(val) = serde_json::from_str::<Value>(&content) {
                    if let Some(servers) = val.get("mcpServers").and_then(|s| s.as_object()) {
                        if servers.contains_key(SERVER_NAME) {
                            return Ok(true);
                        }
                    }
                }
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_mcp_file_enable_and_disable() {
        let temp_dir = std::env::temp_dir().join(format!(
            "test_mcp_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let file_path = temp_dir.join(".mcp.json");
        let script = Path::new("/custom/path/to/script.ts");

        let res = update_mcp_file(&file_path, true, Some(script));
        assert!(res.is_ok());

        let content = fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("design-choice"));
        assert!(content.contains("/custom/path/to/script.ts"));

        let res_disable = update_mcp_file(&file_path, false, None);
        assert!(res_disable.is_ok());

        let content_after = fs::read_to_string(&file_path).unwrap();
        assert!(!content_after.contains("design-choice"));

        let _ = fs::remove_dir_all(temp_dir);
    }
}
