/**
 * Tauri Rust 백엔드와 주고받는 IPC 타입 정의 (docs/02 §4.1 표 / docs/03 §3 기준)
 * Rust struct 필드명과 정확히 snake_case 일치해야 합니다.
 */

export interface SystemMetrics {
  total_memory_gb: number;
  used_memory_gb: number;
  memory_usage_percentage: number;
  cpu_usage_percentage: number;
  gpu_usage_percentage?: number;
  gpu_memory_used_gb?: number;
  /** null이면 조회 실패 — "정상"으로 폴백하지 않는다(D22). */
  thermal_state?: 'nominal' | 'fair' | 'serious' | 'critical' | null;
}

export interface HardwareSpec {
  brand_name: string;
  cpu_cores: number;
  total_memory_gb: number;
  /** system_profiler 파싱 실패 시 null — 값을 추정해 채우지 않는다. */
  gpu_cores: number | null;
}

export interface ClusterStatus {
  is_running: boolean;
  kubernetes_active: boolean;
  mlflow_ready: boolean;
  seaweedfs_ready: boolean;
  artifact_store_wired: boolean;
}

export interface KagentDiagnosticReport {
  target_context: string;
  kagent_ready: boolean;
  pod_issues_count: number;
  recent_diagnosis: string;
  recommended_action: string;
  active_agents: string[];
  available_agents: string[];
}

export interface AirgapAssetItem {
  category: string;
  name: string;
  version: string;
  file_name: string;
  exists: boolean;
  size_mb: number;
  /** 정확한 바이트 크기 — 손상 파일은 MB 반올림 시 0으로 뭉개진다. */
  size_bytes: number;
  /** 파일은 있으나 크기 하한(1KiB) 미만 — 부분 수신/에러 응답이 저장된 상태. */
  corrupt: boolean;
}

export interface AirgapStatusReport {
  airgap_dir: string;
  total_assets_count: number;
  downloaded_count: number;
  total_size_mb: number;
  assets: AirgapAssetItem[];
}

export interface AirgapLatestVersionReport {
  name: string;
  current_version: string;
  latest_version: string;
  has_update: boolean;
}

export interface FineTuneConfig {
  model_path: string;
  data_path: string;
  iters: number;
  batch_size: number;
  learning_rate: number;
  adapter_name: string;
  /** 미지정이면 mlx-lm(D29). mlx-vlm 학습 데이터는 question/answer(+image) jsonl 디렉터리. */
  runtime?: MlxRuntime;
  /** mlx-vlm 전용, 비양자화(bf16) 모델 필요 — 4bit는 QuantizedMatmul::vjp로 실패(실측 2026-07-28). */
  train_vision?: boolean;
}

export interface MlxEnvStatus {
  python_ok: boolean;
  venv_exists: boolean;
  mlx_lm_installed: boolean;
  mlx_lm_version?: string;
  /** VLM 런타임(D29). mlx-vlm은 mlx-lm과 같은 venv에 공존한다. */
  mlx_vlm_installed: boolean;
  mlx_vlm_version?: string;
}

/** 서빙 런타임(D29). 값은 Rust enum의 kebab-case 직렬화와 1:1이다. */
export type MlxRuntime = 'mlx-lm' | 'mlx-vlm';

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
  adapter_path?: string;
  runtime: MlxRuntime;
}

export interface MlxStatus {
  env_setup?: MlxEnvSetupState;
  training?: MlxTrainingState;
  serving?: MlxServingState;
  last_serving_error?: string;
}

export interface HfModel {
  id: string;
  downloads: number;
  likes: number;
  pipeline_tag?: string;
  /** safetensors 텐서 dtype별 바이트 폭 가중합으로 추정한 모델 용량(바이트). safetensors가
   * 없는 리포지토리(GGUF 전용 등)는 undefined. */
  size_bytes?: number;
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

export interface CredentialItem {
  key: string;
  value: string;
}

export interface ServiceAccess {
  service: string;
  url: string;
  health: 'ok' | 'unreachable';
  credential_hint?: string;
  credentials: CredentialItem[];
}

export interface GuardrailStatus {
  memory_pressure_level: 'normal' | 'warn' | 'critical' | 'unknown';
  on_battery: boolean;
  battery_pause_enabled: boolean;
  training_paused: boolean;
  caffeinate_active: boolean;
  thermal_state?: 'nominal' | 'fair' | 'serious' | 'critical' | null;
  thermal_pause_enabled: boolean;
}

export interface FlowRunInfo {
  id: string;
  name: string;
  state_type: string;
  state_name: string;
}

export interface PrefectStatus {
  server_ready: boolean;
  env_installed: boolean;
  eval_env_installed: boolean;
  runner_running: boolean;
  runner_pid?: number;
  recent_runs: FlowRunInfo[];
}

export interface EvalMetric {
  run_id: string;
  task: string;
  metric: string;
  value: number;
  timestamp_ms: number;
}

export interface RagSearchResult {
  id?: string;
  text: string;
  score: number;
  source?: string;
  filename?: string;
  chunk_index?: number;
}

export interface RagIndexStatus {
  document_count: number;
  total_chunks: number;
  last_indexed_at?: string;
  index_path?: string;
  status: 'idle' | 'indexing' | 'ready' | 'error';
  error?: string;
}

export interface DvcVersionTag {
  tag: string;
  commit_hash: string;
  message: string;
  created_at?: string;
  dataset_path?: string;
}

export interface DvcStatus {
  initialized: boolean;
  remote_url?: string;
  current_tag?: string;
  dataset_path?: string;
  tags: DvcVersionTag[];
  last_error?: string;
}

export type DataIngestSourceType = 'web' | 'file' | 'huggingface';

export interface DataIngestConfig {
  source_type: DataIngestSourceType;
  source_target: string;
  chunk_size: number;
  chunk_overlap: number;
  collection_name: string;
  auto_dvc_backup: boolean;
}

export type DagNodeId = 'ingest' | 'clean_chunk' | 'lancedb_store' | 'dvc_backup';

export type DagNodeStatus = 'idle' | 'running' | 'success' | 'error';

export interface DagNodeMetric {
  id: DagNodeId;
  name: string;
  status: DagNodeStatus;
  items_processed: number;
  duration_ms: number;
  logs: string[];
  error_message?: string;
  details?: Record<string, string | number | boolean>;
}

export interface DataIngestPipelineRun {
  run_id: string;
  config: DataIngestConfig;
  overall_status: DagNodeStatus;
  current_node?: DagNodeId;
  start_time?: string;
  end_time?: string;
  nodes: Record<DagNodeId, DagNodeMetric>;
}

/**
 * run_data_ingest / get_ingest_status 원본 응답 (src-tauri/src/commands/data_ingest.rs 1:1).
 * 프론트 표시용 DagNodeMetric/DataIngestPipelineRun과는 별개 — useDataIngest에서 매핑한다.
 */
export interface BackendDagNodeState {
  node_id: string;
  name: string;
  status: string;
  duration_sec: number;
  items_processed: number;
  details: string;
}

export interface BackendIngestFlowResult {
  status: string;
  dataset_name: string;
  source_type: string;
  source_path: string;
  total_duration_sec: number;
  total_items_extracted: number;
  total_chunks_created: number;
  lancedb_collection: string;
  db_path: string;
  dvc_backed_up: boolean;
  dag_nodes: BackendDagNodeState[];
  error?: string;
}

export interface IngestedDatasetInfo {
  collection_name: string;
  total_chunks: number;
  db_path: string;
  is_lance_table: boolean;
}

export interface IngestStatusResponse {
  env_installed: boolean;
  default_db_path: string;
  active_collections: IngestedDatasetInfo[];
  last_result?: BackendIngestFlowResult;
}



/**
 * 배포 대상(D26). `kind`로 구분되는 브리지 상태는 Rust의 tagged enum과 1:1 대응한다.
 * - keep_base: colima 전용. base 매니페스트의 host.lima.internal을 그대로 쓴다(D10 실측값).
 * - verified: 클러스터 **내부에서** 도달을 확인한 주소. 이 상태여야만 배포할 수 있다.
 * - unverified: 후보는 있으나 검증 실패/미실행. 배포가 거부되는 상태다.
 */
export type BridgeState =
  | { kind: 'keep_base' }
  | { kind: 'verified'; host: string }
  | { kind: 'unverified'; candidates: string[]; reason: string };

export interface DeployTarget {
  context: string;
  namespace: string;
  /** null이면 대상 클러스터의 기본 StorageClass를 따른다. */
  storage_class: string | null;
  /** 사내 레지스트리/미러. docker.io 이미지만 재지정된다. */
  image_registry: string | null;
  bridge: BridgeState;
}

export interface PreflightReport {
  context: string;
  reachable: boolean;
  node_count: number;
  node_ips: string[];
  default_storage_class: string | null;
  storage_classes: string[];
  argocd_present: boolean;
  /** 대상 ns를 destination으로 삼는 Application. 비어 있지 않으면 직접 apply가 selfHeal에 되돌려진다. */
  argocd_owners: string[];
  enforcing_policies: string[];
  bridge_candidates: string[];
  /** 사람이 읽을 차단 사유. 비어 있으면 배포 가능. */
  blockers: string[];
}
