import React from 'react';
import { Bot, Code, Network, ShieldCheck } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';

/**
 * kagent를 소비하는 4개 역할(진단·코딩·온톨로지·보안)의 **설계 참조 카드**.
 * 런타임 상태를 표시하는 패널이 아니므로 "연동 완료" 같은 상태 주장을 쓰지 않는다 —
 * 각 카드는 근거 문서만 가리키고, 실제 가동 여부는 진단 탭이 kubectl 실측으로 보여준다.
 */
const CARDS: { key: string; icon: React.ElementType; docRef: string }[] = [
  { key: 'diag', icon: Bot, docRef: 'docs/08-kagent-feasibility.md' },
  { key: 'coding', icon: Code, docRef: 'docs/13-agent-coding-review.md' },
  { key: 'ontology', icon: Network, docRef: 'docs/12-ontology-extended-usage.md' },
  { key: 'security', icon: ShieldCheck, docRef: 'docs/11-kagent-mlops-integration.md' },
];

export const KagentModelArchitectureCards: React.FC = () => {
  const { t } = useTranslation();

  return (
    <div className="space-y-3">
      <p className="text-caption text-inkMuted">{t('kagent.modelsNote')}</p>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {CARDS.map(({ key, icon: Icon, docRef }) => (
          <div
            key={key}
            className="rounded-xl bg-surface p-5 shadow-panel space-y-3 border-t-4 border-t-primary"
          >
            <div className="flex items-center gap-2">
              <div className="p-2 rounded-lg bg-primary/10 text-primary">
                <Icon className="w-5 h-5" />
              </div>
              <h3 className="text-bodyStrong text-ink text-base">{t(`kagent.model.${key}.title`)}</h3>
            </div>
            <p className="text-caption text-inkMuted leading-relaxed">
              {t(`kagent.model.${key}.desc`)}
            </p>
            <div className="pt-3 border-t border-hairline/8 flex items-center justify-between text-caption text-inkFaint">
              <span>{t(`kagent.model.${key}.engine`)}</span>
              <span className="font-mono text-label">{docRef}</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};
