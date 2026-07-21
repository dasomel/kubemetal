import React from 'react';
import { Lock } from 'lucide-react';

interface LockedPreviewProps {
  /** 잠금 해제를 위해 필요한 다음 단계 안내 문구 */
  caption: string;
  children: React.ReactNode;
}

/**
 * 여정의 다음 단계에 해당하는 카드를 "잠긴 프리뷰"로 축소 표시한다.
 * 카드 자체는 그대로 렌더링하되(로직 변경 없음), 인터랙션을 막고
 * 낮은 불투명도 + 다음 단계 캡션 오버레이로 미리보기 느낌을 준다.
 */
export const LockedPreview: React.FC<LockedPreviewProps> = ({ caption, children }) => {
  return (
    <div className="relative animate-card-in">
      <div className="pointer-events-none select-none opacity-45 grayscale-[15%] transition-opacity duration-300">
        {children}
      </div>
      <div className="absolute inset-0 flex items-center justify-center p-4">
        <div className="flex items-center gap-2 px-4 py-2 rounded-lg bg-surface shadow-panel border border-hairline/8 text-caption text-inkMuted max-w-[85%] text-center">
          <Lock className="w-3.5 h-3.5 text-inkFaint shrink-0" />
          <span>{caption}</span>
        </div>
      </div>
    </div>
  );
};
