//! Per-model OpenRouter execution-provider routing and public endpoint discovery.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_yaml::{Mapping, Value};

const ENDPOINT_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RESPONSE_BYTES: &str = "2097152";
const PROVIDERS: &str = "providers";
const OPENROUTER: &str = "openrouter";
const MODEL_OVERRIDES: &str = "modelOverrides";
const ONLY: &str = "only";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterProvider {
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterRouting {
    pub model_id: String,
    pub providers: Vec<String>,
}

#[tauri::command]
pub fn get_openrouter_routing(model_id: String) -> Result<Option<Vec<String>>, String> {
    validate_model_id(&model_id)?;
    let agent_dir = crate::providers::resolve_agent_dir()?;
    read_routing(&models_yaml_path(&agent_dir), &model_id)
}

#[tauri::command]
pub fn list_openrouter_routings() -> Result<Vec<OpenRouterRouting>, String> {
    let agent_dir = crate::providers::resolve_agent_dir()?;
    read_routings(&models_yaml_path(&agent_dir))
}

#[tauri::command]
pub async fn get_openrouter_providers(
    model_id: String,
) -> Result<Vec<OpenRouterProvider>, String> {
    validate_model_id(&model_id)?;
    let url = format!("https://openrouter.ai/api/v1/models/{model_id}/endpoints");
    let output = tokio::time::timeout(
        ENDPOINT_TIMEOUT,
        tokio::process::Command::new("curl")
            .args([
                "--disable",
                "--silent",
                "--show-error",
                "--max-time",
                "8",
                "--max-filesize",
                MAX_RESPONSE_BYTES,
                "--write-out",
                "\\n%{http_code}",
                "--proto",
                "=https",
                "--",
                url.as_str(),
            ])
            .output(),
    )
    .await
    .map_err(|_| "OpenRouter endpoint request timed out after 8 seconds".to_string())?
    .map_err(|error| format!("could not run curl for OpenRouter endpoints: {error}"))?;

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(if detail.trim().is_empty() {
            format!("OpenRouter endpoint request failed ({})", output.status)
        } else {
            format!("OpenRouter endpoint request failed: {}", detail.trim())
        });
    }

    let response = String::from_utf8(output.stdout)
        .map_err(|error| format!("OpenRouter endpoint response was not UTF-8: {error}"))?;
    let (body, status) = response.rsplit_once('\n').ok_or_else(|| {
        "OpenRouter endpoint response did not include an HTTP status".to_string()
    })?;
    if status != "200" {
        return Err(format!("OpenRouter endpoint request returned HTTP {status}"));
    }
    providers_from_response(body)
}

#[tauri::command]
pub fn set_openrouter_routing(
    model_id: String,
    providers: Option<Vec<String>>,
) -> Result<(), String> {
    validate_model_id(&model_id)?;
    let providers = providers.map(normalize_provider_slugs).transpose()?;
    let agent_dir = crate::providers::resolve_agent_dir()?;
    set_routing(
        &models_yaml_path(&agent_dir),
        &model_id,
        providers.as_deref(),
    )
}

fn models_yaml_path(agent_dir: &Path) -> PathBuf {
    agent_dir.join("models.yml")
}

fn validate_model_id(model_id: &str) -> Result<(), String> {
    if model_id.is_empty()
        || model_id.len() > 512
        || model_id.split('/').any(|segment| {
            segment.is_empty()
                || segment == "."
                || segment == ".."
                || !segment.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || matches!(character, '-' | '_' | '.' | '~' | ':' | '@')
                })
        })
    {
        return Err("the OpenRouter model id contains unsupported URL characters".to_string());
    }
    Ok(())
}

fn validate_provider_slug(slug: &str) -> Result<(), String> {
    // Slash-qualified endpoint tags such as `river/turbo` are values, never paths.
    if slug.is_empty()
        || !slug.split('/').all(|segment| {
            !segment.is_empty()
                && segment.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
                })
        })
    {
        return Err("the OpenRouter provider slug is invalid".to_string());
    }
    Ok(())
}

fn normalize_provider_slugs(mut providers: Vec<String>) -> Result<Vec<String>, String> {
    if providers.is_empty() {
        return Err("choose at least one provider or use Automatic to clear routing".to_string());
    }
    for provider in &providers {
        validate_provider_slug(provider)?;
    }
    let mut index = 0;
    while index < providers.len() {
        if providers[..index].contains(&providers[index]) {
            providers.remove(index);
        } else {
            index += 1;
        }
    }
    Ok(providers)
}

fn providers_from_response(body: &str) -> Result<Vec<OpenRouterProvider>, String> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| format!("OpenRouter returned invalid endpoint JSON: {error}"))?;
    let endpoints = value
        .get("data")
        .and_then(|data| data.get("endpoints"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            "OpenRouter endpoint response had no data.endpoints array".to_string()
        })?;

    let mut providers = BTreeMap::<String, String>::new();
    for (index, endpoint) in endpoints.iter().enumerate() {
        let name = endpoint
            .get("provider_name")
            .and_then(serde_json::Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| format!("OpenRouter endpoint {index} had no provider_name"))?;
        let tag = endpoint
            .get("tag")
            .and_then(serde_json::Value::as_str)
            .filter(|tag| !tag.is_empty())
            .ok_or_else(|| format!("OpenRouter endpoint {index} had no tag"))?;
        let slug = tag.split('/').next().unwrap_or_default();
        if slug.is_empty() {
            return Err(format!("OpenRouter endpoint {index} had an empty provider tag"));
        }
        validate_provider_slug(slug)
            .map_err(|_| format!("OpenRouter endpoint {index} had an invalid provider tag"))?;
        providers
            .entry(slug.to_string())
            .and_modify(|existing| {
                if name < existing.as_str() {
                    *existing = name.to_string();
                }
            })
            .or_insert_with(|| name.to_string());
    }

    Ok(providers
        .into_iter()
        .map(|(slug, name)| OpenRouterProvider { slug, name })
        .collect())
}

fn read_routing(path: &Path, model_id: &str) -> Result<Option<Vec<String>>, String> {
    let document = read_document(path)?;
    routing_from_document(document.as_ref(), model_id)
}

fn read_routings(path: &Path) -> Result<Vec<OpenRouterRouting>, String> {
    let document = read_document(path)?;
    routings_from_document(document.as_ref())
}

fn read_document(path: &Path) -> Result<Option<Value>, String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    serde_yaml::from_str(&contents)
        .map(Some)
        .map_err(|error| format!("could not parse {}: {error}", path.display()))
}

fn routing_overrides(root: &Mapping) -> Result<Option<&Mapping>, String> {
    let Some(providers) = mapping_field(root, PROVIDERS)? else {
        return Ok(None);
    };
    let Some(openrouter) = mapping_field(providers, OPENROUTER)? else {
        return Ok(None);
    };
    mapping_field(openrouter, MODEL_OVERRIDES)
}

fn routing_from_document(
    document: Option<&Value>,
    model_id: &str,
) -> Result<Option<Vec<String>>, String> {
    let Some(document) = document else {
        return Ok(None);
    };
    let root = document
        .as_mapping()
        .ok_or_else(|| "models.yml root must be a mapping".to_string())?;
    let Some(overrides) = routing_overrides(root)? else {
        return Ok(None);
    };
    let Some(model) = overrides.get(&Value::String(model_id.to_string())) else {
        return Ok(None);
    };
    routing_from_override(model, model_id)
}

fn routing_from_override(model: &Value, model_id: &str) -> Result<Option<Vec<String>>, String> {
    let model = model.as_mapping().ok_or_else(|| {
        format!("providers.openrouter.modelOverrides[{model_id}] must be a mapping")
    })?;
    let Some(compat) = mapping_field(model, "compat")? else {
        return Ok(None);
    };
    let Some(routing) = mapping_field(compat, "openRouterRouting")? else {
        return Ok(None);
    };
    let Some(only) = routing.get(&Value::String(ONLY.to_string())) else {
        return Ok(None);
    };
    let providers = only.as_sequence().ok_or_else(|| {
        format!(
            "providers.openrouter.modelOverrides[{model_id}].compat.openRouterRouting.only must be a list"
        )
    })?;

    providers
        .iter()
        .map(|provider| {
            provider.as_str().map(str::to_string).ok_or_else(|| {
                format!(
                    "providers.openrouter.modelOverrides[{model_id}].compat.openRouterRouting.only must contain only strings"
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn routings_from_document(document: Option<&Value>) -> Result<Vec<OpenRouterRouting>, String> {
    let Some(document) = document else {
        return Ok(Vec::new());
    };
    let root = document
        .as_mapping()
        .ok_or_else(|| "models.yml root must be a mapping".to_string())?;
    let Some(overrides) = routing_overrides(root)? else {
        return Ok(Vec::new());
    };

    let mut routings = Vec::new();
    for (key, model) in overrides {
        let model_id = key.as_str().ok_or_else(|| {
            "providers.openrouter.modelOverrides keys must be model id strings".to_string()
        })?;
        if let Some(providers) = routing_from_override(model, model_id)? {
            routings.push(OpenRouterRouting {
                model_id: model_id.to_string(),
                providers,
            });
        }
    }
    Ok(routings)
}

fn mapping_field<'a>(mapping: &'a Mapping, field: &str) -> Result<Option<&'a Mapping>, String> {
    match mapping.get(&Value::String(field.to_string())) {
        None => Ok(None),
        Some(Value::Mapping(value)) => Ok(Some(value)),
        Some(_) => Err(format!("{field} in models.yml must be a mapping")),
    }
}

fn set_routing(path: &Path, model_id: &str, providers: Option<&[String]>) -> Result<(), String> {
    let document = read_document(path)?;
    if document.is_none() && providers.is_none() {
        return Ok(());
    }
    let mut document = document.unwrap_or_else(|| Value::Mapping(Mapping::new()));
    let current = routing_from_document(Some(&document), model_id)?;
    match providers {
        Some(providers) if current.as_deref() == Some(providers) => return Ok(()),
        None if current.is_none() => return Ok(()),
        _ => {}
    }

    let root = document
        .as_mapping_mut()
        .ok_or_else(|| "models.yml root must be a mapping".to_string())?;
    if let Some(providers) = providers {
        let provider_map = ensure_mapping_field(root, PROVIDERS)?;
        let openrouter = ensure_mapping_field(provider_map, OPENROUTER)?;
        let overrides = ensure_mapping_field(openrouter, MODEL_OVERRIDES)?;
        let model = ensure_mapping_field(overrides, model_id)?;
        let compat = ensure_mapping_field(model, "compat")?;
        let routing = ensure_mapping_field(compat, "openRouterRouting")?;
        routing.insert(
            Value::String(ONLY.to_string()),
            Value::Sequence(
                providers
                    .iter()
                    .map(|provider| Value::String(provider.clone()))
                    .collect(),
            ),
        );
    } else {
        let provider_map = ensure_mapping_field(root, PROVIDERS)?;
        let openrouter = ensure_mapping_field(provider_map, OPENROUTER)?;
        let overrides = ensure_mapping_field(openrouter, MODEL_OVERRIDES)?;
        let model = ensure_mapping_field(overrides, model_id)?;
        let compat = ensure_mapping_field(model, "compat")?;
        let routing = ensure_mapping_field(compat, "openRouterRouting")?;
        routing.remove(&Value::String(ONLY.to_string()));

        remove_empty_mapping(compat, "openRouterRouting");
        remove_empty_mapping(model, "compat");
        remove_empty_mapping(overrides, model_id);
        remove_empty_mapping(openrouter, MODEL_OVERRIDES);
        remove_empty_mapping(provider_map, OPENROUTER);
        remove_empty_mapping(root, PROVIDERS);
    }

    write_document(path, &document)
}

fn ensure_mapping_field<'a>(
    mapping: &'a mut Mapping,
    field: &str,
) -> Result<&'a mut Mapping, String> {
    let key = Value::String(field.to_string());
    if !mapping.contains_key(&key) {
        mapping.insert(key.clone(), Value::Mapping(Mapping::new()));
    }
    mapping
        .get_mut(&key)
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| format!("{field} in models.yml must be a mapping"))
}

fn remove_empty_mapping(mapping: &mut Mapping, field: &str) {
    let key = Value::String(field.to_string());
    if mapping
        .get(&key)
        .and_then(Value::as_mapping)
        .is_some_and(|child| child.is_empty())
    {
        mapping.remove(&key);
    }
}

fn write_document(path: &Path, document: &Value) -> Result<(), String> {
    let contents = serde_yaml::to_string(document)
        .map_err(|error| format!("could not serialize {}: {error}", path.display()))?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    let permissions = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "refusing to replace models.yml symlink {}",
                path.display()
            ));
        }
        Ok(_) => Some(
            fs::metadata(path)
                .map_err(|error| format!("could not inspect {}: {error}", path.display()))?
                .permissions(),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("could not inspect {}: {error}", path.display())),
    };
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        format!(
            "could not create a temporary config in {}: {error}",
            parent.display()
        )
    })?;
    temporary
        .write_all(contents.as_bytes())
        .map_err(|error| format!("could not write temporary config: {error}"))?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions)
            .map_err(|error| format!("could not preserve config permissions: {error}"))?;
    }
    temporary
        .persist(path)
        .map_err(|error| format!("could not replace {}: {}", path.display(), error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_providers(path: &Path, model_id: &str, providers: &[&str]) -> Result<(), String> {
        let providers = providers
            .iter()
            .map(|provider| provider.to_string())
            .collect::<Vec<_>>();
        set_routing(path, model_id, Some(&providers))
    }

    #[test]
    fn add_change_and_reset_preserve_every_other_config_value() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = models_yaml_path(directory.path());
        fs::write(
            &path,
            "settings:\n  custom: keep\nproviders:\n  openrouter:\n    api: openai-completions\n    modelOverrides:\n      vendor/model-a:\n        temperature: 0.2\n        compat:\n          supportsReasoningEffort: true\n          openRouterRouting:\n            order: [regional]\n      vendor/model-b:\n        compat:\n          keep: unchanged\n  deepinfra:\n    models:\n      - id: preserved\n",
        )
        .expect("write initial config");

        set_providers(&path, "vendor/model-a", &["river"]).expect("add restriction");
        assert_eq!(
            read_routing(&path, "vendor/model-a").expect("read restriction"),
            Some(vec!["river".to_string()])
        );
        set_providers(&path, "vendor/model-a", &["deepinfra"])
            .expect("change restriction");
        assert_eq!(
            read_routing(&path, "vendor/model-a").expect("read changed restriction"),
            Some(vec!["deepinfra".to_string()])
        );
        set_routing(&path, "vendor/model-a", None).expect("reset restriction");
        assert_eq!(read_routing(&path, "vendor/model-a").unwrap(), None);

        let saved: Value = serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["settings"]["custom"], "keep");
        assert_eq!(saved["providers"]["openrouter"]["api"], "openai-completions");
        assert_eq!(
            saved["providers"]["openrouter"][MODEL_OVERRIDES]["vendor/model-a"]["temperature"]
                .as_f64(),
            Some(0.2)
        );
        assert_eq!(
            saved["providers"]["openrouter"][MODEL_OVERRIDES]["vendor/model-a"]["compat"]["supportsReasoningEffort"],
            true
        );
        assert_eq!(
            saved["providers"]["openrouter"][MODEL_OVERRIDES]["vendor/model-a"]["compat"]["openRouterRouting"]["order"][0],
            "regional"
        );
        assert!(saved["providers"]["openrouter"][MODEL_OVERRIDES]["vendor/model-a"]["compat"]["openRouterRouting"][ONLY]
            .is_null());
        assert_eq!(
            saved["providers"]["openrouter"][MODEL_OVERRIDES]["vendor/model-b"]["compat"]["keep"],
            "unchanged"
        );
        assert_eq!(
            saved["providers"]["deepinfra"]["models"][0]["id"],
            "preserved"
        );
    }

    #[test]
    fn malformed_yaml_and_unsupported_shapes_leave_file_bytes_unchanged() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = models_yaml_path(directory.path());
        for initial in [
            "providers: [\n",
            "- not a mapping\n",
            "providers: false\n",
            "providers:\n  openrouter: false\n",
            "providers:\n  openrouter:\n    modelOverrides: false\n",
            "providers:\n  openrouter:\n    modelOverrides:\n      vendor/model-a: false\n",
            "providers:\n  openrouter:\n    modelOverrides:\n      vendor/model-a:\n        compat: false\n",
            "providers:\n  openrouter:\n    modelOverrides:\n      vendor/model-a:\n        compat:\n          openRouterRouting: []\n",
            "providers:\n  openrouter:\n    modelOverrides:\n      vendor/model-a:\n        compat:\n          openRouterRouting:\n            only: river\n",
        ] {
            fs::write(&path, initial).expect("write invalid config");
            set_providers(&path, "vendor/model-a", &["river"])
                .expect_err("invalid config is refused");
            assert_eq!(fs::read(&path).unwrap(), initial.as_bytes());
        }
    }

    #[test]
    fn reset_missing_override_does_not_create_or_rewrite_config() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let missing = models_yaml_path(directory.path());
        set_routing(&missing, "vendor/model-a", None).expect("reset missing config");
        assert!(!missing.exists());

        let path = directory.path().join("other.yml");
        let original = b"providers:\n  deepinfra:\n    models: []\n";
        fs::write(&path, original).expect("write config");
        set_routing(&path, "vendor/model-a", None).expect("reset absent override");
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn reset_prunes_the_empty_override_path() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = models_yaml_path(directory.path());
        fs::write(&path, "{}\n").expect("write empty config");

        set_providers(&path, "vendor/model-a", &["river"]).expect("add restriction");
        set_routing(&path, "vendor/model-a", None).expect("reset restriction");
        let restored: Value = serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert!(restored.as_mapping().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_config_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("scratch directory");
        let path = models_yaml_path(directory.path());
        fs::write(&path, "providers: {}\n").expect("write config");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640))
            .expect("set config permissions");

        set_providers(&path, "vendor/model-a", &["river"]).expect("write restriction");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_refuses_config_symlinks() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("scratch directory");
        let target = directory.path().join("target.yml");
        let link = models_yaml_path(directory.path());
        fs::write(&target, "providers: {}\n").expect("write target");
        symlink(&target, &link).expect("make config symlink");

        assert!(set_providers(&link, "vendor/model-a", &["river"]).is_err());
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "providers: {}\n");
    }

    #[test]
    fn read_preserves_existing_multiple_provider_restrictions() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = models_yaml_path(directory.path());
        fs::write(
            &path,
            "providers:\n  openrouter:\n    modelOverrides:\n      vendor/model-a:\n        compat:\n          openRouterRouting:\n            only: [river, deepinfra]\n",
        )
        .expect("write config");
        assert_eq!(
            read_routing(&path, "vendor/model-a").unwrap(),
            Some(vec!["river".to_string(), "deepinfra".to_string()])
        );
    }

    #[test]
    fn model_ids_allow_openrouter_paths_but_refuse_url_escape_characters() {
        assert!(validate_model_id("~anthropic/claude-fable-latest").is_ok());
        assert!(validate_model_id("openai/gpt-4o:free").is_ok());
        assert!(validate_model_id("@preset/deepseek-river").is_ok());
        assert!(validate_model_id("author/model@preset/slug").is_ok());
        for invalid in [
            "",
            "../model",
            "openai/model?query",
            "openai/model#fragment",
            "openai/%2e%2e/model",
            "openai//model",
            "openai\\model",
        ] {
            assert!(validate_model_id(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn saved_routings_for_two_models_survive_edits_and_removal() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = models_yaml_path(directory.path());
        set_providers(&path, "vendor/model-a", &["river", "deepinfra"])
            .expect("add first routing");
        set_providers(&path, "vendor/model-b", &["together"]).expect("add second routing");

        assert_eq!(
            read_routings(&path).expect("re-list saved routings"),
            vec![
                OpenRouterRouting {
                    model_id: "vendor/model-a".to_string(),
                    providers: vec!["river".to_string(), "deepinfra".to_string()],
                },
                OpenRouterRouting {
                    model_id: "vendor/model-b".to_string(),
                    providers: vec!["together".to_string()],
                },
            ]
        );

        set_providers(&path, "vendor/model-a", &["deepinfra"]).expect("edit first routing");
        set_routing(&path, "vendor/model-b", None).expect("remove second routing");
        assert_eq!(
            read_routings(&path).expect("re-list after removal"),
            vec![OpenRouterRouting {
                model_id: "vendor/model-a".to_string(),
                providers: vec!["deepinfra".to_string()],
            }]
        );

        let saved: Value = serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let only = &saved["providers"]["openrouter"][MODEL_OVERRIDES]["vendor/model-a"]["compat"]
            ["openRouterRouting"][ONLY];
        assert_eq!(only.as_sequence().map(|values| values.len()), Some(1));
        assert_eq!(only[0].as_str(), Some("deepinfra"));
        assert!(saved["providers"]["openrouter"][MODEL_OVERRIDES]["vendor/model-b"].is_null());
    }

    #[test]
    fn routings_list_includes_empty_unknown_catalog_and_ignores_unrelated_overrides() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = models_yaml_path(directory.path());
        assert_eq!(
            read_routings(&path).expect("missing config lists nothing"),
            Vec::<OpenRouterRouting>::new()
        );

        fs::write(
            &path,
            "providers:\n  openrouter:\n    modelOverrides:\n      vendor/not-in-catalog:\n        compat:\n          openRouterRouting:\n            only: []\n      vendor/qualified:\n        compat:\n          openRouterRouting:\n            only: [river/turbo, deepinfra]\n      vendor/no-routing:\n        temperature: 0.4\n        compat:\n          openRouterRouting:\n            order: [regional]\n      vendor/no-compat:\n        temperature: 0.5\n",
        )
        .expect("write config");

        assert_eq!(
            read_routings(&path).expect("list configured routings"),
            vec![
                OpenRouterRouting {
                    model_id: "vendor/not-in-catalog".to_string(),
                    providers: Vec::new(),
                },
                OpenRouterRouting {
                    model_id: "vendor/qualified".to_string(),
                    providers: vec!["river/turbo".to_string(), "deepinfra".to_string()],
                },
            ]
        );
    }

    #[test]
    fn routings_list_reports_malformed_relevant_shapes() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = models_yaml_path(directory.path());
        for initial in [
            "providers: []\n",
            "providers:\n  openrouter: false\n",
            "providers:\n  openrouter:\n    modelOverrides: false\n",
            "providers:\n  openrouter:\n    modelOverrides:\n      7: {}\n",
            "providers:\n  openrouter:\n    modelOverrides:\n      vendor/model-a: false\n",
            "providers:\n  openrouter:\n    modelOverrides:\n      vendor/model-a:\n        compat: false\n",
            "providers:\n  openrouter:\n    modelOverrides:\n      vendor/model-a:\n        compat:\n          openRouterRouting: []\n",
            "providers:\n  openrouter:\n    modelOverrides:\n      vendor/model-a:\n        compat:\n          openRouterRouting:\n            only: river\n",
            "providers:\n  openrouter:\n    modelOverrides:\n      vendor/model-a:\n        compat:\n          openRouterRouting:\n            only: [river, 7]\n",
        ] {
            fs::write(&path, initial).expect("write invalid config");
            assert!(
                read_routings(&path).is_err(),
                "listed malformed shape {initial:?}"
            );
        }
    }

    #[test]
    fn provider_selection_rejects_empty_and_deduplicates_in_order() {
        assert!(normalize_provider_slugs(Vec::new()).is_err());
        assert!(
            normalize_provider_slugs(vec!["river".to_string(), "bad slug".to_string()]).is_err()
        );
        assert!(normalize_provider_slugs(vec!["river//turbo".to_string()]).is_err());
        assert_eq!(
            normalize_provider_slugs(vec![
                "deepinfra".to_string(),
                "river/turbo".to_string(),
                "deepinfra".to_string(),
            ])
            .expect("valid provider selection"),
            vec!["deepinfra".to_string(), "river/turbo".to_string()]
        );
    }
}
