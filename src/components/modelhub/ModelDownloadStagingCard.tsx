import React from 'react';
import type { ModelDownloadStaging } from '../../types/ipc';
import { useTranslation } from '../../i18n/i18nContext';

interface Props {
  inventory: ModelDownloadStaging | null;
  error: string | null;
  onRefresh: () => void;
}

export const ModelDownloadStagingCard: React.FC<Props> = ({ inventory, error, onRefresh }) => {
  const { t } = useTranslation();
  const partial = inventory && (inventory.errors.length > 0 || inventory.entries.some(entry => entry.errors.length > 0));
  return (
    <section className="rounded-xl bg-surface p-4 shadow-panel space-y-3">
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-heading text-ink">{t('modelhub.staging.title')}</h2>
        <button type="button" onClick={onRefresh} className="text-caption text-primary">{t('modelhub.staging.refresh')}</button>
      </div>
      <p className="text-caption text-inkMuted">{t('modelhub.staging.hint')}</p>
      {error && <p role="alert" className="text-caption text-danger">{t('modelhub.staging.failed')}: {error}</p>}
      {!inventory && !error && <p className="text-caption text-inkMuted">{t('modelhub.staging.loading')}</p>}
      {inventory && <>
        <p className="text-caption text-inkFaint">{t('modelhub.staging.observed', { time: new Date(inventory.observed_at_unix_ms).toISOString() })}</p>
        {partial && <p role="alert" className="text-caption text-warning">{t('modelhub.staging.partial')}</p>}
        {inventory.truncated && <p role="alert" className="text-caption text-warning">{t('modelhub.staging.truncated')}</p>}
        {inventory.errors.map((detail, index) => <p key={index} className="text-caption text-danger break-all">{detail}</p>)}
        {inventory.state === 'not_created' && <p className="text-caption text-inkMuted">{t('modelhub.staging.notCreated')}</p>}
        {inventory.state === 'unknown' && <p role="alert" className="text-caption text-warning">{t('modelhub.staging.rootUnknown')}</p>}
        {inventory.state === 'observed' && inventory.entries.length === 0 && !partial && !inventory.truncated && <p className="text-caption text-inkMuted">{t('modelhub.staging.empty')}</p>}
        {inventory.entries.map(entry => (
          <div key={entry.id} className="rounded-lg bg-surfaceRaised p-3 space-y-1">
            <p className="text-bodyStrong text-ink break-all">{entry.id}</p>
            <p className="text-caption text-inkMuted">{t(`modelhub.staging.${entry.state}`)} · {entry.size_bytes === null ? t('modelhub.staging.sizeUnknown') : t('modelhub.staging.bytes', { size: entry.size_bytes })}</p>
            <p className="text-caption text-inkFaint">{t(`modelhub.staging.${entry.reason}`)}</p>
            {entry.errors.map((detail, index) => <p key={index} className="text-caption text-danger break-all">{detail}</p>)}
          </div>
        ))}
      </>}
    </section>
  );
};
