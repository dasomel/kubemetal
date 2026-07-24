import React from 'react';
import { AlertCircle, CheckCircle2 } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';
import type { AirgapAssetItem } from '../../types/ipc';

export const formatStorageSize = (mb: number): string => {
  if (!mb || mb <= 0) return '0 MB';
  if (mb >= 1024 * 1024) return `${(mb / (1024 * 1024)).toFixed(2)} TB`;
  if (mb >= 1024) return `${(mb / 1024).toFixed(2)} GB`;
  return `${mb.toFixed(2)} MB`;
};

interface Props {
  assets: AirgapAssetItem[];
}

export const AirgapAssetTable: React.FC<Props> = ({ assets }) => {
  const { t } = useTranslation();

  return (
    <div className="rounded-xl bg-surface p-5 shadow-panel space-y-3">
      <h3 className="text-bodyStrong text-ink text-base font-bold">{t('airgap.tableTitle')}</h3>

      <div className="overflow-x-auto border border-hairline/8 rounded-xl">
        <table className="w-full text-left text-caption border-collapse">
          <thead>
            <tr className="bg-surfaceRaised border-b border-hairline/8 text-inkFaint">
              <th className="py-3 px-4 font-semibold">{t('airgap.colCategory')}</th>
              <th className="py-3 px-4 font-semibold">{t('airgap.colResource')}</th>
              <th className="py-3 px-4 font-semibold">{t('airgap.colVersion')}</th>
              <th className="py-3 px-4 font-semibold">{t('airgap.colFile')}</th>
              <th className="py-3 px-4 font-semibold text-right">{t('airgap.colState')}</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-hairline/6 text-ink font-mono">
            {assets.length === 0 ? (
              <tr>
                <td colSpan={5} className="py-6 px-4 text-center text-inkFaint font-sans">
                  {t('airgap.tableEmpty')}
                </td>
              </tr>
            ) : (
              assets.map((asset) => (
                <tr key={asset.file_name} className="hover:bg-surfaceRaised/50 transition-colors">
                  <td className="py-3 px-4 font-sans font-medium text-inkMuted">{asset.category}</td>
                  <td className="py-3 px-4 font-sans font-bold text-ink">{asset.name}</td>
                  <td className="py-3 px-4">
                    <span className="px-2 py-0.5 rounded bg-primary/10 text-primary font-bold text-label">
                      {asset.version}
                    </span>
                  </td>
                  <td className="py-3 px-4 text-inkFaint">{asset.file_name}</td>
                  <td className="py-3 px-4 text-right font-sans">
                    {asset.exists ? (
                      <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-success/10 text-success font-bold text-caption">
                        <CheckCircle2 className="w-3.5 h-3.5" />
                        {t('airgap.stateHave', { size: formatStorageSize(asset.size_mb) })}
                      </span>
                    ) : (
                      <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-warning/10 text-warning font-bold text-caption">
                        <AlertCircle className="w-3.5 h-3.5" />
                        {t('airgap.stateMissing')}
                      </span>
                    )}
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
};
