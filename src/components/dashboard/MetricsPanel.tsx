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
    <div className="rounded-xl border border-default bg-surface p-6 shadow-card">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-section text-primary flex items-center gap-2">
          <Activity className="w-5 h-5 text-accent" />
          <span>시스템 자원 현황 (Host Metrics)</span>
        </h2>
        <span className="text-caption text-secondary bg-surface-raised px-2.5 py-1 rounded-full border border-default">
          NFR-02 (1000ms Realtime)
        </span>
      </div>

      {!metrics ? (
        <div className="py-8 text-center text-secondary text-body animate-pulse">
          시스템 메트릭을 수집하는 중...
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {/* 메모리 카드 */}
          <div className="p-4 rounded-lg bg-surface-raised border border-default">
            <div className="flex items-center gap-1.5 text-caption text-secondary mb-1">
              <Database className="w-4 h-4 text-accent" />
              <span>Unified Memory (RAM)</span>
            </div>
            <div className="flex items-baseline gap-1.5 mb-2">
              <span className="text-metric tabular-nums text-primary font-mono">{usedRam}</span>
              <span className="text-caption text-muted">/ {totalRam} GB · {memPercent}%</span>
            </div>
            {/* ProgressBar */}
            <div className="w-full h-2 bg-base rounded-full overflow-hidden mb-3" role="progressbar" aria-valuenow={memPercent} aria-valuemin={0} aria-valuemax={100}>
              <div
                className="h-full bg-accent transition-all duration-500 rounded-full"
                style={{ width: `${Math.min(memPercent, 100)}%` }}
              />
            </div>
            <p className="text-caption text-muted flex items-center gap-1">
              <Info className="w-3.5 h-3.5 shrink-0" />
              권한 없는 macOS sysinfo 모니터링 (D2)
            </p>
          </div>

          {/* CPU 카드 */}
          <div className="p-4 rounded-lg bg-surface-raised border border-default">
            <div className="flex items-center gap-1.5 text-caption text-secondary mb-1">
              <Cpu className="w-4 h-4 text-accent" />
              <span>Host CPU Usage</span>
            </div>
            <div className="flex items-baseline gap-1.5 mb-2">
              <span className="text-metric tabular-nums text-primary font-mono">{cpuPercent}</span>
              <span className="text-caption text-muted">%</span>
            </div>
            {/* ProgressBar */}
            <div className="w-full h-2 bg-base rounded-full overflow-hidden mb-3" role="progressbar" aria-valuenow={cpuPercent} aria-valuemin={0} aria-valuemax={100}>
              <div
                className="h-full bg-accent transition-all duration-500 rounded-full"
                style={{ width: `${Math.min(cpuPercent, 100)}%` }}
              />
            </div>
            <p className="text-caption text-muted flex items-center gap-1">
              <Info className="w-3.5 h-3.5 shrink-0" />
              추천 VM 자원: {vmRec.cpu} CPU / {vmRec.memoryGb}GB RAM (D4)
            </p>
          </div>
        </div>
      )}
    </div>
  );
};
