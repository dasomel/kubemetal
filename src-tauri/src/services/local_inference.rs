use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::services::process::{augmented_path, resolve_cli_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalInferenceRuntimeKind {
    Omlx,
    MlxLm,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeCapabilities {
    pub openai_chat: bool,
    pub openai_responses: bool,
    pub anthropic_messages: bool,
    pub embeddings: bool,
    pub rerank: bool,
    pub multi_model: bool,
    pub model_load_unload: bool,
    pub model_pinning: bool,
    pub model_ttl: bool,
    pub continuous_batching: bool,
    pub tiered_kv_cache: bool,
    pub mcp: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProbe {
    pub runtime: LocalInferenceRuntimeKind,
    pub installed: bool,
    pub executable: Option<String>,
    pub version: Option<String>,
    pub endpoint: String,
    pub managed_by_kubemetal: bool,
    pub capabilities: RuntimeCapabilities,
    pub detail: Option<String>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn omlx_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = resolve_cli_path("omlx") {
        candidates.push(path);
    }
    if let Some(home) = home_dir() {
        candidates.push(home.join(".omlx/bin/omlx"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/omlx"));
    candidates.push(PathBuf::from("/usr/local/bin/omlx"));
    candidates
}

fn mlx_lm_python_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = home_dir() {
        candidates.push(home.join(".kubemetal/venv/bin/python"));
    }
    if let Ok(path) = resolve_cli_path("python3") {
        candidates.push(path);
    }
    candidates
}

async fn command_version(program: &PathBuf, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .env("PATH", augmented_path())
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let text = if stdout.is_empty() { stderr } else { stdout };
    (!text.is_empty()).then_some(text.lines().next().unwrap_or_default().to_string())
}

fn capabilities(runtime: LocalInferenceRuntimeKind) -> RuntimeCapabilities {
    match runtime {
        LocalInferenceRuntimeKind::Omlx => RuntimeCapabilities {
            openai_chat: true,
            openai_responses: true,
            anthropic_messages: true,
            embeddings: true,
            rerank: true,
            multi_model: true,
            model_load_unload: true,
            model_pinning: true,
            model_ttl: true,
            continuous_batching: true,
            tiered_kv_cache: true,
            mcp: true,
        },
        LocalInferenceRuntimeKind::MlxLm => RuntimeCapabilities {
            openai_chat: true,
            openai_responses: false,
            anthropic_messages: false,
            embeddings: false,
            rerank: false,
            multi_model: false,
            model_load_unload: false,
            model_pinning: false,
            model_ttl: false,
            continuous_batching: false,
            tiered_kv_cache: false,
            mcp: false,
        },
    }
}

pub async fn probe_runtime(runtime: LocalInferenceRuntimeKind) -> RuntimeProbe {
    match runtime {
        LocalInferenceRuntimeKind::Omlx => {
            let executable = omlx_candidates().into_iter().find(|p| p.is_file());
            let version = match executable.as_ref() {
                Some(path) => command_version(path, &["--version"]).await,
                None => None,
            };
            RuntimeProbe {
                runtime,
                installed: executable.is_some(),
                executable: executable.as_ref().map(|p| p.display().to_string()),
                version,
                endpoint: "http://127.0.0.1:8000".into(),
                managed_by_kubemetal: false,
                capabilities: capabilities(runtime),
                detail: executable.is_none().then_some(
                    "oMLX not detected. Install via the official macOS app or Homebrew; KubeMetal will not auto-install it in this phase.".into(),
                ),
            }
        }
        LocalInferenceRuntimeKind::MlxLm => {
            let python = mlx_lm_python_candidates().into_iter().find(|p| p.is_file());
            let version = match python.as_ref() {
                Some(path) => {
                    command_version(
                        path,
                        &[
                            "-c",
                            "import importlib.metadata as m; print(m.version('mlx-lm'))",
                        ],
                    )
                    .await
                }
                None => None,
            };
            RuntimeProbe {
                runtime,
                installed: version.is_some(),
                executable: python.as_ref().map(|p| p.display().to_string()),
                version,
                endpoint: "http://127.0.0.1:8080".into(),
                managed_by_kubemetal: true,
                capabilities: capabilities(runtime),
                detail: None,
            }
        }
    }
}

pub async fn probe_all_runtimes() -> Vec<RuntimeProbe> {
    let (omlx, mlx_lm) = tokio::join!(
        probe_runtime(LocalInferenceRuntimeKind::Omlx),
        probe_runtime(LocalInferenceRuntimeKind::MlxLm)
    );
    vec![omlx, mlx_lm]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omlx_exposes_multi_model_capabilities() {
        let caps = capabilities(LocalInferenceRuntimeKind::Omlx);
        assert!(caps.multi_model);
        assert!(caps.continuous_batching);
        assert!(caps.tiered_kv_cache);
        assert!(caps.openai_responses);
        assert!(caps.anthropic_messages);
    }

    #[test]
    fn mlx_lm_remains_basic_fallback_runtime() {
        let caps = capabilities(LocalInferenceRuntimeKind::MlxLm);
        assert!(caps.openai_chat);
        assert!(!caps.multi_model);
        assert!(!caps.tiered_kv_cache);
    }
}
