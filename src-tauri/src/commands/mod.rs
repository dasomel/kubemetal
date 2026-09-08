pub mod access;
// Contract-only boundary for a future agent-driven L3 executor. It is deliberately not wired
// into today's human-triggered Tauri mutations; keep it compiled/tested without exposing a fake
// runtime path. The resolver arguments intentionally mirror the canonical invocation artifact.
#[allow(dead_code, clippy::too_many_arguments)]
pub mod agent_execution_security;
pub mod colima;
pub mod data_ingest;
pub mod deploy_target;
pub mod guardrails;
pub mod kagent;
pub mod local_inference;
pub mod local_inference_bridge;
pub mod local_inference_cache;
pub mod local_inference_cache_identity;
pub mod local_inference_ops;
pub mod metrics;
pub mod mlx;
pub mod modelhub;
pub mod omlx_settings;
pub mod port_forward;
pub mod prefect;
pub mod provision;
pub mod rag;
