import React, { useState } from 'react';
import { Header } from './components/common/Header';
import { MetricsPanel } from './components/dashboard/MetricsPanel';
import { ClusterControl } from './components/dashboard/ClusterControl';
import { ProvisionPanel } from './components/services/ProvisionPanel';
import { ModelHub } from './components/modelhub/ModelHub';
import { MlxStudio } from './components/mlx/MlxStudio';
import { PipelineView } from './components/pipeline/PipelineView';
import { Shield, Sparkles } from 'lucide-react';

type Tab = 'dashboard' | 'modelhub' | 'mlx' | 'pipeline';

const TABS: { id: Tab; label: string }[] = [
  { id: 'dashboard', label: '대시보드' },
  { id: 'modelhub', label: '모델 허브' },
  { id: 'mlx', label: 'MLX 스튜디오' },
  { id: 'pipeline', label: '파이프라인' },
];

export const App: React.FC = () => {
  const [activeTab, setActiveTab] = useState<Tab>('dashboard');

  return (
    <div className="min-h-screen bg-base text-ink flex flex-col font-sans">
      <Header />

      <nav className="border-b border-hairline/8 bg-surface px-6">
        <div className="max-w-7xl w-full mx-auto flex gap-1">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              type="button"
              onClick={() => setActiveTab(tab.id)}
              className={`px-4 py-3 text-label uppercase border-b-2 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary ${
                activeTab === tab.id
                  ? 'text-primary border-primary'
                  : 'text-inkFaint border-transparent hover:text-inkMuted'
              }`}
            >
              {tab.label}
            </button>
          ))}
        </div>
      </nav>

      <main className="flex-1 p-6 space-y-6 max-w-7xl w-full mx-auto">
        {activeTab === 'dashboard' ? (
          <>
            {/* 상단 안내 배너 */}
            <div className="rounded-xl bg-surface p-4 flex items-center justify-between shadow-panel">
              <div className="flex items-center gap-3">
                <div className="p-2 rounded-md bg-surfaceRaised text-primary">
                  <Sparkles className="w-5 h-5" />
                </div>
                <div>
                  <h2 className="text-heading text-ink">
                    Phase 1 MVP Hybrid MLOps Control Dashboard
                  </h2>
                  <p className="text-caption text-inkMuted mt-0.5">
                    K8s 컨트롤 플레인 (Colima vz VM)과 macOS 호스트 연동 스택을 한눈에 제어합니다.
                  </p>
                </div>
              </div>
              <div className="hidden sm:flex items-center gap-2 text-caption text-inkMuted bg-surfaceRaised px-3 py-1.5 rounded-md">
                <Shield className="w-3.5 h-3.5 text-success" />
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
          </>
        ) : activeTab === 'modelhub' ? (
          <ModelHub />
        ) : activeTab === 'mlx' ? (
          <MlxStudio />
        ) : (
          <PipelineView />
        )}
      </main>

      <footer className="border-t border-hairline/8 py-3 px-6 text-center text-caption text-inkFaint">
        KubeMetal &copy; 2026 — Apple Silicon Hybrid MLOps Architecture
      </footer>
    </div>
  );
};

export default App;
