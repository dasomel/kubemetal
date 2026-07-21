import React from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useColima } from '../../hooks/useColima';
import { Boxes, ExternalLink, RefreshCw, Zap, ArrowUpRight, Radio } from 'lucide-react';

const openEndpoint = (url: string) => {
  openUrl(url).catch(() => window.open(url, '_blank'));
};

export const ProvisionPanel: React.FC = () => {
  const {
    status,
    loading,
    portForwardStatus,
    provisionStack,
    startPortForward,
    stopPortForward,
    refresh,
  } = useColima();

  const isRunning = status?.is_running ?? false;
  const k8sActive = status?.kubernetes_active ?? false;
  const mlflowReady = status?.mlflow_ready ?? false;
  const seaweedfsReady = status?.seaweedfs_ready ?? false;
  const artifactStoreWired = status?.artifact_store_wired ?? false;

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Boxes className="w-4 h-4 text-primary" />
          <span>MLOps 스택 프로비저닝 & 포트포워딩</span>
        </h2>

        <button
          onClick={() => refresh()}
          className="p-1.5 rounded-md bg-surfaceRaised hover:brightness-95 text-inkMuted hover:text-ink transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
          title="상태 새로고침"
        >
          <RefreshCw className="w-4 h-4" />
        </button>
      </div>

      {/* 스택 준비 상태 목록 */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-2 mb-4">
        {/* MLflow */}
        <div className="p-3 rounded-lg bg-surfaceRaised flex items-center justify-between">
          <div>
            <div className="text-bodyStrong text-ink">MLflow Tracking</div>
            <div className="text-caption text-inkFaint mt-0.5">Port 5001</div>
          </div>
          <div className="flex flex-col items-end gap-1">
            <div className="flex items-center gap-1.5 text-caption text-inkMuted">
              <span className={`w-2 h-2 rounded-full ${mlflowReady ? 'bg-success' : 'bg-inkFaint'}`} />
              <span>{mlflowReady ? 'Ready' : 'Not Ready'}</span>
            </div>
            <div className={`flex items-center gap-1.5 text-caption ${artifactStoreWired ? 'text-success' : 'text-inkFaint'}`}>
              <span className={`w-2 h-2 rounded-full ${artifactStoreWired ? 'bg-success' : 'bg-inkFaint'}`} />
              <span>{artifactStoreWired ? 'SeaweedFS 연동됨' : '연동 대기'}</span>
            </div>
          </div>
        </div>

        {/* SeaweedFS */}
        <div className="p-3 rounded-lg bg-surfaceRaised flex items-center justify-between">
          <div>
            <div className="text-bodyStrong text-ink">SeaweedFS Storage</div>
            <div className="text-caption text-inkFaint mt-0.5">Port 8333/8888</div>
          </div>
          <div className="flex items-center gap-1.5 text-caption text-inkMuted">
            <span className={`w-2 h-2 rounded-full ${seaweedfsReady ? 'bg-success' : 'bg-inkFaint'}`} />
            <span>{seaweedfsReady ? 'Ready' : 'Not Ready'}</span>
          </div>
        </div>

        {/* GPU Bridge */}
        <div className="p-3 rounded-lg bg-surfaceRaised flex items-center justify-between">
          <div>
            <div className="text-bodyStrong text-ink">mac-gpu-bridge</div>
            <div className="text-caption text-inkFaint mt-0.5">host.lima.internal</div>
          </div>
          <span className="text-caption text-primary">ExternalName</span>
        </div>
      </div>

      {/* 액션 버튼 */}
      <div className="flex flex-wrap gap-3 mb-4">
        <button
          onClick={() => provisionStack()}
          disabled={loading || !isRunning || !k8sActive}
          className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        >
          <Zap className="w-3.5 h-3.5" />
          <span>MLOps 스택 일괄 배포</span>
        </button>

        <button
          onClick={() => startPortForward()}
          disabled={loading || !isRunning || !k8sActive}
          className="py-2.5 px-4 bg-surfaceRaised hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        >
          <Radio
            className={`w-3.5 h-3.5 ${
              portForwardStatus && portForwardStatus.active < portForwardStatus.total
                ? 'text-warning'
                : 'text-primary'
            }`}
          />
          <span>
            {portForwardStatus
              ? portForwardStatus.active === portForwardStatus.total
                ? `포워딩 활성 (${portForwardStatus.active}/${portForwardStatus.total})`
                : `포워딩 일부 활성 (${portForwardStatus.active}/${portForwardStatus.total})`
              : '포트포워딩 시작 (5001, 8333/8888)'}
          </span>
        </button>

        <button
          onClick={() => stopPortForward()}
          disabled={loading || !isRunning}
          className="py-2.5 px-4 bg-surfaceRaised hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-inkMuted text-bodyStrong rounded-md transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        >
          포트포워딩 정지
        </button>
      </div>

      {/* 포트 바인딩 바로가기 가이드 */}
      <div className="pt-4 border-t border-hairline/8">
        <h3 className="text-label uppercase text-inkFaint mb-2 flex items-center gap-1.5">
          <ExternalLink className="w-3.5 h-3.5" /> 호스트 엔드포인트 바로가기
        </h3>
        <div className="flex flex-wrap gap-2 text-caption">
          <button
            type="button"
            onClick={() => openEndpoint('http://localhost:5001')}
            className="px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 text-primary flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            MLflow UI (http://localhost:5001)
            <ArrowUpRight className="w-3 h-3" />
          </button>
          <button
            type="button"
            onClick={() => openEndpoint('http://localhost:8888')}
            className="px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 text-primary flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            SeaweedFS Filer UI (http://localhost:8888)
            <ArrowUpRight className="w-3 h-3" />
          </button>
          <span className="px-3 py-1.5 rounded-md bg-surfaceRaised text-inkMuted">
            SeaweedFS S3 API (http://localhost:8333)
          </span>
          <span className="px-3 py-1.5 rounded-md bg-surfaceRaised text-inkMuted">
            Model Serving (http://127.0.0.1:8080/v1/models)
          </span>
        </div>
      </div>
    </div>
  );
};
