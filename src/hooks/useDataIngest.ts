import { useState, useCallback, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type {
  DataIngestConfig,
  DataIngestPipelineRun,
  DataIngestSourceType,
  DagNodeId,
  DagNodeMetric,
  DagNodeStatus,
  BackendIngestFlowResult,
  BackendDagNodeState,
  IngestedDatasetInfo,
  IngestStatusResponse,
} from '../types/ipc';

const defaultConfig: DataIngestConfig = {
  source_type: 'web',
  source_target: 'https://docs.kubemetal.io',
  chunk_size: 512,
  chunk_overlap: 64,
  collection_name: 'kb_docs',
  auto_dvc_backup: true,
};

// scripts/data/ingest_host.py의 DAG 노드 id/상태 문자열은 프론트 표시용 id/상태와 다르므로 매핑한다.
const BACKEND_NODE_ID_MAP: Record<string, DagNodeId> = {
  extract: 'ingest',
  clean_chunk: 'clean_chunk',
  lancedb_index: 'lancedb_store',
  dvc_backup: 'dvc_backup',
};

const BACKEND_STATUS_MAP: Record<string, DagNodeStatus> = {
  completed: 'success',
  failed: 'error',
  skipped: 'idle',
};

// 프론트 소스 선택기의 'file'은 백엔드(argparse choices: web/rss/hf/huggingface/local)에서 'local'이다.
function toBackendSourceType(sourceType: DataIngestSourceType): string {
  return sourceType === 'file' ? 'local' : sourceType;
}

const ts = () => new Date().toLocaleTimeString();

function createInitialNodes(): Record<DagNodeId, DagNodeMetric> {
  return {
    ingest: {
      id: 'ingest',
      name: 'Source Ingestion',
      status: 'idle',
      items_processed: 0,
      duration_ms: 0,
      logs: ['대기 중... 파이프라인을 실행하면 데이터 수집이 시작됩니다.'],
    },
    clean_chunk: {
      id: 'clean_chunk',
      name: 'Text Cleaning & Chunking',
      status: 'idle',
      items_processed: 0,
      duration_ms: 0,
      logs: ['대기 중... 데이터 수집 후 정제 및 청킹 작업이 진행됩니다.'],
    },
    lancedb_store: {
      id: 'lancedb_store',
      name: 'LanceDB Vector Store',
      status: 'idle',
      items_processed: 0,
      duration_ms: 0,
      logs: ['대기 중... 청크 데이터 임베딩 생성 및 LanceDB 인덱싱이 실행됩니다.'],
    },
    dvc_backup: {
      id: 'dvc_backup',
      name: 'SeaweedFS DVC Backup',
      status: 'idle',
      items_processed: 0,
      duration_ms: 0,
      logs: ['대기 중... 벡터 DB 스냅샷을 SeaweedFS S3 버킷에 DVC 버저닝 백업합니다.'],
    },
  };
}

// run_data_ingest가 반환한 실제 dag_nodes로 최종 노드 상태를 일괄 반영한다.
// (백엔드는 python 서브프로세스가 끝날 때까지 블로킹하는 단일 호출이라 중간 단계 스트리밍은 제공하지 않는다.)
function applyBackendNodes(
  base: Record<DagNodeId, DagNodeMetric>,
  backendNodes: BackendDagNodeState[]
): Record<DagNodeId, DagNodeMetric> {
  const next = { ...base };
  for (const bn of backendNodes) {
    const nodeId = BACKEND_NODE_ID_MAP[bn.node_id];
    if (!nodeId) continue;
    const status = BACKEND_STATUS_MAP[bn.status] || 'idle';
    next[nodeId] = {
      ...next[nodeId],
      status,
      items_processed: bn.items_processed,
      duration_ms: Math.round(bn.duration_sec * 1000),
      logs: [`[${ts()}] ${bn.details}`],
      details: { info: bn.details },
      error_message: status === 'error' ? bn.details : undefined,
    };
  }
  return next;
}

export function useDataIngest() {
  const [config, setConfig] = useState<DataIngestConfig>(defaultConfig);
  const [activeNodeId, setActiveNodeId] = useState<DagNodeId | null>('ingest');
  const [pipelineRun, setPipelineRun] = useState<DataIngestPipelineRun | null>(null);
  const [isPipelineRunning, setIsPipelineRunning] = useState(false);
  const [datasets, setDatasets] = useState<IngestedDatasetInfo[]>([]);
  const [loadingDatasets, setLoadingDatasets] = useState(false);

  const resetPipeline = useCallback(() => {
    setPipelineRun(null);
    setIsPipelineRunning(false);
  }, []);

  const refreshDatasets = useCallback(async () => {
    setLoadingDatasets(true);
    try {
      const list = await invoke<IngestedDatasetInfo[]>('list_ingested_datasets');
      setDatasets(list);
    } catch (err) {
      console.error('수집 이력 목록 로드 오류:', err);
    } finally {
      setLoadingDatasets(false);
    }
  }, []);

  // 탭 진입 시 최소 1회 수집 이력을 조회한다.
  useEffect(() => {
    refreshDatasets();
  }, [refreshDatasets]);

  const triggerPipelineRun = useCallback(async (customConfig?: DataIngestConfig) => {
    const runConfig = customConfig || config;
    const runId = `run-${Date.now().toString(36)}`;
    const startTime = new Date().toISOString();

    const sourceLabel =
      runConfig.source_type === 'web'
        ? 'Web URL'
        : runConfig.source_type === 'huggingface'
        ? 'HuggingFace Dataset'
        : 'Local File/Directory';

    let currentNodes = createInitialNodes();
    currentNodes.ingest = {
      ...currentNodes.ingest,
      status: 'running',
      logs: [
        `[${ts()}] 수집 대상: [${sourceLabel}] ${runConfig.source_target}`,
        `[${ts()}] run_data_ingest 파이프라인 프로세스를 시작합니다...`,
      ],
    };

    const updateRun = (
      overallStatus: DagNodeStatus,
      currentNode: DagNodeId | undefined,
      nodes: Record<DagNodeId, DagNodeMetric>,
      endTime?: string
    ) => {
      setPipelineRun({
        run_id: runId,
        config: runConfig,
        overall_status: overallStatus,
        current_node: currentNode,
        start_time: startTime,
        end_time: endTime,
        nodes: { ...nodes },
      });
    };

    setIsPipelineRunning(true);
    setActiveNodeId('ingest');
    updateRun('running', 'ingest', currentNodes);

    // 백엔드가 실행 중 중간 진행률을 제공하지 않으므로, 폴링은 env/수집 이력 갱신 용도로만 사용한다.
    const pollId = window.setInterval(async () => {
      try {
        const statusRes = await invoke<IngestStatusResponse>('get_ingest_status');
        setDatasets(statusRes.active_collections);
      } catch {
        // best-effort — 폴링 실패는 무시한다.
      }
    }, 2000);

    try {
      // 백엔드 계약(src-tauri/src/commands/data_ingest.rs::IngestConfig, rename_all = "camelCase"):
      // 커맨드 인자는 단일 config 객체이며 필드는 camelCase — snake_case 개별 인자가 아니다.
      const result = await invoke<BackendIngestFlowResult>('run_data_ingest', {
        config: {
          sourceType: toBackendSourceType(runConfig.source_type),
          sourcePath: runConfig.source_target,
          collectionName: runConfig.collection_name,
          chunkSize: runConfig.chunk_size,
          chunkOverlap: runConfig.chunk_overlap,
          enableDvcBackup: runConfig.auto_dvc_backup,
        },
      });

      currentNodes = applyBackendNodes(currentNodes, result.dag_nodes);
      const overallStatus: DagNodeStatus = result.status === 'ok' ? 'success' : 'error';
      updateRun(overallStatus, undefined, currentNodes, new Date().toISOString());

      if (overallStatus === 'success') {
        await refreshDatasets();
      }
    } catch (err: any) {
      // Rust가 실패 시 구조화된 dag_nodes 없이 오류 문자열만 반환하므로, 진행 중이던 노드에 오류를 표기한다.
      const errMessage = String(err?.message || err);
      const failedNodeId =
        (Object.keys(currentNodes) as DagNodeId[]).find((id) => currentNodes[id].status === 'running') || 'ingest';

      currentNodes = {
        ...currentNodes,
        [failedNodeId]: {
          ...currentNodes[failedNodeId],
          status: 'error',
          error_message: errMessage,
          logs: [...currentNodes[failedNodeId].logs, `[오류] ${errMessage}`],
        },
      };
      updateRun('error', failedNodeId, currentNodes, new Date().toISOString());
    } finally {
      window.clearInterval(pollId);
      setIsPipelineRunning(false);
    }
  }, [config, refreshDatasets]);

  return {
    config,
    setConfig,
    pipelineRun,
    activeNodeId,
    setActiveNodeId,
    isPipelineRunning,
    triggerPipelineRun,
    resetPipeline,
    datasets,
    loadingDatasets,
    refreshDatasets,
  };
}
