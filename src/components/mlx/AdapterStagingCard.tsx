import React, { useCallback, useEffect, useRef, useState } from 'react';
import { AlertTriangle, Archive, Loader2, RefreshCw } from 'lucide-react';
import { listAdapterStaging, type AdapterAttemptStatus, type AdapterAttemptSummary } from '../../hooks/useMlx';
import { useTranslation } from '../../i18n/i18nContext';

// D46: 읽기 전용 인벤토리. 삭제/승격/재개 동작은 각자의 흐름에 있으므로 여기에 두지 않는다.
// 백엔드가 돌려준 값만 표시한다 — 추론/보정 없음(D22).
const reasonOf = (status: AdapterAttemptStatus): string | null =>
  status.kind === 'unknown' || status.kind === 'scan_failed' ? status.reason : null;

export const AdapterStagingCard: React.FC = () => {
  const { t } = useTranslation();
  const [attempts, setAttempts] = useState<AdapterAttemptSummary[] | null>(null);
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
      const next = await listAdapterStaging();
      if (mountedRef.current && requestId === requestIdRef.current) {
        setAttempts(next);
      }
    } catch (err) {
      if (mountedRef.current && requestId === requestIdRef.current) {
        setAttempts(null);
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

  return (
    <section className="rounded-xl bg-surface p-4 shadow-panel" aria-live="polite">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div>
          <div className="text-label uppercase text-inkFaint mb-1">MLX</div>
          <h2 className="text-heading text-ink flex items-center gap-2">
            <Archive className="w-4 h-4 text-primary" />
            <span>{t('mlx.stagingInventory.title')}</span>
          </h2>
        </div>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={loading}
          className="px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-caption flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
          <span>{t('mlx.stagingInventory.refresh')}</span>
        </button>
      </div>

      {loading && attempts === null && !error && (
        <div className="flex items-center gap-2 py-4 text-body text-inkMuted">
          <Loader2 className="w-4 h-4 animate-spin text-primary" />
          <span>{t('mlx.stagingInventory.loading')}</span>
        </div>
      )}

      {error && (
        <div role="alert" className="mb-3 flex items-start gap-1.5 text-caption text-danger">
          <AlertTriangle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
          <span className="break-all">{error}</span>
        </div>
      )}

      {attempts && attempts.length === 0 && (
        <p className="py-4 text-body text-inkMuted">{t('mlx.stagingInventory.empty')}</p>
      )}

      {attempts && attempts.length > 0 && (
        <ul className="space-y-2">
          {attempts.map((attempt, index) => {
            const reason = reasonOf(attempt.status);
            const bad = attempt.status.kind === 'unknown' || attempt.status.kind === 'scan_failed';
            return (
              // scan_failed 행은 attempt_id가 빈 문자열이라 key로 쓸 수 없다(docs/02 §4.1).
              <li key={`${index}-${attempt.attempt_id}`} className="rounded-lg bg-surfaceRaised p-3 space-y-1 min-w-0">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className={`text-bodyStrong ${bad ? 'text-danger' : 'text-ink'}`}>
                    {t(`mlx.stagingInventory.status.${attempt.status.kind}`)}
                    {attempt.status.kind === 'unchanged' && ` · ${attempt.status.state}`}
                  </span>
                  <span className="text-caption text-inkMuted break-all">
                    {t('mlx.stagingInventory.adapterName')}: {attempt.adapter_name ?? t('mlx.stagingInventory.noName')}
                  </span>
                </div>
                <code className="block break-all text-caption text-inkMuted">
                  {t('mlx.stagingInventory.attemptId')}: {attempt.attempt_id || t('mlx.stagingInventory.noId')}
                </code>
                {reason !== null && (
                  <p className="flex items-start gap-1.5 break-all text-caption text-danger">
                    <AlertTriangle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
                    <span>
                      {t('mlx.stagingInventory.reason')}: {reason}
                    </span>
                  </p>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
};
