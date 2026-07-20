/**
 * Tauri Rust 백엔드와 주고받는 IPC 타입 정의 (docs/02 §4.1 표 / docs/03 §3 기준)
 * Rust struct 필드명과 정확히 snake_case 일치해야 합니다.
 */

export interface SystemMetrics {
  total_memory_gb: number;
  used_memory_gb: number;
  memory_usage_percentage: number;
  cpu_usage_percentage: number;
}

export interface ClusterStatus {
  is_running: boolean;
  kubernetes_active: boolean;
  mlflow_ready: boolean;
  seaweedfs_ready: boolean;
  artifact_store_wired: boolean;
}

export interface FineTuneConfig {
  model_name: string;
  dataset_path: string;
  batch_size: number;
  iters: number;
  learning_rate: number;
  adapter_path?: string;
}

export interface HfModel {
  id: string;
  downloads: number;
  likes: number;
  pipeline_tag?: string;
}

export interface DownloadStatus {
  repo_id: string;
  total_files: number;
  done_files: number;
  state: 'downloading' | 'done' | 'error';
  error?: string;
}

export interface LocalModel {
  repo_id: string;
  path: string;
  size_bytes: number;
}
