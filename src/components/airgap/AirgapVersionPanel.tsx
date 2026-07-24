import React from 'react';
import { AlertCircle, CheckCircle2, Download, Sparkles } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';
import type { AirgapLatestVersionReport } from '../../types/ipc';

interface Props {
  versions: AirgapLatestVersionReport[];
  downloading: boolean;
  onDownload: () => void;
}

export const AirgapVersionPanel: React.FC<Props> = ({ versions, downloading, onDownload }) => {
  const { t } = useTranslation();
  const hasUpdate = versions.some((v) => v.has_update);

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel space-y-3 border border-hairline/10">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 border-b border-hairline/6 pb-2">
        <div className="flex items-center gap-2 text-bodyStrong text-ink font-bold">
          <Sparkles className="w-4 h-4 text-warning" />
          <span>{t('airgap.versionPanelTitle')}</span>
        </div>
        {hasUpdate ? (
          <button
            type="button"
            onClick={onDownload}
            disabled={downloading}
            className="px-3.5 py-1.5 rounded-lg bg-warning/15 hover:bg-warning/25 text-warning text-caption font-bold flex items-center gap-1.5 transition-all self-start sm:self-auto"
          >
            <Download className="w-3.5 h-3.5" />
            <span>{t('airgap.fetchLatest')}</span>
          </button>
        ) : (
          <span className="text-caption text-success font-bold flex items-center gap-1">
            <CheckCircle2 className="w-3.5 h-3.5" />
            {t('airgap.allUpToDate')}
          </span>
        )}
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
        {versions.map((v) => (
          <div key={v.name} className="p-3 rounded-xl bg-surfaceRaised border border-hairline/8 space-y-1">
            <div className="text-caption text-ink font-bold">{v.name}</div>
            <div className="flex items-center justify-between text-caption gap-2">
              <span className="text-inkFaint">{t('airgap.have', { version: v.current_version })}</span>
              <span className="text-primary font-bold">
                {t('airgap.latest', { version: v.latest_version })}
              </span>
            </div>
            <div className="text-label pt-1 border-t border-hairline/6">
              {v.has_update ? (
                <span className="text-warning font-bold flex items-center gap-1">
                  <AlertCircle className="w-3 h-3" />
                  {t('airgap.updateAvailable')}
                </span>
              ) : (
                <span className="text-success font-bold flex items-center gap-1">
                  <CheckCircle2 className="w-3 h-3" />
                  {t('airgap.upToDate')}
                </span>
              )}
            </div>
          </div>
        ))}
      </div>

      <div className="p-3 rounded-xl bg-surfaceRaised/60 border border-hairline/6 text-caption text-inkMuted space-y-1">
        <div className="font-bold text-ink">{t('airgap.updateGuideTitle')}</div>
        <ol className="list-decimal list-inside space-y-0.5 text-inkFaint">
          <li>{t('airgap.updateGuideStep1')}</li>
          <li>{t('airgap.updateGuideStep2')}</li>
        </ol>
      </div>
    </div>
  );
};
