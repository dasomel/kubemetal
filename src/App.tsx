import React from 'react';
import { Header } from './components/common/Header';
import { MetricsPanel } from './components/dashboard/MetricsPanel';
import { ClusterControl } from './components/dashboard/ClusterControl';
import { ProvisionPanel } from './components/services/ProvisionPanel';
import { Shield, Sparkles } from 'lucide-react';

export const App: React.FC = () => {
  return (
    <div className="min-h-screen bg-slate-950 text-slate-100 flex flex-col font-sans">
      <Header />

      <main className="flex-1 p-6 space-y-6 max-w-7xl w-full mx-auto">
        {/* 상단 안내 배너 */}
        <div className="rounded-xl bg-gradient-to-r from-blue-900/30 via-indigo-900/20 to-purple-900/30 border border-blue-800/40 p-4 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-lg bg-blue-500/10 text-blue-400 border border-blue-500/20">
              <Sparkles className="w-5 h-5" />
            </div>
            <div>
              <h2 className="text-sm font-semibold text-slate-200">
                Phase 1 MVP Hybrid MLOps Control Dashboard
              </h2>
              <p className="text-xs text-slate-400 mt-0.5">
                K8s 컨트롤 플레인 (Colima vz VM)과 macOS 호스트 연동 스택을 한눈에 제어합니다.
              </p>
            </div>
          </div>
          <div className="hidden sm:flex items-center gap-2 text-xs text-slate-400 bg-slate-900/80 px-3 py-1.5 rounded-lg border border-slate-800">
            <Shield className="w-3.5 h-3.5 text-emerald-400" />
            <span>Apple Silicon Metal Safe</span>
          </div>
        </div>

        {/* 메트릭 및 클러스터 제어 대시보드 */}
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
          <MetricsPanel />
          <ClusterControl />
        </div>

        {/* MLOps 서비스 프로비저닝 패널 */}
        <ProvisionPanel />
      </main>

      <footer className="border-t border-slate-900 py-3 px-6 text-center text-xs text-slate-600">
        KubeMetal &copy; 2026 — Apple Silicon Hybrid MLOps Architecture
      </footer>
    </div>
  );
};

export default App;
