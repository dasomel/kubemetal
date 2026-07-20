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
    <div className="rounded-xl border border-slate-800 bg-slate-900/60 p-5 shadow-xl backdrop-blur">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-lg font-bold text-slate-100 flex items-center gap-2">
          <Activity className="w-5 h-5 text-blue-400" />
          <span>시스템 자원 현황 (Host Metrics)</span>
        </h2>
        <span className="text-xs text-slate-400 bg-slate-800/80 px-2.5 py-1 rounded-full border border-slate-700">
          NFR-02 (1000ms Realtime)
        </span>
      </div>

      {!metrics ? (
        <div className="py-8 text-center text-slate-400 text-sm animate-pulse">
          시스템 메트릭을 수집하는 중...
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {/* 메모리 카드 */}
          <div className="p-4 rounded-lg bg-slate-950/50 border border-slate-800/80">
            <div className="flex items-center justify-between mb-2">
              <span className="text-xs font-semibold text-slate-400 flex items-center gap-1.5">
                <Database className="w-4 h-4 text-indigo-400" />
                Unified Memory (RAM)
              </span>
              <span className="text-xs font-mono font-bold text-indigo-300">
                {usedRam} GB / {totalRam} GB ({memPercent}%)
              </span>
            </div>
            {/* ProgressBar */}
            <div className="w-full h-2.5 bg-slate-800 rounded-full overflow-hidden mb-3">
              <div
                className="h-full bg-gradient-to-r from-indigo-500 to-blue-400 transition-all duration-500 rounded-full"
                style={{ width: `${Math.min(memPercent, 100)}%` }}
              />
            </div>
            <p className="text-[11px] text-slate-400 flex items-center gap-1">
              <Info className="w-3.5 h-3.5 text-slate-500 shrink-0" />
              권한 없는 macOS sysinfo 모니터링 (D2)
            </p>
          </div>

          {/* CPU 카드 */}
          <div className="p-4 rounded-lg bg-slate-950/50 border border-slate-800/80">
            <div className="flex items-center justify-between mb-2">
              <span className="text-xs font-semibold text-slate-400 flex items-center gap-1.5">
                <Cpu className="w-4 h-4 text-cyan-400" />
                Host CPU Usage
              </span>
              <span className="text-xs font-mono font-bold text-cyan-300">
                {cpuPercent}%
              </span>
            </div>
            {/* ProgressBar */}
            <div className="w-full h-2.5 bg-slate-800 rounded-full overflow-hidden mb-3">
              <div
                className="h-full bg-gradient-to-r from-cyan-500 to-teal-400 transition-all duration-500 rounded-full"
                style={{ width: `${Math.min(cpuPercent, 100)}%` }}
              />
            </div>
            <p className="text-[11px] text-slate-400 flex items-center gap-1">
              <Info className="w-3.5 h-3.5 text-slate-500 shrink-0" />
              추천 VM 자원: {vmRec.cpu} CPU / {vmRec.memoryGb}GB RAM (D4)
            </p>
          </div>
        </div>
      )}
    </div>
  );
};
