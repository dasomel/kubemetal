import React, { useEffect, useState } from 'react';
import { Header } from './components/common/Header';
import { ErrorBoundary } from './components/common/ErrorBoundary';
import { MetricsPanel } from './components/dashboard/MetricsPanel';
import { ClusterControl } from './components/dashboard/ClusterControl';
import { LockedPreview } from './components/dashboard/LockedPreview';
import { StatusSummaryStrip } from './components/dashboard/StatusSummaryStrip';
import { ProvisionPanel } from './components/services/ProvisionPanel';
import { ModelHub } from './components/modelhub/ModelHub';
import { MlxStudio } from './components/mlx/MlxStudio';
import { PipelineView } from './components/pipeline/PipelineView';
import { AccessConsole } from './components/access/AccessConsole';
import { useColima } from './hooks/useColima';
import { useMlx } from './hooks/useMlx';
import { useServiceAccess } from './hooks/useServiceAccess';
import { Shield, Sparkles } from 'lucide-react';

type Tab = 'dashboard' | 'modelhub' | 'mlx' | 'pipeline' | 'access';

const TABS: { id: Tab; label: string }[] = [
  { id: 'dashboard', label: '대시보드' },
  { id: 'modelhub', label: '모델 허브' },
  { id: 'mlx', label: 'MLX 스튜디오' },
  { id: 'pipeline', label: '파이프라인' },
  { id: 'access', label: '접근 콘솔' },
];

// 접근 콘솔이 포트포워딩으로 회복되는 서비스와 동일한 집합(AccessConsole.tsx 참조) —
// 이 서비스들의 health로 "포워딩 활성" 여부를 근사한다(별도 IPC 없이 기존 데이터 재사용).
const FORWARDING_SERVICE_NAMES = new Set(['MLflow', 'SeaweedFS S3 API', 'SeaweedFS Filer UI']);

const BANNER_COPY: Record<'bootstrap' | 'provision' | 'ready', string> = {
  bootstrap: 'K8s 컨트롤 플레인(Colima vz VM)을 시작하면 하이브리드 MLOps 스택을 구성할 수 있습니다.',
  provision: '클러스터가 기동되었습니다 — MLOps 스택을 배포하고 포트포워딩을 연결하세요.',
  ready: 'K8s 컨트롤 플레인과 MLOps 스택, 호스트 연동까지 모두 준비되었습니다.',
};

/** 탭 라벨 옆에 붙는 가벼운 상태 dot — 색만으로 상태를 전달하지 않도록 title로 텍스트를 함께 제공한다. */
const TabDot: React.FC<{ color: 'success' | 'warning' | 'danger'; title: string; pulse?: boolean }> = ({
  color,
  title,
  pulse,
}) => (
  <span
    title={title}
    className={`w-1.5 h-1.5 rounded-full shrink-0 ${
      color === 'success' ? 'bg-success' : color === 'warning' ? 'bg-warning' : 'bg-danger'
    } ${pulse ? 'animate-pulse' : ''}`}
  />
);

export const App: React.FC = () => {
  const [activeTab, setActiveTab] = useState<Tab>('dashboard');

  // 대시보드 여정 분기와 탭 배지에 쓰는 상태 — 각 카드가 이미 자체적으로 useColima/useMlx를
  // 구독하므로(예: ClusterControl, ProvisionPanel) 여기서도 동일 훅을 재사용한다.
  // 새 폴링 루프를 추가하지 않고, 훅이 이미 내부에서 도는 주기를 그대로 따른다.
  const { status } = useColima();
  const { mlxStatus } = useMlx();
  const access = useServiceAccess();

  const clusterRunning = status?.is_running ?? false;
  const k8sActive = status?.kubernetes_active ?? false;
  const stackReady = (status?.mlflow_ready ?? false) && (status?.seaweedfs_ready ?? false);
  const integrationWired = status?.artifact_store_wired ?? false;

  const journeyStage: 'bootstrap' | 'provision' | 'ready' = !clusterRunning
    ? 'bootstrap'
    : stackReady && integrationWired
      ? 'ready'
      : 'provision';

  // 접근 콘솔 배지는 탭을 방문하지 않아도 최소 1회 조회되어야 "unreachable 수"를 보여줄 수 있다.
  // 인터벌 없이 마운트 시 1회 + 여정이 'ready' 단계로 전환될 때 1회만 재조회한다(새 폴링 루프 없음).
  useEffect(() => {
    access.refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => {
    if (journeyStage === 'ready') access.refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [journeyStage]);

  const trainingActive =
    !!mlxStatus?.training && mlxStatus.training.status !== 'done' && mlxStatus.training.status !== 'error';
  const servingActive = !!mlxStatus?.serving;
  const unreachableCount = access.loaded ? access.services.filter((s) => s.health !== 'ok').length : 0;
  const forwardingServices = access.services.filter((s) => FORWARDING_SERVICE_NAMES.has(s.service));
  const forwardingActive =
    access.loaded && forwardingServices.length > 0 && forwardingServices.every((s) => s.health === 'ok');

  return (
    <div className="min-h-screen bg-base text-ink flex flex-col font-sans">
      <div className="sticky top-0 z-40 bg-surface shadow-sm">
        <Header />
        <nav className="border-b border-hairline/8 bg-surface px-6">
          <div className="w-full flex gap-1">
            {TABS.map((tab) => (
              <button
                key={tab.id}
                type="button"
                onClick={() => setActiveTab(tab.id)}
                className={`px-4 py-3 text-label uppercase border-b-2 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary flex items-center gap-1.5 ${
                  activeTab === tab.id
                    ? 'text-primary border-primary'
                    : 'text-inkFaint border-transparent hover:text-inkMuted'
                }`}
              >
                <span>{tab.label}</span>
                {tab.id === 'mlx' && trainingActive && (
                  <TabDot color="warning" title="파인튜닝 진행 중" pulse />
                )}
                {tab.id === 'pipeline' && (trainingActive || servingActive) && (
                  <TabDot
                    color={servingActive ? 'success' : 'warning'}
                    title={servingActive ? '서빙 중' : '파이프라인 진행 중'}
                    pulse={trainingActive && !servingActive}
                  />
                )}
                {tab.id === 'access' && unreachableCount > 0 && (
                  <span
                    title={`연결 불가 서비스 ${unreachableCount}개`}
                    className="min-w-[16px] h-4 px-1 rounded-full bg-danger text-inverse text-[10px] leading-4 font-semibold flex items-center justify-center"
                  >
                    {unreachableCount}
                  </span>
                )}
              </button>
            ))}
          </div>
        </nav>
      </div>

      <main className="flex-1 p-6 space-y-4 w-full">
        <ErrorBoundary resetKey={activeTab}>
          {activeTab === 'dashboard' ? (
            <>
              {/* 상단 안내 배너 — 여정 단계에 따라 문구가 바뀐다 */}
              <div className="animate-card-in rounded-xl bg-surface p-4 flex items-center justify-between shadow-panel">
                <div className="flex items-center gap-3">
                  <div className="p-2 rounded-md bg-surfaceRaised text-primary">
                    <Sparkles className="w-5 h-5" />
                  </div>
                  <div>
                    <h2 className="text-heading text-ink">
                      Hybrid MLOps Control Dashboard
                    </h2>
                    <p className="text-caption text-inkMuted mt-0.5">{BANNER_COPY[journeyStage]}</p>
                  </div>
                </div>
                <div className="hidden sm:flex items-center gap-2 text-caption text-inkMuted bg-surfaceRaised px-3 py-1.5 rounded-md">
                  <Shield className="w-3.5 h-3.5 text-success" />
                  <span>Apple Silicon Metal Safe</span>
                </div>
              </div>

              {journeyStage === 'bootstrap' && (
                <>
                  {/* 클러스터 미기동 — 시작 카드가 히어로, 스택/포워딩은 잠긴 프리뷰 */}
                  <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                    <MetricsPanel />
                    <ClusterControl />
                  </div>
                  <LockedPreview caption="클러스터를 시작하면 MLOps 스택 배포와 포트포워딩을 진행할 수 있습니다">
                    <ProvisionPanel />
                  </LockedPreview>
                </>
              )}

              {journeyStage === 'provision' && (
                <>
                  {/* 기동됨 + 스택 미배포 — 프로비저닝이 전면, 완료 항목은 요약 배지로 접힘 */}
                  <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                    <ClusterControl compact />
                    <MetricsPanel compact />
                  </div>
                  <ProvisionPanel />
                </>
              )}

              {journeyStage === 'ready' && (
                <>
                  {/* 스택 Ready + 연동 — 전체 상태 요약 스트립을 상단에 고정 */}
                  <StatusSummaryStrip
                    cluster={clusterRunning && k8sActive}
                    stack={stackReady}
                    integration={integrationWired}
                    forwarding={forwardingActive}
                  />
                  <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                    <ClusterControl compact />
                    <MetricsPanel compact />
                  </div>
                  <ProvisionPanel />
                </>
              )}
            </>
          ) : activeTab === 'modelhub' ? (
            <ModelHub />
          ) : activeTab === 'mlx' ? (
            <MlxStudio />
          ) : activeTab === 'pipeline' ? (
            <PipelineView />
          ) : (
            <AccessConsole />
          )}
        </ErrorBoundary>
      </main>

      <footer className="border-t border-hairline/8 py-3 px-6 text-center text-caption text-inkFaint">
        KubeMetal &copy; 2026 — Apple Silicon Hybrid MLOps Architecture
      </footer>
    </div>
  );
};

export default App;
