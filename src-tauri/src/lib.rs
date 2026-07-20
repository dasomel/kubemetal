mod commands;
mod services;

use std::sync::Mutex;
use sysinfo::System;

use commands::colima::{get_cluster_status, start_cluster, stop_cluster};
use commands::metrics::get_system_metrics;
use commands::port_forward::{start_port_forward, stop_port_forward, PortForwardState};
use commands::provision::provision_mlops_stack;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(System::new_all()))
        .manage(PortForwardState::default())
        .invoke_handler(tauri::generate_handler![
            get_system_metrics,
            get_cluster_status,
            start_cluster,
            stop_cluster,
            provision_mlops_stack,
            start_port_forward,
            stop_port_forward,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
