import React, { useEffect, useState } from 'react';
import { Sidebar, MainTab, SidebarBadges } from './components/layout/Sidebar';
import { Header } from './components/common/Header';
import { ErrorBoundary } from './components/common/ErrorBoundary';
import { MetricsPanel } from './components/dashboard/MetricsPanel';
import { ClusterControl } from './components/dashboard/ClusterControl';
import { LockedPreview } from './components/dashboard/LockedPreview';
import { StatusSummaryStrip } from './components/dashboard/StatusSummaryStrip';
import { ProvisionPanel } from './components/services/ProvisionPanel';
import { DeployTargetCard } from './components/services/DeployTargetCard';
import { ModelHub } from './components/modelhub/ModelHub';
import { MlxStudio } from './components/mlx/MlxStudio';
import { PipelineView } from './components/pipeline/PipelineView';
import { DataView } from './components/data/DataView';
import { AccessConsole } from './components/access/AccessConsole';
import { KagentOpsView } from './components/kagent/KagentOpsView';
import { AirgapManagerView } from './components/airgap/AirgapManagerView';
import { useColima } from './hooks/useColima';
import { useMlx } from './hooks/useMlx';
import { useServiceAccess } from './hooks/useServiceAccess';
import { useTranslation } from './i18n/i18nContext';
import { Shield, Sparkles, Terminal } from 'lucide-react';

// 접근 콘솔이 포트포워딩으로 회복되는 서비스와 동일한 집합(AccessConsole.tsx 참조) —
// 이 서비스들의 health로 "포워딩 활성" 여부를 근사한다(별도 IPC 없이 기존 데이터 재사용).
const FORWARDING_SERVICE_NAMES = new Set(['MLflow', 'SeaweedFS S3 API', 'SeaweedFS Filer UI']);

export const App: React.FC = () => {
  const [activeTab, setActiveTab] = useState<MainTab>('dashboard');
  const [sidebarCollapsed, setSidebarCollapsed] = useState<boolean>(false);
  const [showTerminalDock, setShowTerminalDock] = useState<boolean>(false);
  const { t } = useTranslation();

  // 각 카드가 이미 자체적으로 같은 훅을 구독하므로 새 폴링 루프를 추가하지 않고
  // 훅이 내부에서 도는 주기를 그대로 따른다.
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

  const bannerCopy: Record<'bootstrap' | 'provision' | 'ready', string> = {
    bootstrap: t('dashboard.bannerBootstrap'),
    provision: t('dashboard.bannerProvision'),
    ready: t('dashboard.bannerReady'),
  };

  useEffect(() => {
    access.refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (journeyStage === 'ready') access.refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [journeyStage]);

  const forwardingServices = access.services.filter((s) => FORWARDING_SERVICE_NAMES.has(s.service));
  const forwardingActive =
    access.loaded && forwardingServices.length > 0 && forwardingServices.every((s) => s.health === 'ok');

  // 탭 배지 — 탭을 열지 않아도 학습/서빙/도달 불가 상태가 보이도록 사이드바로 넘긴다.
  const trainingActive =
    !!mlxStatus?.training && mlxStatus.training.status !== 'done' && mlxStatus.training.status !== 'error';
  const servingActive = !!mlxStatus?.serving;
  const unreachableCount = access.loaded ? access.services.filter((s) => s.health !== 'ok').length : 0;

  const tabBadges: SidebarBadges = {};
  if (trainingActive) {
    tabBadges.mlx = { kind: 'dot', tone: 'warning', title: t('tabs.dotFineTuning'), pulse: true };
  }
  if (trainingActive || servingActive) {
    tabBadges.pipeline = servingActive
      ? { kind: 'dot', tone: 'success', title: t('tabs.dotServing') }
      : { kind: 'dot', tone: 'warning', title: t('tabs.dotPipeline'), pulse: true };
  }
  if (unreachableCount > 0) {
    tabBadges.access = {
      kind: 'count',
      tone: 'danger',
      title: t('tabs.unreachableCount', { count: unreachableCount }),
      value: unreachableCount,
    };
  }

  // Bottom Dock 로그 — 이미 구독 중인 실제 상태에서만 파생한다(가짜 로그 문자열 금지).
  const dockLines: { tone: 'success' | 'warning' | 'danger' | 'muted'; text: string }[] = [];
  if (status) {
    dockLines.push(
      clusterRunning
        ? {
            tone: k8sActive ? 'success' : 'warning',
            text: t(k8sActive ? 'dock.clusterK8sReady' : 'dock.clusterNoK8s'),
          }
        : { tone: 'warning', text: t('dock.clusterStopped') },
    );
  } else {
    dockLines.push({ tone: 'muted', text: t('dock.clusterUnknown') });
  }
  if (access.loaded) {
    for (const s of access.services) {
      dockLines.push({
        tone: s.health === 'ok' ? 'success' : 'danger',
        text: `${s.service} — ${s.url} (${s.health})`,
      });
    }
  } else {
    dockLines.push({ tone: 'muted', text: t('dock.accessLoading') });
  }

  const dockToneClass = {
    success: 'text-success',
    warning: 'text-warning',
    danger: 'text-danger',
    muted: 'text-inkFaint',
  } as const;

  return (
    <div className="min-h-screen bg-base text-ink flex font-sans">
      {/* 1. 모던 좌측 사이드바 내비게이션 */}
      <Sidebar
        activeTab={activeTab}
        setActiveTab={setActiveTab}
        collapsed={sidebarCollapsed}
        setCollapsed={setSidebarCollapsed}
        badges={tabBadges}
      />

      {/* 2. 우측 메인 영역 (상단 헤더 + 메인 View + 하단 Dock) */}
      <div className="flex-1 flex flex-col min-w-0 min-h-screen">
        <Header />

        <main className="flex-1 p-6 space-y-4 w-full overflow-y-auto">
          <ErrorBoundary resetKey={activeTab}>
            {activeTab === 'dashboard' ? (
              <>
                {/* 상단 여정 상태 배너 */}
                <div className="animate-card-in rounded-xl bg-surface p-4 flex items-center justify-between shadow-panel">
                  <div className="flex items-center gap-3">
                    <div className="p-2 rounded-md bg-surfaceRaised text-primary">
                      <Sparkles className="w-5 h-5" />
                    </div>
                    <div>
                      <h2 className="text-heading text-ink">{t('dashboard.bannerTitle')}</h2>
                      <p className="text-caption text-inkMuted mt-0.5">{bannerCopy[journeyStage]}</p>
                    </div>
                  </div>
                  <div className="hidden sm:flex items-center gap-2 text-caption text-inkMuted bg-surfaceRaised px-3 py-1.5 rounded-md">
                    <Shield className="w-3.5 h-3.5 text-success" />
                    <span>{t('header.metalSafe')}</span>
                  </div>
                </div>

                {/* 배포 대상은 여정 단계와 무관하게 항상 보인다 — 외부 클러스터를 고르려고
                    colima를 먼저 띄워야 한다면 그 선택 자체가 불가능해진다(D26). */}
                <DeployTargetCard />

                {journeyStage === 'bootstrap' && (
                  <>
                    <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                      <MetricsPanel />
                      <ClusterControl />
                    </div>
                    <LockedPreview caption={t('dashboard.lockedBootstrap')}>
                      <ProvisionPanel />
                    </LockedPreview>
                  </>
                )}

                {journeyStage === 'provision' && (
                  <>
                    <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                      <ClusterControl compact />
                      <MetricsPanel compact />
                    </div>
                    <ProvisionPanel />
                  </>
                )}

                {journeyStage === 'ready' && (
                  <>
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
            ) : activeTab === 'kagent' ? (
              <KagentOpsView />
            ) : activeTab === 'pipeline' ? (
              <PipelineView />
            ) : activeTab === 'modelhub' ? (
              <ModelHub />
            ) : activeTab === 'mlx' ? (
              <MlxStudio />
            ) : activeTab === 'data' ? (
              <DataView />
            ) : activeTab === 'airgap' ? (
              <AirgapManagerView />
            ) : (
              <AccessConsole />
            )}
          </ErrorBoundary>
        </main>

        {/* 3. 하단 통합 터미널 & 로그 닥 (Bottom Dock) */}
        <div className="sticky bottom-0 bg-surface border-t border-hairline/8 z-20">
          <div className="px-6 py-2 flex items-center justify-between">
            <button
              type="button"
              onClick={() => setShowTerminalDock((prev) => !prev)}
              className="flex items-center gap-2 text-caption text-inkMuted hover:text-ink font-mono font-medium focus-visible:outline-none"
            >
              <Terminal className="w-4 h-4 text-primary" />
              <span>{t('dock.title')}</span>
              <span className="text-label text-inkFaint">
                [{showTerminalDock ? t('dock.collapse') : t('dock.expand')}]
              </span>
            </button>
            <div className="text-label text-inkFaint">
              {forwardingActive ? t('dock.forwardingOn') : t('dock.forwardingOff')}
            </div>
          </div>

          {showTerminalDock && (
            <div className="px-6 pb-4 pt-1 bg-surfaceRaised border-t border-hairline/6 text-caption font-mono space-y-1 max-h-48 overflow-y-auto">
              {dockLines.map((line) => (
                <div key={line.text} className={dockToneClass[line.tone]}>
                  {line.text}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default App;
