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
    <div className="rounded-xl border border-default bg-surface p-6 shadow-card flex flex-col justify-between">
      <div>
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-section text-primary flex items-center gap-2">
            <Server className="w-5 h-5 text-accent" />
            <span>Colima K8s Control</span>
          </h2>

          <span
            className={`inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-body-strong border ${
              isRunning
                ? 'bg-success/10 text-success border-success/30'
                : 'bg-danger/10 text-danger border-danger/30'
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

        {/* 상태 세부사항 */}
        <div className="grid grid-cols-2 gap-3 mb-5">
          <div className="p-3 rounded-lg bg-surface-raised border border-default">
            <span className="text-caption text-secondary block mb-1">VM 엔진 스펙</span>
            <span className="text-body-strong text-primary">vz + virtiofs</span>
          </div>
          <div className="p-3 rounded-lg bg-surface-raised border border-default">
            <span className="text-caption text-secondary block mb-1">Kubernetes Active</span>
            <span className={`text-body-strong ${k8sActive ? 'text-success' : 'text-secondary'}`}>
              {k8sActive ? 'K3s Enabled' : 'Disabled / Inactive'}
            </span>
          </div>
        </div>

        {/* 동적 추천 할당 뱃지 */}
        <div className="p-3 rounded-lg bg-accent/10 border border-accent/20 text-body text-secondary mb-5 flex items-center gap-2">
          <ShieldCheck className="w-4 h-4 text-accent shrink-0" />
          <span>
            자동 산정 VM 스펙: <strong className="text-primary tabular-nums">{cpu} CPU 코어 / {memoryGb}GB RAM</strong> (호스트 {totalRam}GB RAM 기준 D4)
          </span>
        </div>
      </div>

      {actionMessage && (
        <div className="text-body text-accent bg-accent/10 border border-accent/20 rounded-lg p-2.5 mb-4 flex items-center gap-2">
          <Loader2 className="w-3.5 h-3.5 animate-spin shrink-0" />
          <span>{actionMessage}</span>
        </div>
      )}

      {/* 액션 버튼 그룹 */}
      <div className="flex gap-3">
        {!isRunning ? (
          <button
            onClick={() => startCluster(cpu, memoryGb)}
            disabled={loading || !metrics}
            className="flex-1 py-2.5 px-4 bg-accent-strong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse font-semibold text-body rounded-md transition-all flex items-center justify-center gap-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
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
            className="flex-1 py-2.5 px-4 bg-danger-strong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse font-semibold text-body rounded-md transition-all flex items-center justify-center gap-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-danger focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
          >
            {loading ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <Square className="w-4 h-4 fill-current" />
            )}
            <span>Colima 정지</span>
          </button>
        )}
      </div>
    </div>
  );
};
