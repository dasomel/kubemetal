import React from 'react';
import { useColima } from '../../hooks/useColima';
import { Boxes, ExternalLink, RefreshCw, Zap, ArrowUpRight, Radio } from 'lucide-react';

export const ProvisionPanel: React.FC = () => {
  const {
    status,
    loading,
    provisionStack,
    startPortForward,
    stopPortForward,
    refresh,
  } = useColima();

  const isRunning = status?.is_running ?? false;
  const k8sActive = status?.kubernetes_active ?? false;
  const mlflowReady = status?.mlflow_ready ?? false;
  const seaweedfsReady = status?.seaweedfs_ready ?? false;

  return (
    <div className="rounded-xl border border-default bg-surface p-6 shadow-card">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-section text-primary flex items-center gap-2">
          <Boxes className="w-5 h-5 text-accent" />
          <span>MLOps 스택 프로비저닝 & 포트포워딩</span>
        </h2>

        <button
          onClick={() => refresh()}
          className="p-1.5 rounded-md bg-surface-raised hover:bg-base text-secondary hover:text-primary border border-default transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
          title="상태 새로고침"
        >
          <RefreshCw className="w-4 h-4" />
        </button>
      </div>

      {/* 스택 준비 상태 목록 */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-3 mb-5">
        {/* MLflow */}
        <div className="p-4 rounded-lg bg-surface-raised border border-default flex items-center justify-between">
          <div>
            <div className="text-body-strong text-primary">MLflow Tracking</div>
            <div className="text-caption text-secondary mt-0.5">Port 5001 (D1)</div>
          </div>
          <span
            className={`px-2 py-0.5 text-caption rounded-full font-medium border ${
              mlflowReady
                ? 'bg-success/10 text-success border-success/30'
                : 'bg-base text-muted border-default'
            }`}
          >
            {mlflowReady ? 'Ready' : 'Not Ready'}
          </span>
        </div>

        {/* SeaweedFS */}
        <div className="p-4 rounded-lg bg-surface-raised border border-default flex items-center justify-between">
          <div>
            <div className="text-body-strong text-primary">SeaweedFS Storage</div>
            <div className="text-caption text-secondary mt-0.5">Port 8333/8888</div>
          </div>
          <span
            className={`px-2 py-0.5 text-caption rounded-full font-medium border ${
              seaweedfsReady
                ? 'bg-success/10 text-success border-success/30'
                : 'bg-base text-muted border-default'
            }`}
          >
            {seaweedfsReady ? 'Ready' : 'Not Ready'}
          </span>
        </div>

        {/* GPU Bridge */}
        <div className="p-4 rounded-lg bg-surface-raised border border-default flex items-center justify-between">
          <div>
            <div className="text-body-strong text-primary">mac-gpu-bridge</div>
            <div className="text-caption text-secondary mt-0.5">host.lima.internal (D10)</div>
          </div>
          <span className="px-2 py-0.5 text-caption rounded-full font-medium bg-accent/10 text-accent border border-accent/20">
            ExternalName
          </span>
        </div>
      </div>

      {/* 액션 버튼 */}
      <div className="flex flex-wrap gap-3 mb-5">
        <button
          onClick={() => provisionStack()}
          disabled={loading || !isRunning || !k8sActive}
          className="py-2 px-4 bg-accent-strong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse font-semibold text-body rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        >
          <Zap className="w-3.5 h-3.5" />
          <span>MLOps 스택 일괄 배포</span>
        </button>

        <button
          onClick={() => startPortForward()}
          disabled={loading || !isRunning || !k8sActive}
          className="py-2 px-4 bg-surface-raised hover:bg-base disabled:opacity-50 disabled:cursor-not-allowed text-primary font-medium text-body rounded-md border border-strong transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        >
          <Radio className="w-3.5 h-3.5 text-accent" />
          <span>포트포워딩 시작 (5001, 8333/8888)</span>
        </button>

        <button
          onClick={() => stopPortForward()}
          disabled={loading || !isRunning}
          className="py-2 px-4 bg-surface-raised hover:bg-base disabled:opacity-50 disabled:cursor-not-allowed text-secondary font-medium text-body rounded-md border border-default transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        >
          포트포워딩 정지
        </button>
      </div>

      {/* 포트 바인딩 바로가기 가이드 */}
      <div className="pt-4 border-t border-default">
        <h3 className="text-caption text-secondary mb-2 flex items-center gap-1">
          <ExternalLink className="w-3.5 h-3.5" /> 호스트 엔드포인트 바로가기 (D1 포트 규격)
        </h3>
        <div className="flex flex-wrap gap-2 text-body">
          <a
            href="http://localhost:5001"
            target="_blank"
            rel="noreferrer"
            className="px-3 py-1.5 rounded-md bg-surface-raised hover:bg-base border border-default text-accent flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            MLflow UI (http://localhost:5001)
            <ArrowUpRight className="w-3 h-3" />
          </a>
          <a
            href="http://localhost:8888"
            target="_blank"
            rel="noreferrer"
            className="px-3 py-1.5 rounded-md bg-surface-raised hover:bg-base border border-default text-accent flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            SeaweedFS Filer UI (http://localhost:8888)
            <ArrowUpRight className="w-3 h-3" />
          </a>
          <span className="px-3 py-1.5 rounded-md bg-surface-raised border border-default text-secondary">
            SeaweedFS S3 API (http://localhost:8333)
          </span>
          <span className="px-3 py-1.5 rounded-md bg-surface-raised border border-default text-secondary">
            Model Serving (http://localhost:8080/v1)
          </span>
        </div>
      </div>
    </div>
  );
};
