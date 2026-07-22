import React from 'react';
import { useColima } from '../../hooks/useColima';
import { useMetrics } from '../../hooks/useMetrics';
import { recommendVmResources } from '../../lib/recommendVmResources';
import { useTranslation } from '../../i18n/i18nContext';
import { Server, Play, Square, Loader2, ShieldCheck } from 'lucide-react';

interface ClusterControlProps {
  /** 여정상 다음 단계로 전면에 나설 때만 전체 카드로, 그 외엔 요약 배지로 접는다. */
  compact?: boolean;
}

export const ClusterControl: React.FC<ClusterControlProps> = ({ compact = false }) => {
  const { status, loading, actionMessage, startCluster, stopCluster } = useColima();
  const metrics = useMetrics();
  const { t } = useTranslation();

  const totalRam = metrics?.total_memory_gb ?? 16;
  const { cpu, memoryGb } = recommendVmResources(totalRam);

  const isRunning = status?.is_running ?? false;
  const k8sActive = status?.kubernetes_active ?? false;

  if (compact) {
    return (
      <div className="animate-card-in rounded-xl bg-surface p-4 shadow-panel flex items-center justify-between gap-3 flex-wrap">
        <div className="flex items-center gap-2 min-w-0">
          <Server className="w-4 h-4 text-primary shrink-0" />
          <span className="text-bodyStrong text-ink truncate">{t('cluster.title')}</span>
          <span className="flex items-center gap-1.5 text-caption text-inkMuted shrink-0">
            <span className={`w-2 h-2 rounded-full ${isRunning ? 'bg-success' : 'bg-danger'}`} />
            <span>{isRunning ? t('cluster.running') : t('cluster.stopped')}</span>
          </span>
          <span className="flex items-center gap-1.5 text-caption text-inkFaint shrink-0">
            <span className={`w-1.5 h-1.5 rounded-full ${k8sActive ? 'bg-success' : 'bg-inkFaint'}`} />
            <span>{k8sActive ? t('cluster.k3sEnabled') : t('cluster.k3sDisabled')}</span>
          </span>
        </div>
        {isRunning && (
          <button
            onClick={() => stopCluster()}
            disabled={loading}
            className="shrink-0 px-3 py-1.5 bg-surfaceRaised hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-inkMuted text-caption rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            {loading ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Square className="w-3.5 h-3.5" />}
            <span>{t('cluster.stop')}</span>
          </button>
        )}
      </div>
    );
  }

  return (
    <div className="animate-card-in rounded-xl bg-surface p-4 shadow-panel flex flex-col justify-between">
      <div>
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-heading text-ink flex items-center gap-2">
            <Server className="w-4 h-4 text-primary" />
            <span>{t('cluster.title')}</span>
          </h2>

          <div className="flex items-center gap-1.5 text-caption text-inkMuted">
            <span
              className={`w-2 h-2 rounded-full ${isRunning ? 'bg-success' : 'bg-danger'}`}
            />
            <span>{isRunning ? t('cluster.running') : t('cluster.stopped')}</span>
          </div>
        </div>

        {/* 상태 세부사항 */}
        <div className="grid grid-cols-2 gap-2 mb-4">
          <div className="p-3 rounded-lg bg-surfaceRaised">
            <div className="text-label uppercase text-inkFaint mb-1">{t('cluster.vmSpecLabel')}</div>
            <div className="text-bodyStrong text-ink">vz + virtiofs</div>
          </div>
          <div className="p-3 rounded-lg bg-surfaceRaised">
            <div className="text-label uppercase text-inkFaint mb-1">{t('cluster.k8sActiveLabel')}</div>
            <div className="flex items-center gap-1.5">
              <span className={`w-1.5 h-1.5 rounded-full ${k8sActive ? 'bg-success' : 'bg-inkFaint'}`} />
              <span className="text-bodyStrong text-ink">
                {k8sActive ? t('cluster.k3sEnabled') : t('cluster.k3sInactive')}
              </span>
            </div>
          </div>
        </div>

        {/* 동적 추천 할당 안내 */}
        <div className="p-3 rounded-lg bg-surfaceRaised text-caption text-inkMuted mb-4 flex items-center gap-2">
          <ShieldCheck className="w-4 h-4 text-primary shrink-0" />
          <span>
            {t('cluster.autoSpecInfo', { cpu, memoryGb, totalRam })}
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
            <span>{t('cluster.startBtn')}</span>
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
            <span>{t('cluster.stopBtn')}</span>
          </button>
        )}
      </div>
    </div>
  );
};
