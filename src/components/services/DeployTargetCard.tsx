import React from 'react';
import { useDeployTarget } from '../../hooks/useDeployTarget';
import { useTranslation } from '../../i18n/i18nContext';
import { Server, Radar, ShieldCheck, AlertTriangle, Save, Search } from 'lucide-react';

/** 값이 없을 때 "없음"을 지어내지 않고 미조회 상태를 그대로 드러낸다(D22). */
const Field: React.FC<{ label: string; children: React.ReactNode }> = ({ label, children }) => (
  <div className="p-3 rounded-lg bg-surfaceRaised">
    <div className="text-label uppercase text-inkFaint mb-1">{label}</div>
    <div className="text-caption text-ink">{children}</div>
  </div>
);

export const DeployTargetCard: React.FC = () => {
  const {
    target,
    contexts,
    contextsError,
    preflight,
    preflightError,
    busy,
    blockers,
    isColima,
    selectContext,
    patchTarget,
    runPreflight,
    detectBridge,
    save,
  } = useDeployTarget();
  const { t } = useTranslation();

  if (!target) {
    return (
      <div className="rounded-xl bg-surface p-4 shadow-panel">
        <div className="text-caption text-inkMuted">{t('deployTarget.loading')}</div>
      </div>
    );
  }

  const bridge = target.bridge;
  const bridgeVerified = bridge.kind === 'verified' || bridge.kind === 'keep_base';

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Server className="w-4 h-4 text-primary" />
          <span>{t('deployTarget.title')}</span>
        </h2>
        <button
          onClick={() => void save()}
          disabled={busy !== null}
          className="py-1.5 px-3 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-caption rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        >
          <Save className="w-3.5 h-3.5" />
          <span>{t('deployTarget.save')}</span>
        </button>
      </div>

      {/* 컨텍스트 선택 */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-2 mb-3">
        <div className="p-3 rounded-lg bg-surfaceRaised">
          <label htmlFor="deploy-context" className="text-label uppercase text-inkFaint mb-1 block">
            {t('deployTarget.context')}
          </label>
          {contextsError ? (
            <div className="text-caption text-danger flex items-start gap-1.5">
              <AlertTriangle className="w-3.5 h-3.5 shrink-0 mt-0.5" />
              <span>{t('deployTarget.contextsError', { error: contextsError })}</span>
            </div>
          ) : (
            <select
              id="deploy-context"
              value={target.context}
              onChange={(e) => selectContext(e.target.value)}
              className="w-full bg-surface text-ink text-caption rounded-md px-2 py-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
            >
              {!contexts.includes(target.context) && (
                <option value={target.context}>{target.context}</option>
              )}
              {contexts.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          )}
        </div>

        <div className="p-3 rounded-lg bg-surfaceRaised">
          <label htmlFor="deploy-namespace" className="text-label uppercase text-inkFaint mb-1 block">
            {t('deployTarget.namespace')}
          </label>
          <input
            id="deploy-namespace"
            value={target.namespace}
            onChange={(e) => patchTarget({ namespace: e.target.value })}
            className="w-full bg-surface text-ink text-caption rounded-md px-2 py-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          />
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-2 mb-3">
        <div className="p-3 rounded-lg bg-surfaceRaised">
          <label htmlFor="deploy-sc" className="text-label uppercase text-inkFaint mb-1 block">
            {t('deployTarget.storageClass')}
          </label>
          <input
            id="deploy-sc"
            value={target.storage_class ?? ''}
            placeholder={t('deployTarget.storageClassPlaceholder')}
            onChange={(e) => patchTarget({ storage_class: e.target.value || null })}
            className="w-full bg-surface text-ink text-caption rounded-md px-2 py-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          />
        </div>
        <div className="p-3 rounded-lg bg-surfaceRaised">
          <label htmlFor="deploy-registry" className="text-label uppercase text-inkFaint mb-1 block">
            {t('deployTarget.imageRegistry')}
          </label>
          <input
            id="deploy-registry"
            value={target.image_registry ?? ''}
            placeholder={t('deployTarget.imageRegistryPlaceholder')}
            onChange={(e) => patchTarget({ image_registry: e.target.value || null })}
            className="w-full bg-surface text-ink text-caption rounded-md px-2 py-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          />
        </div>
      </div>

      {/* 호스트 브리지 (D10) */}
      <div className="p-3 rounded-lg bg-surfaceRaised mb-3">
        <div className="flex items-center justify-between mb-1.5">
          <span className="text-label uppercase text-inkFaint">{t('deployTarget.bridge')}</span>
          <button
            onClick={() => void detectBridge()}
            disabled={busy !== null || isColima}
            title={isColima ? t('deployTarget.bridgeColimaFixed') : undefined}
            className="py-1 px-2.5 bg-surface hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-caption rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            <Radar className={`w-3.5 h-3.5 ${busy === 'bridge' ? 'animate-pulse text-primary' : 'text-primary'}`} />
            <span>{busy === 'bridge' ? t('deployTarget.bridgeDetecting') : t('deployTarget.bridgeDetect')}</span>
          </button>
        </div>

        {bridge.kind === 'keep_base' && (
          <div className="text-caption text-success flex items-center gap-1.5">
            <ShieldCheck className="w-3.5 h-3.5" />
            <span>{t('deployTarget.bridgeKeepBase')}</span>
          </div>
        )}
        {bridge.kind === 'verified' && (
          <div className="text-caption text-success flex items-center gap-1.5">
            <ShieldCheck className="w-3.5 h-3.5" />
            <span>{t('deployTarget.bridgeVerified', { host: bridge.host })}</span>
          </div>
        )}
        {bridge.kind === 'unverified' && (
          <div className="text-caption text-warning flex items-start gap-1.5">
            <AlertTriangle className="w-3.5 h-3.5 shrink-0 mt-0.5" />
            <div>
              <div>{t('deployTarget.bridgeUnverified')}</div>
              <div className="text-inkMuted mt-0.5">{bridge.reason}</div>
              {bridge.candidates.length > 0 && (
                <div className="text-inkFaint mt-0.5">
                  {t('deployTarget.bridgeCandidates', { list: bridge.candidates.join(', ') })}
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      {/* 사전점검 */}
      <div className="flex items-center gap-2 mb-3">
        <button
          onClick={() => void runPreflight()}
          disabled={busy !== null}
          className="py-2 px-3 bg-surfaceRaised hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-caption rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          <Search className={`w-3.5 h-3.5 text-primary ${busy === 'preflight' ? 'animate-pulse' : ''}`} />
          <span>{busy === 'preflight' ? t('deployTarget.preflightRunning') : t('deployTarget.preflight')}</span>
        </button>
        {!preflight && !preflightError && (
          <span className="text-caption text-inkFaint">{t('deployTarget.preflightNotRun')}</span>
        )}
      </div>

      {preflightError && (
        <div className="p-3 rounded-lg bg-surfaceRaised text-caption text-danger flex items-start gap-1.5 mb-3">
          <AlertTriangle className="w-3.5 h-3.5 shrink-0 mt-0.5" />
          <span>{t('deployTarget.preflightFailed', { error: preflightError })}</span>
        </div>
      )}

      {preflight && (
        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-2 mb-3">
          <Field label={t('deployTarget.nodes')}>
            {t('deployTarget.nodeCount', { count: preflight.node_count })}
            <div className="text-inkFaint mt-0.5 break-all">{preflight.node_ips.join(', ')}</div>
          </Field>
          <Field label={t('deployTarget.defaultSc')}>
            {preflight.default_storage_class ?? (
              <span className="text-warning">{t('deployTarget.noDefaultSc')}</span>
            )}
          </Field>
          <Field label={t('deployTarget.argocd')}>
            {!preflight.argocd_present
              ? t('deployTarget.argocdAbsent')
              : preflight.argocd_owners.length > 0
                ? <span className="text-warning">{t('deployTarget.argocdOwned', { list: preflight.argocd_owners.join(', ') })}</span>
                : <span className="text-success">{t('deployTarget.argocdFree')}</span>}
          </Field>
          <Field label={t('deployTarget.kyverno')}>
            {preflight.enforcing_policies.length > 0
              ? preflight.enforcing_policies.join(', ')
              : t('deployTarget.kyvernoNone')}
          </Field>
        </div>
      )}

      {/* 차단 사유 — 배포 버튼의 비활성 판단과 같은 근거를 그대로 보여준다. */}
      {blockers.length > 0 ? (
        <div className="p-3 rounded-lg bg-surfaceRaised">
          <div className="text-label uppercase text-warning mb-1.5 flex items-center gap-1.5">
            <AlertTriangle className="w-3.5 h-3.5" />
            {t('deployTarget.blockers')}
          </div>
          <ul className="text-caption text-inkMuted space-y-1 list-disc list-inside">
            {blockers.map((b, i) => (
              <li key={i}>{b}</li>
            ))}
          </ul>
        </div>
      ) : (
        bridgeVerified && (
          <div className="p-3 rounded-lg bg-surfaceRaised text-caption text-success flex items-center gap-1.5">
            <ShieldCheck className="w-3.5 h-3.5" />
            <span>{t('deployTarget.ready', { context: target.context, namespace: target.namespace })}</span>
          </div>
        )
      )}
    </div>
  );
};
