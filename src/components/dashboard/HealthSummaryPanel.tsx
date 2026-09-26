import React, { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AlertTriangle, Loader2, RefreshCw } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';
import type { SystemHealthSummary } from '../../types/ipc';

type Tone = 'success' | 'warning' | 'muted';

const overallTone: Record<SystemHealthSummary['overall'], Tone> = {
  healthy: 'success',
  degraded: 'warning',
  unknown: 'muted',
};

export const HealthSummaryPanel: React.FC = () => {
  const { t } = useTranslation();
  const [summary, setSummary] = useState<SystemHealthSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [fetchedAt, setFetchedAt] = useState<string | null>(null);
  const [loading, setLoading] = useState<boolean>(false);
  // 탭 전환으로 컴포넌트가 언마운트되거나 수동 새로고침이 겹쳐도, get_system_health_summary
  // (colima/kubectl을 셸아웃)가 늦게 응답했을 때 죽은/낡은 응답으로 상태를 덮어쓰지 않는다.
  const requestIdRef = useRef(0);

  const fetchSummary = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    try {
      const response = await invoke<SystemHealthSummary>('get_system_health_summary');
      if (requestIdRef.current !== requestId) return; // 언마운트되었거나 더 최신 새로고침이 시작됨
      setSummary(response);
      setFetchedAt(new Date().toLocaleTimeString());
      setError(null);
    } catch (fetchError) {
      if (requestIdRef.current !== requestId) return;
      setError(String(fetchError));
    } finally {
      if (requestIdRef.current === requestId) {
        setLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    fetchSummary();
    return () => {
      requestIdRef.current += 1;
    };
  }, [fetchSummary]);

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
          onClick={fetchSummary}
          disabled={loading}
          className="shrink-0 px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-caption font-medium flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          {loading ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <RefreshCw className="w-3.5 h-3.5" />}
          <span>{loading ? t('health.refreshing') : t('health.refresh')}</span>
        </button>
      </div>

      {error && (
        <p className="rounded-lg bg-surfaceRaised p-3 text-caption text-danger flex gap-2" role="alert">
          <AlertTriangle className="w-4 h-4 shrink-0" />
          <span>{t('health.fetchFailed', { error })}</span>
        </p>
      )}

      {summary && (
        <>
          <div className="flex items-center gap-2 mb-3 text-caption text-inkMuted">
            <StatusIndicator tone={overallTone[summary.overall]} />
            <span className="text-bodyStrong text-ink">{t(`health.overall.${summary.overall}`)}</span>
            {fetchedAt && <span className="text-inkFaint">{t('health.fetchedAt', { time: fetchedAt })}</span>}
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-3 gap-3">
            <HealthComponent title={t('health.component.colima')} error={summary.colima_error}>
              {summary.colima && (
                <StatusLines lines={[
                  [t('health.colima.running'), summary.colima.is_running],
                  [t('health.colima.kubernetes'), summary.colima.kubernetes_active],
                  [t('health.colima.mlflow'), summary.colima.mlflow_ready],
                  [t('health.colima.seaweedfs'), summary.colima.seaweedfs_ready],
                  [t('health.colima.artifactStore'), summary.colima.artifact_store_wired],
                ]} valueLabel={valueLabel} />
              )}
            </HealthComponent>

            <HealthComponent title={t('health.component.guardrails')} error={summary.guardrails_error}>
              {summary.guardrails && (
                <div className="space-y-1 text-caption text-inkMuted">
                  <p>{t('health.guardrails.memoryPressure', { level: t(`health.memoryPressure.${summary.guardrails.memory_pressure_level}`) })}</p>
                  <p>{t('health.guardrails.thermal', { state: summary.guardrails.thermal_state ? t(`health.thermal.${summary.guardrails.thermal_state}`) : unavailable })}</p>
                </div>
              )}
            </HealthComponent>

            <HealthComponent title={t('health.component.kagent')} error={summary.kagent_error}>
              {summary.kagent && (
                <div className="space-y-1 text-caption text-inkMuted">
                  <p>{t('health.kagent.context', { context: summary.kagent.target_context })}</p>
                  <p>{t('health.kagent.installed')}: {valueLabel(summary.kagent.kagent_installed)}</p>
                  <p>{t('health.kagent.ready')}: {valueLabel(summary.kagent.kagent_ready)}</p>
                  <p>{t('health.kagent.podIssues', { count: summary.kagent.pod_issues_count })}</p>
                </div>
              )}
            </HealthComponent>
          </div>
        </>
      )}
    </section>
  );
};

const HealthComponent: React.FC<{ title: string; error: string | null; children: React.ReactNode }> = ({
  title,
  error,
  children,
}) => {
  const { t } = useTranslation();

  return (
    <div className="rounded-lg bg-surfaceRaised p-3 min-w-0">
      <div className="flex items-center gap-1.5 mb-2">
        <StatusIndicator tone={error ? 'warning' : 'success'} />
        <h3 className="text-bodyStrong text-ink">{title}</h3>
      </div>
      {error ? (
        <div className="text-caption text-danger">
          <p className="font-medium">{t('health.probeFailed')}</p>
          <p className="mt-1 break-words">{error}</p>
        </div>
      ) : children ? (
        <>
          <p className="text-label text-success uppercase mb-1.5">{t('health.probeSucceeded')}</p>
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
