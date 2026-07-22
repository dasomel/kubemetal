import React from 'react';
import { useMetrics } from '../../hooks/useMetrics';
import { recommendVmResources } from '../../lib/recommendVmResources';
import { useTranslation } from '../../i18n/i18nContext';
import { Cpu, Database, Activity, Info } from 'lucide-react';

interface MetricsPanelProps {
  /** 여정상 보조 정보로 물러설 때 한 줄 요약으로 축소한다. */
  compact?: boolean;
}

export const MetricsPanel: React.FC<MetricsPanelProps> = ({ compact = false }) => {
  const metrics = useMetrics();
  const { t } = useTranslation();

  const totalRam = metrics?.total_memory_gb ?? 0;
  const usedRam = metrics?.used_memory_gb ?? 0;
  const memPercent = metrics?.memory_usage_percentage ?? 0;
  const cpuPercent = metrics?.cpu_usage_percentage ?? 0;

  const gpuPercent = metrics?.gpu_usage_percentage ?? 0;
  const gpuMemGb = metrics?.gpu_memory_used_gb ?? 0;

  const vmRec = recommendVmResources(totalRam);

  if (compact) {
    return (
      <div className="animate-card-in rounded-xl bg-surface p-4 shadow-panel flex items-center gap-5 flex-wrap">
        <div className="flex items-center gap-1.5 text-caption text-inkMuted shrink-0">
          <Activity className="w-3.5 h-3.5 text-primary" />
          <span className="text-bodyStrong text-ink">{t('metrics.systemResources')}</span>
        </div>
        {!metrics ? (
          <div className="flex items-center gap-2 flex-1 min-w-[120px]">
            <div className="h-3 w-24 rounded-full bg-surfaceRaised animate-pulse" />
            <div className="h-3 w-16 rounded-full bg-surfaceRaised animate-pulse" />
          </div>
        ) : (
          <>
            <div className="flex items-center gap-1.5 text-caption text-inkMuted">
              <Database className="w-3.5 h-3.5 text-inkFaint" />
              <span className="tabular-nums text-ink">{usedRam}</span>
              <span className="text-inkFaint">/ {totalRam}GB · {memPercent}%</span>
            </div>
            <div className="flex items-center gap-1.5 text-caption text-inkMuted">
              <Cpu className="w-3.5 h-3.5 text-inkFaint" />
              <span className="tabular-nums text-ink">{cpuPercent}%</span>
              <span className="text-inkFaint">CPU</span>
            </div>
            <div className="flex items-center gap-1.5 text-caption text-inkMuted">
              <Activity className="w-3.5 h-3.5 text-primary shrink-0" />
              <span className="tabular-nums text-ink">{gpuPercent}%</span>
              <span className="text-inkFaint">GPU ({gpuMemGb}GB)</span>
            </div>
          </>
        )}
      </div>
    );
  }

  return (
    <div className="animate-card-in rounded-xl bg-surface p-4 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">{t('metrics.hostMetrics')}</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Activity className="w-4 h-4 text-primary" />
          <span>{t('metrics.title')}</span>
        </h2>
      </div>

      {!metrics ? (
        <div className="grid grid-cols-1 md:grid-cols-3 gap-3" aria-busy="true">
          {[0, 1, 2].map((i) => (
            <div key={i} className="p-3 rounded-lg bg-surfaceRaised">
              <div className="h-3 w-28 rounded-full bg-base animate-pulse mb-3" />
              <div className="h-8 w-20 rounded-md bg-base animate-pulse mb-3" />
              <div className="h-1.5 w-full rounded-full bg-base animate-pulse mb-3" />
              <div className="h-3 w-40 rounded-full bg-base animate-pulse" />
            </div>
          ))}
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          {/* 메모리 카드 */}
          <div className="p-3 rounded-lg bg-surfaceRaised">
            <div className="flex items-center gap-1.5 text-label uppercase text-inkFaint mb-2">
              <Database className="w-3.5 h-3.5 text-primary" />
              <span>{t('metrics.unifiedMemory')}</span>
            </div>
            <div className="flex items-baseline gap-1.5 mb-2">
              <span className="text-metric tabular-nums text-ink font-mono">{usedRam}</span>
              <span className="text-caption text-inkFaint">/ {totalRam} GB · {memPercent}%</span>
            </div>
            {/* ProgressBar */}
            <div className="w-full h-1.5 bg-base rounded-full overflow-hidden mb-3" role="progressbar" aria-valuenow={memPercent} aria-valuemin={0} aria-valuemax={100}>
              <div
                className="h-full bg-primary transition-all duration-500 rounded-full"
                style={{ width: `${Math.min(memPercent, 100)}%` }}
              />
            </div>
            <p className="text-caption text-inkFaint flex items-center gap-1">
              <Info className="w-3.5 h-3.5 shrink-0" />
              {t('metrics.sysinfoNote')}
            </p>
          </div>

          {/* CPU 카드 */}
          <div className="p-3 rounded-lg bg-surfaceRaised">
            <div className="flex items-center gap-1.5 text-label uppercase text-inkFaint mb-2">
              <Cpu className="w-3.5 h-3.5 text-primary" />
              <span>{t('metrics.hostCpuUsage')}</span>
            </div>
            <div className="flex items-baseline gap-1.5 mb-2">
              <span className="text-metric tabular-nums text-ink font-mono">{cpuPercent}</span>
              <span className="text-caption text-inkFaint">%</span>
            </div>
            {/* ProgressBar */}
            <div className="w-full h-1.5 bg-base rounded-full overflow-hidden mb-3" role="progressbar" aria-valuenow={cpuPercent} aria-valuemin={0} aria-valuemax={100}>
              <div
                className="h-full bg-primary transition-all duration-500 rounded-full"
                style={{ width: `${Math.min(cpuPercent, 100)}%` }}
              />
            </div>
            <p className="text-caption text-inkFaint flex items-center gap-1">
              <Info className="w-3.5 h-3.5 shrink-0" />
              {t('metrics.recommendedVm', { cpu: vmRec.cpu, memoryGb: vmRec.memoryGb })}
            </p>
          </div>

          {/* GPU 카드 */}
          <div className="p-3 rounded-lg bg-surfaceRaised">
            <div className="flex items-center gap-1.5 text-label uppercase text-inkFaint mb-2">
              <Activity className="w-3.5 h-3.5 text-primary" />
              <span>Apple Metal GPU</span>
            </div>
            <div className="flex items-baseline gap-1.5 mb-2">
              <span className="text-metric tabular-nums text-ink font-mono">{gpuPercent}</span>
              <span className="text-caption text-inkFaint">% 사용 · {gpuMemGb} GB</span>
            </div>
            {/* ProgressBar */}
            <div className="w-full h-1.5 bg-base rounded-full overflow-hidden mb-3" role="progressbar" aria-valuenow={gpuPercent} aria-valuemin={0} aria-valuemax={100}>
              <div
                className="h-full bg-primary transition-all duration-500 rounded-full"
                style={{ width: `${Math.min(gpuPercent, 100)}%` }}
              />
            </div>
            <p className="text-caption text-inkFaint flex items-center gap-1">
              <Info className="w-3.5 h-3.5 shrink-0" />
              ioreg Metal 가속 모니터링
            </p>
          </div>
        </div>
      )}
    </div>
  );
};
