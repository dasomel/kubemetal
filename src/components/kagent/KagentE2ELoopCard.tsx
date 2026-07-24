import React from 'react';
import { RefreshCw } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';

/**
 * MLOps E2E 자율 피드백 루프의 **절차 설명 카드**(`make test-e2e` / `scripts/e2e/`와 대응).
 * 실행 결과를 보여주는 패널이 아니므로 성공/적중 같은 결과 주장을 담지 않는다.
 */
const STEPS = ['synth', 'finetune', 'eval', 'diagnose', 'patch'] as const;

export const KagentE2ELoopCard: React.FC = () => {
  const { t } = useTranslation();

  return (
    <div className="rounded-xl bg-surface p-5 shadow-panel space-y-4">
      <div className="flex items-center gap-2">
        <RefreshCw className="w-5 h-5 text-primary" />
        <h3 className="text-bodyStrong text-ink text-base">{t('kagent.e2eTitle')}</h3>
      </div>

      <p className="text-caption text-inkMuted">{t('kagent.e2eNote')}</p>

      <div className="grid grid-cols-1 md:grid-cols-5 gap-3 text-caption">
        {STEPS.map((step, index) => (
          <div
            key={step}
            className="p-4 rounded-xl bg-surfaceRaised border border-hairline/8 space-y-1.5"
          >
            <div className="text-label text-primary font-bold uppercase">
              {t('kagent.e2eStepLabel', { index: index + 1 })}
            </div>
            <div className="text-ink font-bold">{t(`kagent.e2e.${step}.title`)}</div>
            <div className="text-inkMuted leading-normal">{t(`kagent.e2e.${step}.desc`)}</div>
          </div>
        ))}
      </div>
    </div>
  );
};
