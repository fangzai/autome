//! Nonblocking, per-core model discovery. CLI/network latency never owns dispatch.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use autome_domain::role::Runtime;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Model {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub runtime: Runtime,
    pub status: &'static str,
    pub models: Vec<Model>,
    pub detail: Option<String>,
    pub manual_input: bool,
}

impl Snapshot {
    fn empty(runtime: Runtime, status: &'static str, detail: Option<String>) -> Self {
        Self {
            runtime,
            status,
            models: vec![],
            detail,
            manual_input: true,
        }
    }
}

type Query = dyn Fn(Runtime, &Path) -> Result<Vec<Model>, String> + Send + Sync;
struct Entry {
    key: String,
    snapshot: Snapshot,
    finished: Option<Instant>,
}

#[derive(Clone)]
pub struct Catalog {
    entries: Arc<Mutex<BTreeMap<Runtime, Entry>>>,
    query: Arc<Query>,
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            entries: Arc::default(),
            query: Arc::new(query_runtime),
        }
    }
}

impl Catalog {
    #[cfg(test)]
    pub(crate) fn testing() -> Self {
        Self {
            entries: Arc::default(),
            query: Arc::new(|_, _| Ok(vec![])),
        }
    }
    pub fn request(&self, runtime: Runtime, binary: Option<PathBuf>, refresh: bool) -> Snapshot {
        let Some(binary) = binary else {
            return Snapshot::empty(runtime, "missing", Some("尚未安装该 CLI".into()));
        };
        let metadata = std::fs::metadata(&binary).ok();
        let key = format!(
            "{:?}:{}:{:?}:{:?}",
            runtime,
            binary.display(),
            metadata.as_ref().and_then(|m| m.modified().ok()),
            metadata.as_ref().map(|m| m.len())
        );
        let loading = Snapshot::empty(runtime, "loading", None);
        {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = entries.get(&runtime)
                && entry.key == key
            {
                let ttl = if entry.snapshot.status == "error" {
                    30
                } else {
                    600
                };
                if entry.snapshot.status == "loading"
                    || (!refresh
                        && entry
                            .finished
                            .is_some_and(|t| t.elapsed() < Duration::from_secs(ttl)))
                {
                    return entry.snapshot.clone();
                }
            }
            entries.insert(
                runtime,
                Entry {
                    key: key.clone(),
                    snapshot: loading.clone(),
                    finished: None,
                },
            );
        }
        let entries = Arc::clone(&self.entries);
        let query = Arc::clone(&self.query);
        std::thread::spawn(move || {
            let snapshot = match query(runtime, &binary) {
                Ok(models) => Snapshot {
                    models,
                    ..Snapshot::empty(runtime, "ready", None)
                },
                Err(message) => Snapshot::empty(runtime, "error", Some(message)),
            };
            let mut entries = entries.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = entries.get_mut(&runtime)
                && entry.key == key
            {
                entry.snapshot = snapshot;
                entry.finished = Some(Instant::now());
            }
        });
        loading
    }
}

fn query_runtime(runtime: Runtime, binary: &Path) -> Result<Vec<Model>, String> {
    match runtime {
        Runtime::Agy => query_agy(binary),
        Runtime::Codex => query_codex(binary),
        Runtime::Claude => Ok(default_claude_models()),
    }
}

fn query_agy(binary: &Path) -> Result<Vec<Model>, String> {
    let output =
        crate::platform::capture(binary, &["models"], Duration::from_secs(10), 1024 * 1024)
            .map_err(|e| format!("模型列表查询失败：{e}"))?;
    if !output.status.success() {
        return Err(format!(
            "模型列表命令失败（退出码 {:?}），请检查 CLI 登录与网络",
            output.status.code()
        ));
    }
    parse_agy(&String::from_utf8(output.stdout).map_err(|_| "模型列表不是 UTF-8".to_string())?)
}

fn parse_agy(text: &str) -> Result<Vec<Model>, String> {
    let mut models = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if line.starts_with("Fetching")
            || line.starts_with("Available models")
            || line.starts_with("Model")
            || line.starts_with("ID")
            || line.chars().all(|c| c == '-' || c.is_whitespace())
        {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let id = parts.next().unwrap_or("");
        // A model ID is an ASCII identifier, not a progress message or ANSI escape.
        if !id.contains('-')
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._:/".contains(c))
        {
            return Err("无法识别 CLI 模型列表格式，请更新解析器；仍可手动输入模型 ID".into());
        }
        let name = parts.next().unwrap_or(id).trim();
        models.insert(
            id.to_owned(),
            Model {
                id: id.to_owned(),
                name: if name.is_empty() {
                    id.into()
                } else {
                    name.into()
                },
            },
        );
    }
    Ok(models.into_values().collect())
}

fn query_codex(_binary: &Path) -> Result<Vec<Model>, String> {
    let codex_home = std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .map(|h| PathBuf::from(h).join(".codex"))
                .ok()
        });

    if let Some(home) = codex_home {
        let cache_file = home.join("models_cache.json");
        if cache_file.exists()
            && let Ok(content) = std::fs::read_to_string(&cache_file)
            && let Ok(models) = parse_codex_models(&content)
            && !models.is_empty()
        {
            return Ok(models);
        }
    }

    Ok(default_codex_models())
}

fn parse_codex_models(text: &str) -> Result<Vec<Model>, String> {
    #[derive(serde::Deserialize)]
    struct CodexItem {
        slug: Option<String>,
        id: Option<String>,
        display_name: Option<String>,
        name: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct CodexCache {
        models: Option<Vec<CodexItem>>,
    }
    let cache: CodexCache = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut list = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for item in cache.models.unwrap_or_default() {
        if let Some(id) = item.slug.or(item.id) {
            let id = id.trim().to_string();
            if !id.is_empty() && seen.insert(id.clone()) {
                let name = item
                    .display_name
                    .or(item.name)
                    .unwrap_or_else(|| id.clone());
                list.push(Model { id, name });
            }
        }
    }
    if list.is_empty() {
        return Err("未从 Codex 缓存中找到有效模型".into());
    }
    Ok(list)
}

fn default_codex_models() -> Vec<Model> {
    vec![
        Model {
            id: "gpt-6.1-sol".into(),
            name: "GPT-6.1-Sol".into(),
        },
        Model {
            id: "gpt-6-astra".into(),
            name: "GPT-6-Astra".into(),
        },
        Model {
            id: "gpt-6-sol".into(),
            name: "GPT-6-Sol".into(),
        },
        Model {
            id: "gpt-6-luna".into(),
            name: "GPT-6-Luna".into(),
        },
        Model {
            id: "gpt-5.6-sol".into(),
            name: "GPT-5.6-Sol".into(),
        },
        Model {
            id: "gpt-5.6-terra".into(),
            name: "GPT-5.6-Terra".into(),
        },
        Model {
            id: "o3-mini".into(),
            name: "o3-mini".into(),
        },
        Model {
            id: "o1".into(),
            name: "o1".into(),
        },
    ]
}

fn default_claude_models() -> Vec<Model> {
    vec![
        Model {
            id: "claude-opus-5".into(),
            name: "Claude Opus 5".into(),
        },
        Model {
            id: "claude-sonnet-5.5".into(),
            name: "Claude Sonnet 5.5".into(),
        },
        Model {
            id: "claude-haiku-4.5".into(),
            name: "Claude Haiku 4.5".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_columns_without_treating_progress_as_models() {
        let models = parse_agy("Fetching models...\nID  Name\ngemini-4-high\tGemini High\nclaude-opus-5   Claude Opus\n").unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[1].name, "Gemini High");
        assert!(parse_agy("Please log in first").is_err());
        assert!(parse_agy("").unwrap().is_empty());
    }

    #[test]
    fn parses_codex_models_from_json() {
        let json = r#"{
            "models": [
                {"slug": "gpt-6.1-sol", "display_name": "GPT-6.1-Sol"},
                {"slug": "gpt-6-astra", "display_name": "GPT-6-Astra"}
            ]
        }"#;
        let models = parse_codex_models(json).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-6.1-sol");
        assert_eq!(models[0].name, "GPT-6.1-Sol");
    }

    #[test]
    fn a_slow_query_returns_immediately_and_reports_failure_without_a_cli() {
        let (tx, rx) = std::sync::mpsc::channel();
        let rx = Mutex::new(rx);
        let catalog = Catalog {
            entries: Arc::default(),
            query: Arc::new(move |_, _| {
                rx.lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap();
                Err("timeout".into())
            }),
        };
        let binary = Some(PathBuf::from("fake-agy"));
        let started = Instant::now();
        assert_eq!(
            catalog.request(Runtime::Agy, binary.clone(), false).status,
            "loading"
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(
            catalog
                .request(Runtime::Codex, binary.clone(), false)
                .status,
            "loading"
        );
        tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let state = catalog.request(Runtime::Agy, binary.clone(), false);
            if state.status != "loading" {
                assert_eq!(state.status, "error");
                assert_eq!(state.detail.as_deref(), Some("timeout"));
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(catalog.request(Runtime::Agy, None, false).status, "missing");
    }
}
