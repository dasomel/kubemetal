import React from 'react';
import { CheckCircle2 } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';

interface StatusItem {
  label: string;
  ok: boolean;
}

interface StatusSummaryStripProps {
  cluster: boolean;
  stack: boolean;
  integration: boolean;
  forwarding: boolean;
}

/**
 * 스택이 Ready + 연동된 상태에서 상단에 고정되는 전체 상태 요약 스트립.
 * 클러스터/스택/연동/포워딩 4개 dot로 한눈에 현재 상태를 보여준다.
 */
export const StatusSummaryStrip: React.FC<StatusSummaryStripProps> = ({
  cluster,
  stack,
  integration,
  forwarding,
}) => {
  const { t } = useTranslation();

  const items: StatusItem[] = [
    { label: t('strip.cluster'), ok: cluster },
    { label: t('strip.stack'), ok: stack },
    { label: t('strip.integration'), ok: integration },
    { label: t('strip.forwarding'), ok: forwarding },
  ];

  return (
    <div className="animate-card-in rounded-xl bg-surface p-4 shadow-panel flex flex-wrap items-center gap-x-6 gap-y-2">
      <div className="flex items-center gap-1.5 text-caption text-inkMuted shrink-0">
        <CheckCircle2 className="w-3.5 h-3.5 text-success" />
        <span className="text-bodyStrong text-ink">{t('strip.allReady')}</span>
      </div>
      <div className="flex flex-wrap items-center gap-x-5 gap-y-1.5">
        {items.map((item) => (
          <div key={item.label} className="flex items-center gap-1.5 text-caption text-inkMuted">
            <span className={`w-2 h-2 rounded-full shrink-0 ${item.ok ? 'bg-success' : 'bg-inkFaint'}`} />
            <span>{item.label}</span>
          </div>
        ))}
      </div>
    </div>
  );
};
