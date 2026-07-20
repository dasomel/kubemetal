import React from 'react';
import { useColima } from '../../hooks/useColima';
import { useMetrics } from '../../hooks/useMetrics';
import { recommendVmResources } from '../../lib/recommendVmResources';
import { Server, Play, Square, Loader2, ShieldCheck } from 'lucide-react';

export const ClusterControl: React.FC = () => {
  const { status, loading, actionMessage, startCluster, stopCluster } = useColima();
  const metrics = useMetrics();

  const totalRam = metrics?.total_memory_gb ?? 16;
  const { cpu, memoryGb } = recommendVmResources(totalRam);

  const isRunning = status?.is_running ?? false;
  const k8sActive = status?.kubernetes_active ?? false;

  return (
    <div className="rounded-xl bg-surface p-6 shadow-panel flex flex-col justify-between">
      <div>
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-heading text-ink flex items-center gap-2">
            <Server className="w-4 h-4 text-primary" />
            <span>Colima K8s Control</span>
          </h2>

          <div className="flex items-center gap-1.5 text-caption text-inkMuted">
            <span
              className={`w-2 h-2 rounded-full ${isRunning ? 'bg-success' : 'bg-danger'}`}
            />
            <span>{isRunning ? 'RUNNING (vz)' : 'STOPPED'}</span>
          </div>
        </div>

        {/* 상태 세부사항 */}
        <div className="grid grid-cols-2 gap-3 mb-5">
          <div className="p-4 rounded-lg bg-surfaceRaised">
            <div className="text-label uppercase text-inkFaint mb-1">VM 엔진 스펙</div>
            <div className="text-bodyStrong text-ink">vz + virtiofs</div>
          </div>
          <div className="p-4 rounded-lg bg-surfaceRaised">
            <div className="text-label uppercase text-inkFaint mb-1">Kubernetes Active</div>
            <div className="flex items-center gap-1.5">
              <span className={`w-1.5 h-1.5 rounded-full ${k8sActive ? 'bg-success' : 'bg-inkFaint'}`} />
              <span className="text-bodyStrong text-ink">
                {k8sActive ? 'K3s Enabled' : 'Disabled / Inactive'}
              </span>
            </div>
          </div>
        </div>

        {/* 동적 추천 할당 안내 */}
        <div className="p-4 rounded-lg bg-surfaceRaised text-caption text-inkMuted mb-5 flex items-center gap-2">
          <ShieldCheck className="w-4 h-4 text-primary shrink-0" />
          <span>
            자동 산정 VM 스펙:{' '}
            <span className="text-bodyStrong text-ink tabular-nums">
              {cpu} CPU 코어 / {memoryGb}GB RAM
            </span>{' '}
            (호스트 {totalRam}GB RAM 기준)
          </span>
        </div>
      </div>

      {actionMessage && (
        <div className="text-caption text-inkMuted bg-surfaceRaised rounded-lg p-3 mb-4 flex items-center gap-2">
          <Loader2 className="w-3.5 h-3.5 animate-spin shrink-0 text-primary" />
          <span>{actionMessage}</span>
        </div>
      )}

      {/* 액션 버튼 */}
      <div className="flex gap-3">
        {!isRunning ? (
          <button
            onClick={() => startCluster(cpu, memoryGb)}
            disabled={loading || !metrics}
            className="flex-1 py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center justify-center gap-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
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
            className="flex-1 py-2.5 px-4 bg-dangerStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center justify-center gap-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-danger focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
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
