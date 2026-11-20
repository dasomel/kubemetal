import React from 'react';
import { Copy } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';
import type { SupportBundleVerificationReport } from '../../types/ipc';

type CorrelationSnapshot = { correlation_id: string; observed_at: string };

type HealthCorrelationProps = {
  snapshot?: CorrelationSnapshot | null;
  status?: SupportBundleVerificationReport['health_link_status'];
};

const NOTICES = {
  invalidLink: { key: 'health.correlation.invalidLink', tone: 'text-danger' },
  mismatch: { key: 'health.correlation.mismatch', tone: 'text-danger' },
  notLinked: { key: 'health.correlation.notLinked', tone: 'text-inkFaint' },
} as const;

// A notice replaces the identity row whenever there is no usable identity to show.
function resolveNotice({ snapshot, status }: HealthCorrelationProps): keyof typeof NOTICES | null {
  if (status === 'invalid_link') return 'invalidLink';
  if (status === 'mismatch' && !snapshot) return 'mismatch';
  if (!snapshot?.correlation_id || status === 'not_linked') return 'notLinked';
  return null;
}

export const HealthCorrelation: React.FC<HealthCorrelationProps> = ({ snapshot, status }) => {
  const { t } = useTranslation();
  const [error, setError] = React.useState<string | null>(null);
  const notice = resolveNotice({ snapshot, status });
  if (notice !== null || !snapshot) {
    const { key, tone } = NOTICES[notice ?? 'notLinked'];
    return <p className={`text-caption ${tone}`}>{t(key)}</p>;
  }
  return (
    <div className="text-caption text-inkMuted">
      <span>{t('health.correlation.label')}: </span>
      <button
        type="button"
        title={snapshot.correlation_id}
        aria-label={t('health.correlation.copy')}
        className="inline-flex items-center gap-1 rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(snapshot.correlation_id);
            setError(null);
          } catch (err) {
            setError(String(err));
          }
        }}
      >
        {snapshot.correlation_id.slice(0, 12)} <Copy className="w-3 h-3" aria-hidden="true" />
      </button>
      <span className="ml-2">{snapshot.observed_at}</span>
      {status === 'mismatch' && <span className="text-danger ml-2">{t('health.correlation.mismatch')}</span>}
      {error && <p role="alert" className="text-danger">{t('health.correlation.copyFailed', { error })}</p>}
    </div>
  );
};
