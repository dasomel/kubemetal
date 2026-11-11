import React, { useCallback, useEffect, useRef, useState } from 'react';
import { AlertTriangle, Check, CheckCircle2, Copy, Loader2, RefreshCw, ShieldCheck } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';
import { useSupportBundle } from '../../hooks/useSupportBundle';
import type { SystemHealthSummary } from '../../types/ipc';
import {
  createHealthSummaryController,
  defaultHealthSummarySource,
  type HealthSummaryState,
  initialHealthSummaryState,
} from './healthSummaryState';

type Tone = 'success' | 'warning' | 'muted';

const overallTone: Record<SystemHealthSummary['overall'], Tone> = {
  healthy: 'success',
  degraded: 'warning',
  unknown: 'muted',
};

export const HealthSummaryPanel: React.FC = () => {
  const bundle = useSupportBundle();
  const [copied, setCopied] = useState<boolean>(false);
  const [externalPath, setExternalPath] = useState<string>('');
  const [state, setState] = useState<HealthSummaryState>(initialHealthSummaryState);
  const controllerRef = useRef<ReturnType<typeof createHealthSummaryController> | null>(null);

  useEffect(() => {
    const controller = createHealthSummaryController(defaultHealthSummarySource, setState);
    controllerRef.current = controller;
    void controller.fetch();
    return () => {
      controller.dispose();
    };
  }, []);

  const handleRefresh = useCallback(() => {
    void controllerRef.current?.fetch();
  }, []);

  return (
    <HealthSummaryView state={state} onRefresh={handleRefresh}>
      <SupportBundleSection
        bundle={bundle}
        copied={copied}
        setCopied={setCopied}
        externalPath={externalPath}
        setExternalPath={setExternalPath}
      />
    </HealthSummaryView>
  );
};

export const HealthSummaryView: React.FC<{
  state: HealthSummaryState;
  onRefresh: () => void;
  children?: React.ReactNode;
}> = ({ state, onRefresh, children }) => {
  const { t } = useTranslation();
  const valueLabel = (value: boolean) => t(value ? 'health.value.yes' : 'health.value.no');
  const unavailable = t('health.value.unavailable');

  return (
    <section className="animate-card-in rounded-xl bg-surface p-4 shadow-panel" aria-labelledby="health-summary-title">
      <div className="flex items-start justify-between gap-3 mb-4">
        <div>
          <h2 id="health-summary-title" className="text-heading text-ink">{t('health.title')}</h2>
          <p className="text-caption text-inkMuted mt-0.5">{t('health.subtitle')}</p>
        </div>
        <button
          type="button"
          onClick={onRefresh}
          disabled={state.loading}
          className="shrink-0 px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-caption font-medium flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          {state.loading ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <RefreshCw className="w-3.5 h-3.5" />}
          <span>{state.loading ? t('health.refreshing') : t('health.refresh')}</span>
        </button>
      </div>

      {state.error !== null && (
        <p className="rounded-lg bg-surfaceRaised p-3 text-caption text-danger flex gap-2" role="alert">
          <AlertTriangle className="w-4 h-4 shrink-0" />
          <span>{t('health.fetchFailed', { error: state.error })}</span>
        </p>
      )}

      {state.summary && (
        <>
          <div className="flex items-center gap-2 mb-3 text-caption text-inkMuted">
            <StatusIndicator tone={state.stale ? 'muted' : overallTone[state.summary.overall]} />
            <span className="text-bodyStrong text-ink">
              {t(state.stale ? 'health.currentUnavailable' : `health.overall.${state.summary.overall}`)}
            </span>
            {state.fetchedAt && <span className="text-inkFaint">{t('health.fetchedAt', { time: state.fetchedAt })}</span>}
          </div>
          {state.stale && <p className="text-caption text-inkMuted mb-3">{t('health.previousSnapshot')}</p>}

          <div className="grid grid-cols-1 lg:grid-cols-3 gap-3">
            <HealthComponent title={t('health.component.colima')} error={state.summary.colima_error} stale={state.stale}>
              {state.summary.colima && (
                <StatusLines lines={[
                  [t('health.colima.running'), state.summary.colima.is_running],
                  [t('health.colima.kubernetes'), state.summary.colima.kubernetes_active],
                  [t('health.colima.mlflow'), state.summary.colima.mlflow_ready],
                  [t('health.colima.seaweedfs'), state.summary.colima.seaweedfs_ready],
                  [t('health.colima.artifactStore'), state.summary.colima.artifact_store_wired],
                ]} valueLabel={valueLabel} />
              )}
            </HealthComponent>

            <HealthComponent title={t('health.component.guardrails')} error={state.summary.guardrails_error} stale={state.stale}>
              {state.summary.guardrails && (
                <div className="space-y-1 text-caption text-inkMuted">
                  <p>{t('health.guardrails.memoryPressure', { level: t(`health.memoryPressure.${state.summary.guardrails.memory_pressure_level}`) })}</p>
                  <p>{t('health.guardrails.thermal', { state: state.summary.guardrails.thermal_state ? t(`health.thermal.${state.summary.guardrails.thermal_state}`) : unavailable })}</p>
                </div>
              )}
            </HealthComponent>

            <HealthComponent title={t('health.component.kagent')} error={state.summary.kagent_error} stale={state.stale}>
              {state.summary.kagent && (
                <div className="space-y-1 text-caption text-inkMuted">
                  <p>{t('health.kagent.context', { context: state.summary.kagent.target_context })}</p>
                  <p>{t('health.kagent.installed')}: {valueLabel(state.summary.kagent.kagent_installed)}</p>
                  <p>{t('health.kagent.ready')}: {valueLabel(state.summary.kagent.kagent_ready)}</p>
                  <p>{t('health.kagent.podIssues', { count: state.summary.kagent.pod_issues_count })}</p>
                </div>
              )}
            </HealthComponent>
          </div>
        </>
      )}

      {children}
    </section>
  );
};

const HealthComponent: React.FC<{
  title: string;
  error: string | null;
  stale: boolean;
  children: React.ReactNode;
}> = ({
  title,
  error,
  stale,
  children,
}) => {
  const { t } = useTranslation();

  return (
    <div className="rounded-lg bg-surfaceRaised p-3 min-w-0">
      <div className="flex items-center gap-1.5 mb-2">
        <StatusIndicator tone={stale ? 'muted' : error ? 'warning' : 'success'} />
        <h3 className="text-bodyStrong text-ink">{title}</h3>
      </div>
      {error ? (
        <div className="text-caption text-danger">
          <p className="font-medium">{t('health.probeFailed')}</p>
          <p className="mt-1 break-words">{error}</p>
        </div>
      ) : children ? (
        <>
          {!stale && (
            <p className="text-label text-success uppercase mb-1.5">
              {t('health.probeSucceeded')}
            </p>
          )}
          {children}
        </>
      ) : (
        <p className="text-caption text-inkFaint">{t('health.noResult')}</p>
      )}
    </div>
  );
};

const StatusLines: React.FC<{ lines: [string, boolean][]; valueLabel: (value: boolean) => string }> = ({
  lines,
  valueLabel,
}) => (
  <div className="space-y-1 text-caption text-inkMuted">
    {lines.map(([label, value]) => <p key={label}>{label}: {valueLabel(value)}</p>)}
  </div>
);

const StatusIndicator: React.FC<{ tone: Tone }> = ({ tone }) => (
  <span className={`w-2 h-2 rounded-full shrink-0 ${tone === 'success' ? 'bg-success' : tone === 'warning' ? 'bg-warning' : 'bg-inkFaint'}`} />
);

const SupportBundleSection: React.FC<{
  bundle: ReturnType<typeof useSupportBundle>;
  copied: boolean;
  setCopied: React.Dispatch<React.SetStateAction<boolean>>;
  externalPath: string;
  setExternalPath: React.Dispatch<React.SetStateAction<string>>;
}> = ({
  bundle,
  copied,
  setCopied,
  externalPath,
  setExternalPath,
}) => {
  const { t } = useTranslation();

  return (
    <div className="mt-4 space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={bundle.create}
          disabled={bundle.loading}
          className="px-3 py-2 rounded-md bg-surfaceRaised hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-caption font-medium flex items-center gap-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          {bundle.loading && <Loader2 className="w-4 h-4 animate-spin" aria-hidden="true" />}
          {t(bundle.loading ? 'health.bundle.creating' : 'health.bundle.create')}
        </button>
      </div>

      {bundle.error && (
        <p role="alert" className="text-caption text-danger break-words">
          {t('health.bundle.failed', { error: bundle.error })}
        </p>
      )}

      {bundle.result && (
        <div role="status" className="rounded-lg bg-surfaceRaised p-3 text-caption text-inkMuted space-y-2 break-words">
          <div className="space-y-1">
            <p>{t('health.bundle.created', { path: bundle.result.bundle_dir })}</p>
            <p>{t('health.bundle.manifest', { path: bundle.result.manifest_path })}</p>
            <p>{t('health.bundle.counts', { files: bundle.result.files_count, omitted: bundle.result.omitted_count })}</p>
            {bundle.result.redaction.review_before_sharing && <p className="text-inkFaint">{t('health.bundle.review')}</p>}
          </div>

          <div className="flex flex-wrap items-center gap-2 pt-1 border-t border-borderSubtle">
            <button
              type="button"
              onClick={async () => {
                if (bundle.result) {
                  await navigator.clipboard.writeText(bundle.result.bundle_dir);
                  setCopied(true);
                  setTimeout(() => setCopied(false), 2000);
                }
              }}
              className="px-2.5 py-1.5 rounded-md bg-surface text-ink text-caption font-medium flex items-center gap-1.5 hover:brightness-95 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
            >
              {copied ? <Check className="w-3.5 h-3.5 text-success" /> : <Copy className="w-3.5 h-3.5" />}
              <span>{t(copied ? 'health.bundle.copied' : 'health.bundle.copy')}</span>
            </button>

            <button
              type="button"
              onClick={() => bundle.verify()}
              disabled={bundle.verifying}
              className="px-2.5 py-1.5 rounded-md bg-surface text-ink text-caption font-medium flex items-center gap-1.5 hover:brightness-95 disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
            >
              {bundle.verifying ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
              ) : (
                <ShieldCheck className="w-3.5 h-3.5" />
              )}
              <span>{t(bundle.verifying ? 'health.bundle.verifying' : 'health.bundle.verify')}</span>
            </button>
          </div>
        </div>
      )}

      {bundle.verificationReport && (
        <div
          role="status"
          className={`rounded-lg p-3 text-caption space-y-1.5 ${
            bundle.verificationReport.verified
              ? 'bg-success/10 text-success'
              : 'bg-danger/10 text-danger'
          }`}
        >
          <div className="flex items-center gap-1.5 font-medium">
            {bundle.verificationReport.verified ? (
              <CheckCircle2 className="w-4 h-4 shrink-0" />
            ) : (
              <AlertTriangle className="w-4 h-4 shrink-0" />
            )}
            <span>
              {t(
                bundle.verificationReport.verified
                  ? 'health.bundle.verifiedPass'
                  : 'health.bundle.verifiedFail',
                {
                  count: bundle.verificationReport.verified
                    ? bundle.verificationReport.files_checked
                    : bundle.verificationReport.problems.length,
                }
              )}
            </span>
          </div>
          {bundle.verificationReport.problems.length > 0 && (
            <ul className="list-disc list-inside space-y-0.5 text-inkMuted pl-2">
              {bundle.verificationReport.problems.slice(0, 5).map((p, idx) => (
                <li key={idx} className="break-words">
                  [{p.category}] {p.message}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {bundle.verifyError && (
        <p role="alert" className="text-caption text-danger break-words">
          {t('health.bundle.verifyError', { error: bundle.verifyError })}
        </p>
      )}

      <div className="pt-2 border-t border-borderSubtle">
        <div className="flex gap-2">
          <input
            type="text"
            value={externalPath}
            onChange={(e) => setExternalPath(e.target.value)}
            placeholder={t('health.bundle.importPlaceholder')}
            className="flex-1 px-2.5 py-1.5 rounded-md bg-surfaceRaised text-caption text-ink placeholder:text-inkFaint border border-borderSubtle focus:outline-none focus:ring-1 focus:ring-primary"
          />
          <button
            type="button"
            onClick={() => externalPath.trim() && bundle.verify(externalPath.trim())}
            disabled={bundle.verifying || !externalPath.trim()}
            className="px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 disabled:opacity-50 text-caption font-medium text-ink shrink-0 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary"
          >
            {t('health.bundle.importVerify')}
          </button>
        </div>
      </div>
    </div>
  );
};
