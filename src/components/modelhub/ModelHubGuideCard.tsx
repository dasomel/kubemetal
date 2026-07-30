import React from 'react';
import { Gauge } from 'lucide-react';
import { useMetrics } from '../../hooks/useMetrics';
import { RAM_SIZE_PROFILES, matchRamProfile, type RamSizeProfile } from '../../lib/modelCategories';
import { useTranslation } from '../../i18n/i18nContext';

function recommendationFor(totalGb: number, t: (key: string, params?: Record<string, string | number>) => string): string {
  const rounded = Math.round(totalGb);
  if (totalGb >= 64) {
    return t('modelhub.rec.large', { gb: rounded });
  }
  if (totalGb >= 32) {
    return t('modelhub.rec.mid', { gb: rounded });
  }
  if (totalGb >= 16) {
    return t('modelhub.rec.small', { gb: rounded });
  }
  return t('modelhub.rec.tiny', { gb: rounded });
}

interface ModelHubGuideCardProps {
  activeSelectionId: string | null;
  onSelectProfile: (profile: RamSizeProfile) => void;
}

export const ModelHubGuideCard: React.FC<ModelHubGuideCardProps> = ({
  activeSelectionId,
  onSelectProfile,
}) => {
  const metrics = useMetrics(5000);
  const matchedProfile = metrics ? matchRamProfile(metrics.total_memory_gb) : null;
  const { t } = useTranslation();

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">Guide</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Gauge className="w-4 h-4 text-primary" />
          <span>{t('modelhub.guideTitle')}</span>
        </h2>
      </div>

      {metrics && (
        <p className="text-body text-inkMuted mb-4">{recommendationFor(metrics.total_memory_gb, t)}</p>
      )}

      <div className="rounded-lg bg-surfaceRaised overflow-hidden">
        <div className="grid grid-cols-2 px-3 pt-3 pb-2 text-label uppercase text-inkFaint">
          <span>{t('modelhub.tableUnifiedMemory')}</span>
          <span>{t('modelhub.tableRecommendedSpec')}</span>
        </div>
        <div className="divide-y divide-hairline/8">
          {RAM_SIZE_PROFILES.map((profile) => {
            const isMatch = matchedProfile?.id === profile.id;
            const isActive = activeSelectionId === profile.id;
            return (
              <button
                key={profile.id}
                type="button"
                onClick={() => onSelectProfile(profile)}
                className={`w-full grid grid-cols-2 items-center px-3 py-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary ${
                  isActive ? 'bg-surface' : 'hover:bg-surface/60'
                }`}
              >
                <span className={`text-bodyStrong flex items-center gap-1.5 ${isActive ? 'text-primary' : 'text-ink'}`}>
                  {profile.range}
                  {isMatch && (
                    <span className="text-caption text-primary font-normal">{t('modelhub.thisMacTag')}</span>
                  )}
                </span>
                <span className="text-body text-inkMuted">{t(profile.sizeLabelKey)}</span>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
};
