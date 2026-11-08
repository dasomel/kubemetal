import React, { useCallback, useEffect, useRef, useState } from 'react';
import { AlertTriangle, CheckCircle2, Cpu, Loader2, RefreshCw } from 'lucide-react';
import { getCumetalStatus } from '../../hooks/useCumetal';
import { useTranslation } from '../../i18n/i18nContext';
import type { CumetalStatus } from '../../types/ipc';

// D50: 읽기 전용 탐지 카드. 설치/컴파일/실행 버튼은 두지 않는다(실행은 D51 CumetalSmokeCard). 백엔드가 돌려준 값만 표시한다(D22).
export const CumetalCard: React.FC = () => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<CumetalStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);
  const requestIdRef = useRef(0);

  const refresh = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    if (mountedRef.current) {
      setLoading(true);
      setError(null);
    }
    try {
      const next = await getCumetalStatus();
      if (mountedRef.current && requestId === requestIdRef.current) {
        setStatus(next);
      }
    } catch (err) {
      if (mountedRef.current && requestId === requestIdRef.current) {
        setStatus(null);
        setError(String(err));
      }
    } finally {
      if (mountedRef.current && requestId === requestIdRef.current) {
        setLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    void refresh();
    return () => {
      mountedRef.current = false;
    };
  }, [refresh]);

  const rows: Array<{ key: string; label: string; value: string | null; error: string | null }> = status
    ? [
        { key: 'metal', label: 'metal', value: status.metal_toolchain.metal, error: status.metal_toolchain.metal_error },
        {
          key: 'metallib',
          label: 'metallib',
          value: status.metal_toolchain.metallib,
          error: status.metal_toolchain.metallib_error,
        },
        { key: 'cumetal', label: 'cumetal', value: status.cumetal_path, error: status.cumetal_error },
        { key: 'cumetalc', label: 'cumetalc', value: status.cumetalc_path, error: status.cumetalc_error },
        { key: 'version', label: t('mlx.cumetal.version'), value: status.version, error: status.version_error },
      ]
    : [];
  const toolchainMissing =
    status !== null && (status.metal_toolchain.metal === null || status.metal_toolchain.metallib === null);

  return (
    <section className="rounded-xl bg-surface p-4 shadow-panel" aria-live="polite">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div>
          <div className="text-label uppercase text-inkFaint mb-1">MLX</div>
          <h2 className="text-heading text-ink flex items-center gap-2">
            <Cpu className="w-4 h-4 text-primary" />
            <span>{t('mlx.cumetal.title')}</span>
          </h2>
        </div>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={loading}
          className="px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-caption flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
          <span>{t('mlx.cumetal.refresh')}</span>
        </button>
      </div>

      <p className="mb-3 text-caption text-inkMuted">{t('mlx.cumetal.noTelemetry')}</p>

      {loading && status === null && !error && (
        <div className="flex items-center gap-2 py-4 text-body text-inkMuted">
          <Loader2 className="w-4 h-4 animate-spin text-primary" />
          <span>{t('mlx.cumetal.loading')}</span>
        </div>
      )}

      {error && (
        <div role="alert" className="mb-3 flex items-start gap-1.5 text-caption text-danger">
          <AlertTriangle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
          <span className="break-all">{error}</span>
        </div>
      )}

      {status && (
        <div className="space-y-2">
          <div className="rounded-lg bg-surfaceRaised p-3 space-y-1 min-w-0">
            <span
              className={`text-bodyStrong flex items-center gap-1.5 ${
                status.state === 'available' ? 'text-success' : 'text-warning'
              }`}
            >
              {status.state === 'available' ? (
                <CheckCircle2 className="w-4 h-4 shrink-0" />
              ) : (
                <AlertTriangle className="w-4 h-4 shrink-0" />
              )}
              {t(`mlx.cumetal.state.${status.state}`)}
            </span>
            {status.reason !== null && (
              <p className="break-all text-caption text-inkMuted">
                {t('mlx.cumetal.reason')}: {status.reason}
              </p>
            )}
          </div>

          <ul className="space-y-2">
            {rows.map((row) => (
              <li key={row.key} className="rounded-lg bg-surfaceRaised p-3 space-y-1 min-w-0">
                <div className="text-caption text-inkMuted">{row.label}</div>
                {row.value !== null ? (
                  <code className="block break-all text-caption text-ink">{row.value}</code>
                ) : (
                  <p className="break-all text-caption text-danger">{row.error ?? t('mlx.cumetal.notRun')}</p>
                )}
              </li>
            ))}
            <li className="rounded-lg bg-surfaceRaised p-3 space-y-1 min-w-0">
              <div className="text-caption text-inkMuted">doctor</div>
              {status.doctor ? (
                <>
                  <p className={`text-caption ${status.doctor.success ? 'text-success' : 'text-danger'}`}>
                    {status.doctor.success ? t('mlx.cumetal.doctorOk') : t('mlx.cumetal.doctorFailed')}
                    {status.doctor.exit_code !== null && ` · exit ${status.doctor.exit_code}`}
                  </p>
                  {status.doctor.output_excerpt && (
                    <pre className="whitespace-pre-wrap break-all text-caption text-inkMuted">
                      {status.doctor.output_excerpt}
                      {status.doctor.truncated && ` …${t('mlx.cumetal.truncated')}`}
                    </pre>
                  )}
                </>
              ) : (
                <p className="break-all text-caption text-danger">{status.doctor_error ?? t('mlx.cumetal.notRun')}</p>
              )}
            </li>
          </ul>

          {toolchainMissing && (
            <>
              <p className="text-caption text-inkMuted">{t('mlx.cumetal.aotNote')}</p>
              <p className="text-caption text-inkMuted">{t('mlx.cumetal.installHint')}</p>
            </>
          )}
        </div>
      )}
    </section>
  );
};
