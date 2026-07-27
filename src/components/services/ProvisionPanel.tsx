import React from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useColima } from '../../hooks/useColima';
import { useDeployTarget } from '../../hooks/useDeployTarget';
import { usePrefect } from '../../hooks/usePrefect';
import { useTranslation } from '../../i18n/i18nContext';
import { Boxes, ExternalLink, RefreshCw, Zap, ArrowUpRight, Radio } from 'lucide-react';

const openEndpoint = (url: string) => {
  openUrl(url).catch(() => window.open(url, '_blank'));
};

export const ProvisionPanel: React.FC = () => {
  const {
    status,
    loading,
    portForwardStatus,
    provisionStack,
    startPortForward,
    stopPortForward,
    refresh,
  } = useColima();
  const { t } = useTranslation();

  const { target, isColima } = useDeployTarget();

  const isRunning = status?.is_running ?? false;
  const k8sActive = status?.kubernetes_active ?? false;

  // colima 대상일 때만 VM 기동을 전제로 한다. 외부 클러스터는 이 앱이 수명주기를
  // 소유하지 않으므로 colima 상태로 버튼을 막으면 영영 배포할 수 없다(D26).
  const clusterUsable = isColima ? isRunning && k8sActive : true;

  // 브리지 주소를 하드코딩하지 않는다 — 대상마다 다르고, 미검증이면 그 사실을 드러내야 한다.
  const bridgeLabel =
    target?.bridge.kind === 'verified'
      ? target.bridge.host
      : target?.bridge.kind === 'unverified'
        ? t('provision.bridgeUnverified')
        : 'host.lima.internal';
  const mlflowReady = status?.mlflow_ready ?? false;
  const seaweedfsReady = status?.seaweedfs_ready ?? false;
  const artifactStoreWired = status?.artifact_store_wired ?? false;

  // 파이프라인 탭이 아닌 대시보드 카드이므로 5초 폴링 없이 마운트(탭 진입) 시 1회만 조회한다.
  const { status: prefectStatus } = usePrefect(false);
  const prefectReady = prefectStatus?.server_ready ?? false;

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Boxes className="w-4 h-4 text-primary" />
          <span>{t('provision.title')}</span>
        </h2>

        <button
          onClick={() => refresh()}
          className="p-1.5 rounded-md bg-surfaceRaised hover:brightness-95 text-inkMuted hover:text-ink transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
          title={t('provision.refreshTooltip')}
        >
          <RefreshCw className="w-4 h-4" />
        </button>
      </div>

      {/* 스택 준비 상태 목록 */}
      <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-2 mb-4">
        {/* MLflow */}
        <div className="p-3 rounded-lg bg-surfaceRaised flex items-center justify-between">
          <div>
            <div className="text-bodyStrong text-ink">{t('provision.mlflowTracking')}</div>
            <div className="text-caption text-inkFaint mt-0.5">Port 5001</div>
          </div>
          <div className="flex flex-col items-end gap-1">
            <div className="flex items-center gap-1.5 text-caption text-inkMuted">
              <span className={`w-2 h-2 rounded-full ${mlflowReady ? 'bg-success' : 'bg-inkFaint'}`} />
              <span>{mlflowReady ? t('provision.mlflowReady') : t('provision.mlflowNotReady')}</span>
            </div>
            <div className={`flex items-center gap-1.5 text-caption ${artifactStoreWired ? 'text-success' : 'text-inkFaint'}`}>
              <span className={`w-2 h-2 rounded-full ${artifactStoreWired ? 'bg-success' : 'bg-inkFaint'}`} />
              <span>{artifactStoreWired ? t('provision.seaweedWired') : t('provision.seaweedPending')}</span>
            </div>
          </div>
        </div>

        {/* SeaweedFS */}
        <div className="p-3 rounded-lg bg-surfaceRaised flex items-center justify-between">
          <div>
            <div className="text-bodyStrong text-ink">{t('provision.seaweedfsStorage')}</div>
            <div className="text-caption text-inkFaint mt-0.5">Port 8333/8888</div>
          </div>
          <div className="flex items-center gap-1.5 text-caption text-inkMuted">
            <span className={`w-2 h-2 rounded-full ${seaweedfsReady ? 'bg-success' : 'bg-inkFaint'}`} />
            <span>{seaweedfsReady ? t('provision.seaweedReady') : t('provision.seaweedNotReady')}</span>
          </div>
        </div>

        {/* GPU Bridge */}
        <div className="p-3 rounded-lg bg-surfaceRaised flex items-center justify-between">
          <div>
            <div className="text-bodyStrong text-ink">{t('provision.macGpuBridge')}</div>
            <div className="text-caption text-inkFaint mt-0.5">{bridgeLabel}</div>
          </div>
          <span
            className={`text-caption ${
              target?.bridge.kind === 'unverified' ? 'text-warning' : 'text-primary'
            }`}
          >
            {target?.bridge.kind === 'verified'
              ? t('provision.bridgeEndpoints')
              : t('provision.externalName')}
          </span>
        </div>

        {/* Prefect */}
        <div className="p-3 rounded-lg bg-surfaceRaised flex items-center justify-between">
          <div>
            <div className="text-bodyStrong text-ink">{t('provision.prefectOrchestration')}</div>
            <div className="text-caption text-inkFaint mt-0.5">Port 4200</div>
          </div>
          <div className="flex items-center gap-1.5 text-caption text-inkMuted">
            <span className={`w-2 h-2 rounded-full ${prefectReady ? 'bg-success' : 'bg-inkFaint'}`} />
            <span>{prefectReady ? t('provision.prefectReady') : t('provision.prefectNotReady')}</span>
          </div>
        </div>
      </div>

      {/* 액션 버튼 */}
      <div className="flex flex-wrap gap-3 mb-4">
        <button
          onClick={() => provisionStack()}
          disabled={loading || !clusterUsable}
          className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        >
          <Zap className="w-3.5 h-3.5" />
          <span>{t('provision.deployBtn')}</span>
        </button>

        <button
          onClick={() => startPortForward()}
          disabled={loading || !clusterUsable}
          className="py-2.5 px-4 bg-surfaceRaised hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        >
          <Radio
            className={`w-3.5 h-3.5 ${
              portForwardStatus && portForwardStatus.active < portForwardStatus.total
                ? 'text-warning'
                : 'text-primary'
            }`}
          />
          <span>
            {portForwardStatus
              ? portForwardStatus.active === portForwardStatus.total
                ? t('provision.forwardingActive', { active: portForwardStatus.active, total: portForwardStatus.total })
                : t('provision.forwardingPartial', { active: portForwardStatus.active, total: portForwardStatus.total })
              : t('provision.startForwardingBtn')}
          </span>
        </button>

        <button
          onClick={() => stopPortForward()}
          disabled={loading || !clusterUsable}
          className="py-2.5 px-4 bg-surfaceRaised hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-inkMuted text-bodyStrong rounded-md transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        >
          {t('provision.stopForwardingBtn')}
        </button>
      </div>

      {/* 포트 바인딩 바로가기 가이드 */}
      <div className="pt-4 border-t border-hairline/8">
        <h3 className="text-label uppercase text-inkFaint mb-2 flex items-center gap-1.5">
          <ExternalLink className="w-3.5 h-3.5" /> {t('provision.hostEndpoints')}
        </h3>
        <div className="flex flex-wrap gap-2 text-caption">
          <button
            type="button"
            onClick={() => openEndpoint('http://localhost:5001')}
            className="px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 text-primary flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            MLflow UI (http://localhost:5001)
            <ArrowUpRight className="w-3 h-3" />
          </button>
          <button
            type="button"
            onClick={() => openEndpoint('http://localhost:8888')}
            className="px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 text-primary flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            SeaweedFS Filer UI (http://localhost:8888)
            <ArrowUpRight className="w-3 h-3" />
          </button>
          <span className="px-3 py-1.5 rounded-md bg-surfaceRaised text-inkMuted">
            SeaweedFS S3 API (http://localhost:8333)
          </span>
          <span className="px-3 py-1.5 rounded-md bg-surfaceRaised text-inkMuted">
            Model Serving (http://127.0.0.1:8080/v1/models)
          </span>
        </div>
      </div>
    </div>
  );
};
