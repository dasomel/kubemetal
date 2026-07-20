import React from 'react';
import { useMetrics } from '../../hooks/useMetrics';
import { recommendVmResources } from '../../lib/recommendVmResources';
import { Cpu, Database, Activity, Info } from 'lucide-react';

export const MetricsPanel: React.FC = () => {
  const metrics = useMetrics();

  const totalRam = metrics?.total_memory_gb ?? 0;
  const usedRam = metrics?.used_memory_gb ?? 0;
  const memPercent = metrics?.memory_usage_percentage ?? 0;
  const cpuPercent = metrics?.cpu_usage_percentage ?? 0;

  const vmRec = recommendVmResources(totalRam);

  return (
    <div className="rounded-xl bg-surface p-6 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">Host Metrics</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Activity className="w-4 h-4 text-primary" />
          <span>시스템 자원 현황</span>
        </h2>
      </div>

      {!metrics ? (
        <div className="py-8 text-center text-inkMuted text-body animate-pulse">
          시스템 메트릭을 수집하는 중...
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {/* 메모리 카드 */}
          <div className="p-4 rounded-lg bg-surfaceRaised">
            <div className="flex items-center gap-1.5 text-label uppercase text-inkFaint mb-2">
              <Database className="w-3.5 h-3.5 text-primary" />
              <span>Unified Memory</span>
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
              권한 없는 macOS sysinfo 모니터링
            </p>
          </div>

          {/* CPU 카드 */}
          <div className="p-4 rounded-lg bg-surfaceRaised">
            <div className="flex items-center gap-1.5 text-label uppercase text-inkFaint mb-2">
              <Cpu className="w-3.5 h-3.5 text-primary" />
              <span>Host CPU Usage</span>
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
              추천 VM 자원: {vmRec.cpu} CPU / {vmRec.memoryGb}GB RAM
            </p>
          </div>
        </div>
      )}
    </div>
  );
};
