use serde::Serialize;

use crate::services::local_inference::{LocalInferenceRuntimeKind, RuntimeCapabilities};

pub trait LocalInferenceRuntimeAdapter {
    fn kind(&self) -> LocalInferenceRuntimeKind;
    fn display_name(&self) -> &'static str;
    fn default_port(&self) -> u16;
    fn capabilities(&self) -> RuntimeCapabilities;
    fn install_hint(&self) -> &'static str;
    fn install_commands(&self) -> &'static [&'static str];
}

#[derive(Debug, Clone, Copy)]
pub struct OmlxAdapter;

impl LocalInferenceRuntimeAdapter for OmlxAdapter {
    fn kind(&self) -> LocalInferenceRuntimeKind { LocalInferenceRuntimeKind::Omlx }
    fn display_name(&self) -> &'static str { "oMLX" }
    fn default_port(&self) -> u16 { 8000 }
    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
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
        }
    }
    fn install_hint(&self) -> &'static str {
        "Official paths: the macOS app provides ~/.omlx/bin/omlx; Homebrew uses the jundot/omlx tap. KubeMetal only detects and documents these paths and never installs/upgrades oMLX implicitly."
    }
    fn install_commands(&self) -> &'static [&'static str] {
        &[
            "brew tap jundot/omlx https://github.com/jundot/omlx",
            "brew install jundot/omlx/omlx",
            "brew update && brew upgrade omlx",
        ]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MlxLmAdapter;

impl LocalInferenceRuntimeAdapter for MlxLmAdapter {
    fn kind(&self) -> LocalInferenceRuntimeKind { LocalInferenceRuntimeKind::MlxLm }
    fn display_name(&self) -> &'static str { "mlx-lm" }
    fn default_port(&self) -> u16 { 8080 }
    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
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
        }
    }
    fn install_hint(&self) -> &'static str {
        "Use KubeMetal MLX Studio environment setup. mlx-lm remains the basic/fallback serving runtime."
    }
    fn install_commands(&self) -> &'static [&'static str] { &[] }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeAdapterDescriptor {
    pub runtime: LocalInferenceRuntimeKind,
    pub display_name: String,
    pub default_port: u16,
    pub capabilities: RuntimeCapabilities,
    pub install_hint: String,
    pub install_commands: Vec<String>,
}

fn descriptor(adapter: &impl LocalInferenceRuntimeAdapter) -> RuntimeAdapterDescriptor {
    RuntimeAdapterDescriptor {
        runtime: adapter.kind(),
        display_name: adapter.display_name().into(),
        default_port: adapter.default_port(),
        capabilities: adapter.capabilities(),
        install_hint: adapter.install_hint().into(),
        install_commands: adapter.install_commands().iter().map(|value| (*value).into()).collect(),
    }
}

pub fn all_runtime_adapters() -> Vec<RuntimeAdapterDescriptor> {
    vec![descriptor(&OmlxAdapter), descriptor(&MlxLmAdapter)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapters_keep_omlx_optional_and_mlx_lm_fallback() {
        let adapters = all_runtime_adapters();
        assert_eq!(adapters.len(), 2);
        assert_eq!(adapters[0].runtime, LocalInferenceRuntimeKind::Omlx);
        assert_eq!(adapters[0].default_port, 8000);
        assert!(adapters[0].capabilities.multi_model);
        assert!(adapters[0].install_commands.iter().any(|value| value.contains("jundot/omlx/omlx")));
        assert_eq!(adapters[1].runtime, LocalInferenceRuntimeKind::MlxLm);
        assert_eq!(adapters[1].default_port, 8080);
        assert!(!adapters[1].capabilities.multi_model);
    }
}
