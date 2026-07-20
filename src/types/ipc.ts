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
  model_path: string;
  data_path: string;
  iters: number;
  batch_size: number;
  learning_rate: number;
  adapter_name: string;
}

export interface MlxEnvStatus {
  python_ok: boolean;
  venv_exists: boolean;
  mlx_lm_installed: boolean;
  mlx_lm_version?: string;
}

export interface MlxEnvSetupState {
  state: string;
  error?: string;
}

export interface MlxTrainingState {
  pid: number;
  status: string;
  current_iter: number;
  total_iters: number;
  last_loss?: number;
  adapter_path?: string;
  error?: string;
}

export interface MlxServingState {
  pid: number;
  port: number;
  model_path: string;
}

export interface MlxStatus {
  env_setup?: MlxEnvSetupState;
  training?: MlxTrainingState;
  serving?: MlxServingState;
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

export interface RegisteredModel {
  name: string;
  latest_version?: string;
  last_updated_ms?: number;
}
