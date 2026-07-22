import React from 'react';
import { DataIngestionDagCard } from '../pipeline/DataIngestionDagCard';
import { RagCard } from '../pipeline/RagCard';
import { DvcCard } from '../pipeline/DvcCard';
import { useTranslation } from '../../i18n/i18nContext';
import { Database } from 'lucide-react';

export const DataView: React.FC = () => {
  const { t } = useTranslation();

  return (
    <div className="space-y-4">
      <div className="rounded-xl bg-surface p-4 shadow-panel">
        <div className="flex items-center gap-2 mb-0.5">
          <Database className="w-4 h-4 text-primary" />
          <h2 className="text-heading text-ink">{t('data.title')}</h2>
        </div>
        <p className="text-caption text-inkMuted">
          {t('data.subtitle')}
        </p>
      </div>

      <DataIngestionDagCard />

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <RagCard />
        <DvcCard />
      </div>
    </div>
  );
};
