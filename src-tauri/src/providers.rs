//! Provider management: credentials in `agent.db` and custom providers in `models.yml`.
//!
//! Rule for this module: clean persistence against the engine's real storage files.
//! - Catalog credentials live in `~/.omp/agent/agent.db` (table `auth_credentials`).
//! - Custom endpoints live in `~/.omp/agent/models.yml` (root `providers` map).

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde_json::json;

use crate::dto::{CatalogProviderDto, CustomProviderInput, ProviderAccountDto};

/// Return the agent directory path.
fn resolve_agent_dir() -> Result<PathBuf, String> {
    omp_store::paths::agent_dir()
        .ok_or_else(|| "could not resolve the agent directory (~/.omp/agent)".to_string())
}

/// Path to SQLite `agent.db`.
fn agent_db_path(agent_dir: &Path) -> PathBuf {
    agent_dir.join("agent.db")
}

/// Path to `models.yml`.
fn models_yml_path(agent_dir: &Path) -> PathBuf {
    agent_dir.join("models.yml")
}

/// Built-in catalogue of popular and supported OMP providers.
#[tauri::command]
pub fn get_catalog_providers() -> Vec<CatalogProviderDto> {
    vec![
        CatalogProviderDto {
            id: "openrouter".into(),
            name: "OpenRouter".into(),
            auth_type: "api_key".into(),
            env_var: Some("OPENROUTER_API_KEY".into()),
            description: Some(
                "Unified gateway to Claude, GPT-4, Llama 3, Gemini, DeepSeek, and hundreds more"
                    .into(),
            ),
        },
        CatalogProviderDto {
            id: "anthropic".into(),
            name: "Anthropic Claude".into(),
            auth_type: "oauth_or_key".into(),
            env_var: Some("ANTHROPIC_API_KEY".into()),
            description: Some("Claude 3.7 Sonnet, Claude 3.5 Sonnet, and Claude 3.5 Haiku".into()),
        },
        CatalogProviderDto {
            id: "openai".into(),
            name: "OpenAI".into(),
            auth_type: "api_key".into(),
            env_var: Some("OPENAI_API_KEY".into()),
            description: Some("GPT-4o, o1, o3-mini, and GPT-4.5".into()),
        },
        CatalogProviderDto {
            id: "openai-codex".into(),
            name: "OpenAI Codex".into(),
            auth_type: "oauth".into(),
            env_var: None,
            description: Some("ChatGPT Plus/Team/Pro subscription via OAuth".into()),
        },
        CatalogProviderDto {
            id: "google".into(),
            name: "Google Gemini".into(),
            auth_type: "api_key".into(),
            env_var: Some("GEMINI_API_KEY".into()),
            description: Some(
                "Gemini 2.5 Flash, Gemini 2.5 Pro via Google AI Studio API key".into(),
            ),
        },
        CatalogProviderDto {
            id: "google-antigravity".into(),
            name: "Google Antigravity".into(),
            auth_type: "oauth".into(),
            env_var: None,
            description: Some("Google Cloud Antigravity authentication via browser OAuth".into()),
        },
        CatalogProviderDto {
            id: "deepseek".into(),
            name: "DeepSeek".into(),
            auth_type: "api_key".into(),
            env_var: Some("DEEPSEEK_API_KEY".into()),
            description: Some(
                "DeepSeek V3 and DeepSeek R1 models directly from deepseek.com".into(),
            ),
        },
        CatalogProviderDto {
            id: "groq".into(),
            name: "Groq".into(),
            auth_type: "api_key".into(),
            env_var: Some("GROQ_API_KEY".into()),
            description: Some(
                "Ultra-low-latency Llama 3.3, DeepSeek, and Mixtral inference".into(),
            ),
        },
        CatalogProviderDto {
            id: "xai".into(),
            name: "xAI (Grok)".into(),
            auth_type: "api_key".into(),
            env_var: Some("XAI_API_KEY".into()),
            description: Some("Grok 2 and Grok 2 Vision".into()),
        },
        CatalogProviderDto {
            id: "cerebras".into(),
            name: "Cerebras".into(),
            auth_type: "api_key".into(),
            env_var: Some("CEREBRAS_API_KEY".into()),
            description: Some("Wafer-scale high-throughput Llama inference".into()),
        },
        CatalogProviderDto {
            id: "mistral".into(),
            name: "Mistral AI".into(),
            auth_type: "api_key".into(),
            env_var: Some("MISTRAL_API_KEY".into()),
            description: Some("Mistral Large, Codestral, and Pixtral models".into()),
        },
        CatalogProviderDto {
            id: "github-copilot".into(),
            name: "GitHub Copilot".into(),
            auth_type: "oauth".into(),
            env_var: Some("COPILOT_GITHUB_TOKEN".into()),
            description: Some("GitHub Copilot subscription".into()),
        },
        CatalogProviderDto {
            id: "perplexity".into(),
            name: "Perplexity".into(),
            auth_type: "api_key".into(),
            env_var: Some("PERPLEXITY_API_KEY".into()),
            description: Some("Sonar online reasoning and search models".into()),
        },
        CatalogProviderDto {
            id: "zai".into(),
            name: "z.ai (Zhipu GLM)".into(),
            auth_type: "api_key".into(),
            env_var: Some("ZAI_API_KEY".into()),
            description: Some("GLM-4 and GLM-4V models".into()),
        },
        CatalogProviderDto {
            id: "minimax".into(),
            name: "MiniMax".into(),
            auth_type: "api_key".into(),
            env_var: Some("MINIMAX_API_KEY".into()),
            description: Some("MiniMax Text-01 and MiniMax-ABAB models".into()),
        },
        CatalogProviderDto {
            id: "commandcode".into(),
            name: "Command Code".into(),
            auth_type: "api_key".into(),
            env_var: Some("COMMAND_CODE_API_KEY".into()),
            description: Some("Command Code Provider API models".into()),
        },
        CatalogProviderDto {
            id: "azure".into(),
            name: "Azure OpenAI".into(),
            auth_type: "api_key".into(),
            env_var: Some("AZURE_OPENAI_API_KEY".into()),
            description: Some("Enterprise Azure OpenAI deployments".into()),
        },
    ]
}

/// Retrieve all configured providers:
/// 1. Stored credentials from `agent.db` (`auth_credentials` table).
/// 2. Custom providers from `models.yml`.
#[tauri::command]
pub async fn get_configured_providers() -> Result<Vec<ProviderAccountDto>, String> {
    let agent_dir = resolve_agent_dir()?;
    let mut results: Vec<ProviderAccountDto> = Vec::new();

    // 1. Read SQLite agent.db
    let db_path = agent_db_path(&agent_dir);
    if db_path.is_file() {
        if let Ok(conn) = Connection::open(&db_path) {
            let mut stmt = conn
                .prepare(
                    "SELECT id, provider, credential_type, created_at, disabled_cause FROM auth_credentials ORDER BY provider ASC, id ASC",
                )
                .map_err(|e| format!("failed to prepare statement: {e}"))?;

            let rows = stmt
                .query_map([], |row| {
                    let id: i64 = row.get(0)?;
                    let provider: String = row.get(1)?;
                    let credential_type: String = row.get(2)?;
                    let created_at: Option<i64> = row.get(3)?;
                    let _disabled: Option<String> = row.get(4)?;
                    Ok(ProviderAccountDto {
                        id: Some(id),
                        provider,
                        credential_type,
                        is_custom: false,
                        base_url: None,
                        models: Vec::new(),
                        created_at,
                    })
                })
                .map_err(|e| format!("failed to read auth_credentials: {e}"))?;

            for row in rows.flatten() {
                results.push(row);
            }
        }
    }

    // 2. Read models.yml
    let yml_path = models_yml_path(&agent_dir);
    if yml_path.is_file() {
        if let Ok(contents) = fs::read_to_string(&yml_path) {
            if let Ok(val) = serde_yaml::from_str::<serde_yaml::Value>(&contents) {
                if let Some(providers_map) = val.get("providers").and_then(|p| p.as_mapping()) {
                    for (k, v) in providers_map {
                        if let Some(provider_id) = k.as_str() {
                            let base_url = v
                                .get("baseUrl")
                                .and_then(|u| u.as_str())
                                .map(|s| s.to_string());

                            let mut models = Vec::new();
                            if let Some(models_seq) = v.get("models").and_then(|m| m.as_sequence())
                            {
                                for m in models_seq {
                                    if let Some(mid) = m.get("id").and_then(|id| id.as_str()) {
                                        models.push(mid.to_string());
                                    }
                                }
                            }

                            results.push(ProviderAccountDto {
                                id: None,
                                provider: provider_id.to_string(),
                                credential_type: "custom".to_string(),
                                is_custom: true,
                                base_url,
                                models,
                                created_at: None,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Store or update an API key in `agent.db` for a provider.
#[tauri::command]
pub async fn add_provider_api_key(provider: String, api_key: String) -> Result<(), String> {
    let provider = provider.trim();
    let api_key = api_key.trim();

    if provider.is_empty() {
        return Err("provider id cannot be empty".to_string());
    }
    if api_key.is_empty() {
        return Err("api key cannot be empty".to_string());
    }

    let agent_dir = resolve_agent_dir()?;
    if !agent_dir.is_dir() {
        fs::create_dir_all(&agent_dir)
            .map_err(|e| format!("failed to create agent directory: {e}"))?;
    }

    let db_path = agent_db_path(&agent_dir);
    let conn = Connection::open(&db_path).map_err(|e| format!("failed to open agent.db: {e}"))?;

    // Ensure the table exists in case of fresh setup
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS auth_credentials (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            provider TEXT NOT NULL,
            credential_type TEXT NOT NULL,
            data TEXT NOT NULL,
            disabled_cause TEXT DEFAULT NULL,
            identity_key TEXT DEFAULT NULL,
            created_at INTEGER NOT NULL DEFAULT (CAST(strftime('%s','now') AS INTEGER)),
            updated_at INTEGER NOT NULL DEFAULT (CAST(strftime('%s','now') AS INTEGER))
        );
        CREATE INDEX IF NOT EXISTS idx_auth_provider ON auth_credentials(provider);",
    )
    .map_err(|e| format!("failed to initialize auth_credentials table: {e}"))?;

    let payload = json!({
        "key": api_key,
        "source": "login",
        "type": "api_key"
    })
    .to_string();

    // Check if an existing api_key credential exists for this provider
    let existing_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM auth_credentials WHERE provider = ?1 AND credential_type = 'api_key' LIMIT 1",
            params![provider],
            |row| row.get(0),
        )
        .ok();

    if let Some(id) = existing_id {
        conn.execute(
            "UPDATE auth_credentials SET data = ?1, updated_at = CAST(strftime('%s','now') AS INTEGER), disabled_cause = NULL WHERE id = ?2",
            params![payload, id],
        )
        .map_err(|e| format!("failed to update credential: {e}"))?;
    } else {
        conn.execute(
            "INSERT INTO auth_credentials (provider, credential_type, data, created_at, updated_at) VALUES (?1, 'api_key', ?2, CAST(strftime('%s','now') AS INTEGER), CAST(strftime('%s','now') AS INTEGER))",
            params![provider, payload],
        )
        .map_err(|e| format!("failed to insert credential: {e}"))?;
    }

    Ok(())
}

/// Remove a credential record by its SQLite ID.
#[tauri::command]
pub async fn remove_provider_credential(id: i64) -> Result<(), String> {
    let agent_dir = resolve_agent_dir()?;
    let db_path = agent_db_path(&agent_dir);
    if !db_path.is_file() {
        return Ok(());
    }

    let conn = Connection::open(&db_path).map_err(|e| format!("failed to open agent.db: {e}"))?;

    conn.execute("DELETE FROM auth_credentials WHERE id = ?1", params![id])
        .map_err(|e| format!("failed to delete credential: {e}"))?;

    Ok(())
}

/// Add or update a custom provider in `~/.omp/agent/models.yml`.
#[tauri::command]
pub async fn add_custom_provider(input: CustomProviderInput) -> Result<(), String> {
    let id = input.id.trim();
    let base_url = input.base_url.trim();
    let model_id = input.model_id.trim();

    if id.is_empty() {
        return Err("provider id cannot be empty".to_string());
    }
    if base_url.is_empty() {
        return Err("base URL cannot be empty".to_string());
    }
    if model_id.is_empty() {
        return Err("model id cannot be empty".to_string());
    }

    let agent_dir = resolve_agent_dir()?;
    if !agent_dir.is_dir() {
        fs::create_dir_all(&agent_dir)
            .map_err(|e| format!("failed to create agent directory: {e}"))?;
    }

    let yml_path = models_yml_path(&agent_dir);
    let mut root: serde_yaml::Value = if yml_path.is_file() {
        let text = fs::read_to_string(&yml_path).unwrap_or_default();
        serde_yaml::from_str(&text).unwrap_or_else(|_| serde_yaml::Mapping::new().into())
    } else {
        serde_yaml::Mapping::new().into()
    };

    if !root.is_mapping() {
        root = serde_yaml::Mapping::new().into();
    }

    let root_map = root.as_mapping_mut().unwrap();
    let providers_key = serde_yaml::Value::String("providers".into());
    if !root_map.contains_key(&providers_key) {
        root_map.insert(providers_key.clone(), serde_yaml::Mapping::new().into());
    }

    let providers_val = root_map.get_mut(&providers_key).unwrap();
    if !providers_val.is_mapping() {
        *providers_val = serde_yaml::Mapping::new().into();
    }
    let providers_map = providers_val.as_mapping_mut().unwrap();

    let api_format = input.api.as_deref().unwrap_or("openai-completions");
    let model_name = input
        .model_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(model_id);

    let mut provider_entry = serde_yaml::Mapping::new();
    provider_entry.insert(
        serde_yaml::Value::String("api".into()),
        serde_yaml::Value::String(api_format.into()),
    );
    provider_entry.insert(
        serde_yaml::Value::String("baseUrl".into()),
        serde_yaml::Value::String(base_url.into()),
    );

    if let Some(key) = input.api_key.as_deref().filter(|k| !k.trim().is_empty()) {
        provider_entry.insert(
            serde_yaml::Value::String("auth".into()),
            serde_yaml::Value::String("apiKey".into()),
        );
        provider_entry.insert(
            serde_yaml::Value::String("apiKey".into()),
            serde_yaml::Value::String(key.trim().into()),
        );
    } else {
        provider_entry.insert(
            serde_yaml::Value::String("auth".into()),
            serde_yaml::Value::String("none".into()),
        );
    }

    // Build the model list entry
    let mut model_entry = serde_yaml::Mapping::new();
    model_entry.insert(
        serde_yaml::Value::String("id".into()),
        serde_yaml::Value::String(model_id.into()),
    );
    model_entry.insert(
        serde_yaml::Value::String("name".into()),
        serde_yaml::Value::String(model_name.into()),
    );
    model_entry.insert(
        serde_yaml::Value::String("reasoning".into()),
        serde_yaml::Value::Bool(true),
    );
    model_entry.insert(
        serde_yaml::Value::String("input".into()),
        serde_yaml::Value::Sequence(vec![
            serde_yaml::Value::String("text".into()),
            serde_yaml::Value::String("image".into()),
        ]),
    );
    model_entry.insert(
        serde_yaml::Value::String("contextWindow".into()),
        serde_yaml::Value::Number(128000.into()),
    );
    model_entry.insert(
        serde_yaml::Value::String("maxTokens".into()),
        serde_yaml::Value::Number(16384.into()),
    );

    provider_entry.insert(
        serde_yaml::Value::String("models".into()),
        serde_yaml::Value::Sequence(vec![serde_yaml::Value::Mapping(model_entry)]),
    );

    providers_map.insert(
        serde_yaml::Value::String(id.into()),
        serde_yaml::Value::Mapping(provider_entry),
    );

    let serialized =
        serde_yaml::to_string(&root).map_err(|e| format!("failed to format models.yml: {e}"))?;

    fs::write(&yml_path, serialized).map_err(|e| format!("failed to write models.yml: {e}"))?;

    Ok(())
}

/// Remove a custom provider from `~/.omp/agent/models.yml`.
#[tauri::command]
pub async fn remove_custom_provider(provider_id: String) -> Result<(), String> {
    let agent_dir = resolve_agent_dir()?;
    let yml_path = models_yml_path(&agent_dir);
    if !yml_path.is_file() {
        return Ok(());
    }

    let contents = fs::read_to_string(&yml_path).unwrap_or_default();
    let mut root: serde_yaml::Value =
        serde_yaml::from_str(&contents).map_err(|e| format!("failed to parse models.yml: {e}"))?;

    if let Some(providers_map) = root.get_mut("providers").and_then(|p| p.as_mapping_mut()) {
        let key = serde_yaml::Value::String(provider_id);
        providers_map.remove(&key);
    }

    let serialized =
        serde_yaml::to_string(&root).map_err(|e| format!("failed to serialize models.yml: {e}"))?;

    fs::write(&yml_path, serialized).map_err(|e| format!("failed to write models.yml: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_providers_lists_major_providers() {
        let providers = get_catalog_providers();
        assert!(!providers.is_empty(), "catalogue must not be empty");

        let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"openrouter"), "must include openrouter");
        assert!(ids.contains(&"anthropic"), "must include anthropic");
        assert!(ids.contains(&"openai"), "must include openai");
        assert!(ids.contains(&"deepseek"), "must include deepseek");
    }

    #[test]
    fn custom_provider_yaml_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("omp-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let yml_path = temp_dir.join("models.yml");

        let mut root = serde_yaml::Mapping::new();
        let mut providers = serde_yaml::Mapping::new();
        let mut prov = serde_yaml::Mapping::new();
        prov.insert(
            serde_yaml::Value::String("baseUrl".into()),
            serde_yaml::Value::String("http://127.0.0.1:8080".into()),
        );
        providers.insert(
            serde_yaml::Value::String("my-test".into()),
            serde_yaml::Value::Mapping(prov),
        );
        root.insert(
            serde_yaml::Value::String("providers".into()),
            serde_yaml::Value::Mapping(providers),
        );

        let s = serde_yaml::to_string(&root).expect("serializes");
        fs::write(&yml_path, &s).expect("writes");

        let read_back: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(&yml_path).unwrap()).expect("deserializes");
        assert!(read_back.get("providers").is_some());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
