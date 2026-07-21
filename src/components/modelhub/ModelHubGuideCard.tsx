import React from 'react';
import { Gauge } from 'lucide-react';
import { useMetrics } from '../../hooks/useMetrics';

const RAM_PROFILES = [
  { range: '16GB', sizeLabel: '1~4B급' },
  { range: '32~48GB', sizeLabel: '7~14B급' },
  { range: '64GB+', sizeLabel: '32B급 이상' },
] as const;

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

export const ModelHubGuideCard: React.FC = () => {
  // 가이드 문구는 자주 바뀔 필요가 없어 대시보드보다 느린 주기로만 폴링한다.
  const metrics = useMetrics(5000);

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

      <div className="rounded-lg bg-surfaceRaised p-4">
        <table className="w-full text-body">
          <thead>
            <tr className="text-left border-b border-hairline/8">
              <th className="pb-2 text-label uppercase text-inkFaint font-normal">통합 메모리</th>
              <th className="pb-2 text-label uppercase text-inkFaint font-normal">권장 모델 규모</th>
            </tr>
          </thead>
          <tbody>
            {RAM_PROFILES.map((profile) => (
              <tr key={profile.range}>
                <td className="pt-2 text-bodyStrong text-ink">{profile.range}</td>
                <td className="pt-2 text-inkMuted">{profile.sizeLabel}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
};
