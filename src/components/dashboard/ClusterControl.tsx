import React from 'react';
import { useColima } from '../../hooks/useColima';
import { useMetrics } from '../../hooks/useMetrics';
import { recommendVmResources } from '../../lib/recommendVmResources';
import { Server, Play, Square, Loader2, CheckCircle2, XCircle, ShieldCheck } from 'lucide-react';

export const ClusterControl: React.FC = () => {
  const { status, loading, actionMessage, startCluster, stopCluster } = useColima();
  const metrics = useMetrics();

  const totalRam = metrics?.total_memory_gb ?? 16;
  const { cpu, memoryGb } = recommendVmResources(totalRam);

  const isRunning = status?.is_running ?? false;
  const k8sActive = status?.kubernetes_active ?? false;

  return (
    <div className="rounded-xl border border-slate-800 bg-slate-900/60 p-5 shadow-xl backdrop-blur flex flex-col justify-between">
      <div>
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-bold text-slate-100 flex items-center gap-2">
            <Server className="w-5 h-5 text-indigo-400" />
            <span>Colima K8s Control</span>
          </h2>

          <div className="flex items-center gap-2">
            <span
              className={`inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-semibold border ${
                isRunning
                  ? 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20'
                  : 'bg-rose-500/10 text-rose-400 border-rose-500/20'
              }`}
            >
              {isRunning ? (
                <>
                  <CheckCircle2 className="w-3.5 h-3.5" /> RUNNING (vz)
                </>
              ) : (
                <>
                  <XCircle className="w-3.5 h-3.5" /> STOPPED
                </>
              )}
            </span>
          </div>
        </div>

        {/* 상태 세부사항 */}
        <div className="grid grid-cols-2 gap-3 mb-5">
          <div className="p-3 rounded-lg bg-slate-950/40 border border-slate-800 text-xs">
            <span className="text-slate-400 block mb-1">VM 엔진 스펙</span>
            <span className="font-semibold text-slate-200">
              vz + virtiofs
            </span>
          </div>
          <div className="p-3 rounded-lg bg-slate-950/40 border border-slate-800 text-xs">
            <span className="text-slate-400 block mb-1">Kubernetes Active</span>
            <span className={`font-semibold ${k8sActive ? 'text-emerald-400' : 'text-slate-400'}`}>
              {k8sActive ? 'K3s Enabled' : 'Disabled / Inactive'}
            </span>
          </div>
        </div>

        {/* 동적 추천 할당 뱃지 */}
        <div className="p-3 rounded-lg bg-indigo-950/20 border border-indigo-800/40 text-xs text-indigo-200 mb-5 flex items-center gap-2">
          <ShieldCheck className="w-4 h-4 text-indigo-400 shrink-0" />
          <span>
            자동 산정 VM 스펙: <strong>{cpu} CPU 코어 / {memoryGb}GB RAM</strong> (호스트 {totalRam}GB RAM 기준 D4)
          </span>
        </div>
      </div>

      {actionMessage && (
        <div className="text-xs text-blue-400 bg-blue-500/10 border border-blue-500/20 rounded-lg p-2.5 mb-4 flex items-center gap-2 animate-pulse">
          <Loader2 className="w-3.5 h-3.5 animate-spin" />
          <span>{actionMessage}</span>
        </div>
      )}

      {/* 액션 버튼 그룹 */}
      <div className="flex gap-3">
        {!isRunning ? (
          <button
            onClick={() => startCluster(cpu, memoryGb)}
            disabled={loading || !metrics}
            className="flex-1 py-2.5 px-4 bg-gradient-to-r from-blue-600 to-indigo-600 hover:from-blue-500 hover:to-indigo-500 disabled:opacity-50 text-white font-medium text-sm rounded-lg transition-all shadow-lg shadow-blue-500/20 flex items-center justify-center gap-2"
          >
            {loading ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <Play className="w-4 h-4 fill-current" />
            )}
            <span>Apple vz 기반 K8s 스타트</span>
          </button>
        ) : (
          <button
            onClick={() => stopCluster()}
            disabled={loading}
            className="flex-1 py-2.5 px-4 bg-slate-800 hover:bg-rose-900/40 text-rose-300 border border-slate-700 hover:border-rose-700/50 font-medium text-sm rounded-lg transition-all flex items-center justify-center gap-2"
          >
            {loading ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <Square className="w-4 h-4 fill-current text-rose-400" />
            )}
            <span>Colima 정지</span>
          </button>
        )}
      </div>
    </div>
  );
};
