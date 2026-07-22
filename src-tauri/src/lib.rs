mod commands;
mod services;

use std::sync::Mutex;
use sysinfo::System;

use commands::access::get_service_access;
use commands::colima::{get_cluster_status, start_cluster, stop_cluster};
use commands::guardrails::{
    get_guardrail_status, pause_mlx_training, resume_mlx_training, set_guardrail_config,
    GuardrailState,
};
use commands::metrics::get_system_metrics;
use commands::mlx::{
    check_mlx_env, get_mlx_status, kill_mlx_process, run_mlx_finetune, setup_mlx_env,
    start_model_serving, stop_model_serving, suggest_serving_port, MlxState,
};
use commands::modelhub::{
    download_hf_model, get_model_downloads, list_local_models, list_registered_models,
    register_model_mlflow, search_hf_models, upload_model_to_storage, ModelHubState,
};
use commands::port_forward::{start_port_forward, stop_port_forward, PortForwardState};
use commands::prefect::{
    get_eval_results, get_prefect_status, setup_eval_env, setup_prefect_env,
    start_prefect_runner, stop_prefect_runner, trigger_evaluate_flow, trigger_finetune_flow,
    PrefectState,
};
use commands::provision::provision_mlops_stack;
use commands::rag::{
    dvc_commit_dataset, get_dvc_status, get_rag_status, index_documents, query_rag, setup_rag_env,
    RagState,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(System::new_all()))
        .manage(PortForwardState::default())
        .manage(ModelHubState::default())
        .manage(MlxState::default())
        .manage(GuardrailState::default())
        .manage(PrefectState::default())
        .manage(RagState::default())
        .invoke_handler(tauri::generate_handler![
            get_system_metrics,
            get_cluster_status,
            start_cluster,
            stop_cluster,
            provision_mlops_stack,
            start_port_forward,
            stop_port_forward,
            search_hf_models,
            download_hf_model,
            get_model_downloads,
            list_local_models,
            upload_model_to_storage,
            register_model_mlflow,
            list_registered_models,
            check_mlx_env,
            setup_mlx_env,
            run_mlx_finetune,
            get_mlx_status,
            kill_mlx_process,
            start_model_serving,
            stop_model_serving,
            suggest_serving_port,
            get_service_access,
            get_guardrail_status,
            set_guardrail_config,
            pause_mlx_training,
            resume_mlx_training,
            get_prefect_status,
            setup_prefect_env,
            start_prefect_runner,
            stop_prefect_runner,
            trigger_finetune_flow,
            setup_eval_env,
            trigger_evaluate_flow,
            get_eval_results,
            get_rag_status,
            setup_rag_env,
            index_documents,
            query_rag,
            dvc_commit_dataset,
            get_dvc_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
