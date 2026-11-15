import React from 'react';
import { Copy } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';

export const HealthCorrelation: React.FC<{
  snapshot?: { correlation_id: string; observed_at: string } | null;
  status?: 'linked' | 'not_linked' | 'mismatch' | 'invalid_link';
}> = ({ snapshot, status }) => {
  const { t } = useTranslation();
  const [error, setError] = React.useState<string | null>(null);
  if (status === 'invalid_link') {
    return <p className="text-caption text-danger">{t('health.correlation.invalidLink')}</p>;
  }
  if (status === 'mismatch' && !snapshot) {
    return <p className="text-caption text-danger">{t('health.correlation.mismatch')}</p>;
  }
  if (!snapshot?.correlation_id || status === 'not_linked') {
    return <p className="text-caption text-inkFaint">{t('health.correlation.notLinked')}</p>;
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
