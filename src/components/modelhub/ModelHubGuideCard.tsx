import React from 'react';
import { Gauge } from 'lucide-react';
import { useMetrics } from '../../hooks/useMetrics';
import { RAM_SIZE_PROFILES, matchRamProfile, type RamSizeProfile } from '../../lib/modelCategories';

function recommendationFor(totalGb: number): string {
  const rounded = Math.round(totalGb);
  if (totalGb >= 64) {
    return `이 Mac(${rounded}GB)은 32B급 4bit까지 무난합니다. mlx-community의 4bit 변환본을 권장합니다.`;
  }
  if (totalGb >= 32) {
    return `이 Mac(${rounded}GB)은 7~14B급 4bit 모델이 적합합니다. mlx-community의 4bit 변환본을 권장합니다.`;
  }
  if (totalGb >= 16) {
    return `이 Mac(${rounded}GB)은 1~4B급 4bit 모델을 권장합니다. 더 큰 모델은 메모리 압박이 생길 수 있습니다.`;
  }
  return `이 Mac(${rounded}GB)은 메모리가 제한적입니다. 1~3B급 초경량 4bit 모델을 권장합니다.`;
}

interface ModelHubGuideCardProps {
  activeSelectionId: string | null;
  onSelectProfile: (profile: RamSizeProfile) => void;
}

export const ModelHubGuideCard: React.FC<ModelHubGuideCardProps> = ({
  activeSelectionId,
  onSelectProfile,
}) => {
  // 가이드 문구는 자주 바뀔 필요가 없어 대시보드보다 느린 주기로만 폴링한다.
  const metrics = useMetrics(5000);
  const matchedProfile = metrics ? matchRamProfile(metrics.total_memory_gb) : null;

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">Guide</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Gauge className="w-4 h-4 text-primary" />
          <span>메모리 기반 모델 가이드</span>
        </h2>
      </div>

      {metrics && (
        <p className="text-body text-inkMuted mb-4">{recommendationFor(metrics.total_memory_gb)}</p>
      )}

      <div className="rounded-lg bg-surfaceRaised overflow-hidden">
        <div className="grid grid-cols-2 px-3 pt-3 pb-2 text-label uppercase text-inkFaint">
          <span>통합 메모리</span>
          <span>권장 모델 규모 · 클릭해 검색</span>
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
                    <span className="text-caption text-primary font-normal">(이 Mac)</span>
                  )}
                </span>
                <span className="text-body text-inkMuted">{profile.sizeLabel}</span>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
};
