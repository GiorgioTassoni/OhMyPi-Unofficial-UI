//! Live provider connection probing and custom provider configuration parsing.
//!
//! Provides fail-closed live network probes for local / self-hosted LLM endpoints
//! (e.g. llama.cpp, vLLM, Ollama), OAuth session health checks (e.g. ChatGPT / OpenAI Codex),
//! and CLI-mediated credential tests for standard cloud providers.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use serde::Deserialize;

use crate::dto::ProviderTestResultDto;
use crate::providers::resolve_agent_dir;

#[derive(Debug, Clone, Deserialize)]
pub struct CustomProviderConfig {
    pub api: Option<String>,
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    pub auth: Option<String>,
    #[serde(default)]
    pub models: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ModelsYmlRoot {
    #[serde(default)]
    pub providers: HashMap<String, CustomProviderConfig>,
}

/// Canonical reader for `models.yml` custom providers configuration.
///
/// Prevents manual `serde_yaml` AST navigation duplication across models and providers.
pub fn read_custom_providers_config(agent_dir: &Path) -> HashMap<String, CustomProviderConfig> {
    let yml_path = agent_dir.join("models.yml");
    if !yml_path.is_file() {
        return HashMap::new();
    }
    let contents = match std::fs::read_to_string(&yml_path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    serde_yaml::from_str::<ModelsYmlRoot>(&contents)
        .map(|r| r.providers)
        .unwrap_or_default()
}

/// Probes a custom OpenAI-compatible HTTP endpoint (e.g. llama.cpp, vLLM, Ollama)
/// with a 4s connection timeout.
async fn probe_custom_endpoint(
    base_url: &str,
    api_key: Option<&str>,
    start: Instant,
) -> Result<ProviderTestResultDto, String> {
    let probe_url = if base_url.ends_with("/v1") || base_url.ends_with("/v1/") {
        format!("{}/models", base_url.trim_end_matches('/'))
    } else {
        format!("{}/v1/models", base_url.trim_end_matches('/'))
    };

    let mut args = vec![
        "-s".to_string(),
        "-S".to_string(),
        "-m".to_string(),
        "4".to_string(),
        "-w".to_string(),
        "\n%{http_code}".to_string(),
    ];

    if let Some(key) = api_key {
        if key != "none" && !key.trim().is_empty() {
            args.push("-H".to_string());
            args.push(format!("Authorization: Bearer {key}"));
        }
    }

    args.push(probe_url);

    let curl_output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new("curl").args(&args).output(),
    )
    .await;

    let latency_ms = start.elapsed().as_millis() as u64;

    match curl_output {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                let err = if !stderr.trim().is_empty() {
                    stderr.trim().to_string()
                } else {
                    "Connection refused: server is offline or unreachable".to_string()
                };
                return Ok(ProviderTestResultDto {
                    ok: false,
                    latency_ms,
                    model_count: 0,
                    sample_models: Vec::new(),
                    message: format!("Server offline ({err})"),
                });
            }

            let mut lines: Vec<&str> = stdout.trim().lines().collect();
            let http_code = lines.pop().unwrap_or("000");
            let body = lines.join("\n");

            if http_code == "200" {
                let mut sample_models = Vec::new();
                let mut model_count = 0;

                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body) {
                    if let Some(data) = val.get("data").and_then(|d| d.as_array()) {
                        model_count = data.len();
                        for item in data.iter().take(5) {
                            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                                sample_models.push(id.to_string());
                            }
                        }
                    } else if let Some(models) = val.get("models").and_then(|m| m.as_array()) {
                        model_count = models.len();
                        for item in models.iter().take(5) {
                            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                                sample_models.push(id.to_string());
                            }
                        }
                    }
                }

                if model_count == 0 {
                    model_count = 1;
                }

                Ok(ProviderTestResultDto {
                    ok: true,
                    latency_ms,
                    model_count,
                    sample_models,
                    message: format!(
                        "Connected (HTTP 200, {model_count} model{} available)",
                        if model_count == 1 { "" } else { "s" }
                    ),
                })
            } else if http_code == "401" || http_code == "403" {
                Ok(ProviderTestResultDto {
                    ok: false,
                    latency_ms,
                    model_count: 0,
                    sample_models: Vec::new(),
                    message: format!("Authentication failed (HTTP {http_code})"),
                })
            } else {
                Ok(ProviderTestResultDto {
                    ok: false,
                    latency_ms,
                    model_count: 0,
                    sample_models: Vec::new(),
                    message: format!("Server returned HTTP {http_code}"),
                })
            }
        }
        Ok(Err(err)) => Ok(ProviderTestResultDto {
            ok: false,
            latency_ms,
            model_count: 0,
            sample_models: Vec::new(),
            message: format!("Failed to run connection probe: {err}"),
        }),
        Err(_) => Ok(ProviderTestResultDto {
            ok: false,
            latency_ms: 5000,
            model_count: 0,
            sample_models: Vec::new(),
            message: "Connection timed out after 5 seconds: server offline".to_string(),
        }),
    }
}

/// Fallback probe for catalog providers: verify credentials and model listing via `omp` CLI.
async fn probe_omp_models(
    omp_bin: &Path,
    provider: &str,
    start: Instant,
) -> Result<ProviderTestResultDto, String> {
    let token_output = tokio::process::Command::new(omp_bin)
        .args(["token", provider])
        .output()
        .await;

    let token_err = match token_output {
        Ok(out) if !out.status.success() => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if !err.is_empty() {
                Some(err.lines().next().unwrap_or(&err).to_string())
            } else {
                Some("Authentication failed: invalid or missing credentials".to_string())
            }
        }
        _ => None,
    };

    if let Some(err_msg) = token_err {
        let latency_ms = start.elapsed().as_millis() as u64;
        return Ok(ProviderTestResultDto {
            ok: false,
            latency_ms,
            model_count: 0,
            sample_models: Vec::new(),
            message: err_msg,
        });
    }

    let cmd_result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::process::Command::new(omp_bin)
            .args(["models", provider, "--json"])
            .output(),
    )
    .await;

    let latency_ms = start.elapsed().as_millis() as u64;

    match cmd_result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&stdout) {
                if let Some(models) = val.get("models").and_then(|m| m.as_array()) {
                    let mut sample_models = Vec::new();
                    for m in models.iter().take(5) {
                        if let Some(id) = m.get("id").and_then(|v| v.as_str()) {
                            sample_models.push(id.to_string());
                        }
                    }

                    if !models.is_empty() {
                        let count = models.len();
                        return Ok(ProviderTestResultDto {
                            ok: true,
                            latency_ms,
                            model_count: count,
                            sample_models,
                            message: format!(
                                "Connected ({count} model{} available)",
                                if count == 1 { "" } else { "s" }
                            ),
                        });
                    }
                }
            }

            let err_msg = if !stderr.trim().is_empty() {
                stderr.lines().next().unwrap_or("").trim().to_string()
            } else {
                "No models reported by provider or endpoint unreachable".to_string()
            };

            Ok(ProviderTestResultDto {
                ok: false,
                latency_ms,
                model_count: 0,
                sample_models: Vec::new(),
                message: err_msg,
            })
        }
        Ok(Err(err)) => Ok(ProviderTestResultDto {
            ok: false,
            latency_ms,
            model_count: 0,
            sample_models: Vec::new(),
            message: format!("Failed to launch omp runner: {err}"),
        }),
        Err(_) => Ok(ProviderTestResultDto {
            ok: false,
            latency_ms: 15000,
            model_count: 0,
            sample_models: Vec::new(),
            message: "Connection timed out after 15 seconds".to_string(),
        }),
    }
}

/// Test connection and authentication for a provider.
#[tauri::command]
pub async fn test_provider_connection(provider: String) -> Result<ProviderTestResultDto, String> {
    let provider = provider.trim().to_string();
    if provider.is_empty() {
        return Err("provider id cannot be empty".to_string());
    }

    let start = Instant::now();
    let agent_dir = resolve_agent_dir().ok();

    // 1. Check if provider is a custom endpoint defined in models.yml with a baseUrl
    if let Some(ref dir) = agent_dir {
        let configs = read_custom_providers_config(dir);
        if let Some(cfg) = configs.get(&provider) {
            if let Some(ref base_url) = cfg.base_url {
                return probe_custom_endpoint(base_url, cfg.api_key.as_deref(), start).await;
            }
        }
    }

    let omp_bin = omp_transport::resolve_omp_binary();

    // 2. Query provider models dynamically via omp CLI (works for OAuth & API keys)
    probe_omp_models(&omp_bin, &provider, start).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn read_custom_providers_config_parses_models_yml() {
        let temp_dir = std::env::temp_dir().join(format!("omp-test-probe-{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let yml_path = temp_dir.join("models.yml");

        let content = r#"
providers:
  llama.cpp:
    baseUrl: "http://127.0.0.1:8080"
    api: "openai-completions"
    apiKey: "none"
    models:
      - id: "qwen2.5-coder-32b"
        name: "Qwen 2.5 Coder 32B"
  openai-codex:
    baseUrl: "https://chatgpt.com/backend-api"
    api: "openai-codex-responses"
    auth: "oauth"
"#;
        fs::write(&yml_path, content).expect("writes yaml");

        let configs = read_custom_providers_config(&temp_dir);
        assert_eq!(configs.len(), 2);
        assert!(configs.contains_key("llama.cpp"));
        assert_eq!(
            configs["llama.cpp"].base_url.as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(configs["llama.cpp"].models.len(), 1);
        assert_eq!(configs["openai-codex"].auth.as_deref(), Some("oauth"));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
