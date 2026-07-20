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
    <div className="rounded-xl border border-slate-800 bg-slate-900/60 p-5 shadow-xl backdrop-blur">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-lg font-bold text-slate-100 flex items-center gap-2">
          <Boxes className="w-5 h-5 text-teal-400" />
          <span>MLOps 스택 프로비저닝 & 포트포워딩</span>
        </h2>

        <button
          onClick={() => refresh()}
          className="p-1.5 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-400 hover:text-slate-200 transition-colors"
          title="상태 새로고침"
        >
          <RefreshCw className="w-4 h-4" />
        </button>
      </div>

      {/* 스택 준비 상태 목록 */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-3 mb-5">
        {/* MLflow */}
        <div className="p-3.5 rounded-lg bg-slate-950/50 border border-slate-800 flex items-center justify-between">
          <div>
            <div className="text-xs font-semibold text-slate-300">MLflow Tracking</div>
            <div className="text-[11px] text-slate-400 mt-0.5">Port 5001 (D1)</div>
          </div>
          <span
            className={`px-2 py-0.5 text-[11px] rounded-full font-medium ${
              mlflowReady
                ? 'bg-emerald-500/20 text-emerald-400 border border-emerald-500/30'
                : 'bg-slate-800 text-slate-400'
            }`}
          >
            {mlflowReady ? 'Ready' : 'Not Ready'}
          </span>
        </div>

        {/* SeaweedFS */}
        <div className="p-3.5 rounded-lg bg-slate-950/50 border border-slate-800 flex items-center justify-between">
          <div>
            <div className="text-xs font-semibold text-slate-300">SeaweedFS Storage</div>
            <div className="text-[11px] text-slate-400 mt-0.5">Port 8333/8888</div>
          </div>
          <span
            className={`px-2 py-0.5 text-[11px] rounded-full font-medium ${
              seaweedfsReady
                ? 'bg-emerald-500/20 text-emerald-400 border border-emerald-500/30'
                : 'bg-slate-800 text-slate-400'
            }`}
          >
            {seaweedfsReady ? 'Ready' : 'Not Ready'}
          </span>
        </div>

        {/* GPU Bridge */}
        <div className="p-3.5 rounded-lg bg-slate-950/50 border border-slate-800 flex items-center justify-between">
          <div>
            <div className="text-xs font-semibold text-slate-300">mac-gpu-bridge</div>
            <div className="text-[11px] text-slate-400 mt-0.5">host.lima.internal (D10)</div>
          </div>
          <span className="px-2 py-0.5 text-[11px] rounded-full font-medium bg-blue-500/10 text-blue-400 border border-blue-500/20">
            ExternalName
          </span>
        </div>
      </div>

      {/* 액션 버튼 */}
      <div className="flex flex-wrap gap-3 mb-5">
        <button
          onClick={() => provisionStack()}
          disabled={loading || !isRunning || !k8sActive}
          className="py-2 px-4 bg-teal-600 hover:bg-teal-500 disabled:opacity-50 text-white font-medium text-xs rounded-lg transition-all shadow-md shadow-teal-500/10 flex items-center gap-1.5"
        >
          <Zap className="w-3.5 h-3.5" />
          <span>MLOps 스택 일괄 배포</span>
        </button>

        <button
          onClick={() => startPortForward()}
          disabled={loading || !isRunning || !k8sActive}
          className="py-2 px-4 bg-slate-800 hover:bg-slate-700 disabled:opacity-50 text-slate-200 font-medium text-xs rounded-lg border border-slate-700 transition-all flex items-center gap-1.5"
        >
          <Radio className="w-3.5 h-3.5 text-blue-400" />
          <span>포트포워딩 시작 (5001, 8333/8888)</span>
        </button>

        <button
          onClick={() => stopPortForward()}
          disabled={loading || !isRunning}
          className="py-2 px-4 bg-slate-800 hover:bg-slate-700 disabled:opacity-50 text-slate-400 font-medium text-xs rounded-lg border border-slate-700 transition-all"
        >
          포트포워딩 정지
        </button>
      </div>

      {/* 포트 바인딩 바로가기 가이드 */}
      <div className="pt-4 border-t border-slate-800/80">
        <h3 className="text-xs font-semibold text-slate-400 mb-2 flex items-center gap-1">
          <ExternalLink className="w-3.5 h-3.5" /> 호스트 엔드포인트 바로가기 (D1 포트 규격)
        </h3>
        <div className="flex flex-wrap gap-2 text-xs">
          <a
            href="http://localhost:5001"
            target="_blank"
            rel="noreferrer"
            className="px-3 py-1.5 rounded bg-slate-950 hover:bg-slate-800 border border-slate-800 text-blue-400 flex items-center gap-1"
          >
            MLflow UI (http://localhost:5001)
            <ArrowUpRight className="w-3 h-3" />
          </a>
          <a
            href="http://localhost:8888"
            target="_blank"
            rel="noreferrer"
            className="px-3 py-1.5 rounded bg-slate-950 hover:bg-slate-800 border border-slate-800 text-teal-400 flex items-center gap-1"
          >
            SeaweedFS Filer UI (http://localhost:8888)
            <ArrowUpRight className="w-3 h-3" />
          </a>
          <span className="px-3 py-1.5 rounded bg-slate-950 border border-slate-800 text-slate-400">
            SeaweedFS S3 API (http://localhost:8333)
          </span>
          <span className="px-3 py-1.5 rounded bg-slate-950 border border-slate-800 text-slate-400">
            Model Serving (http://localhost:8080/v1)
          </span>
        </div>
      </div>
    </div>
  );
};
