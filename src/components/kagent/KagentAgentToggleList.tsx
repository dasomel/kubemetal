import React from 'react';
import { Sliders } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';

interface Props {
  /** 백엔드가 실제로 설치/삭제할 수 있는 에이전트 목록(`available_agents`). */
  toggleable: string[];
  /** kubectl 실측으로 Ready인 에이전트 이름 집합(`active_agents`). */
  active: Set<string>;
  targetContext: string;
  onToggle: (agentName: string) => void;
}

export const KagentAgentToggleList: React.FC<Props> = ({
  toggleable,
  active,
  targetContext,
  onToggle,
}) => {
  const { t } = useTranslation();

  return (
    <div className="rounded-xl bg-surface p-5 shadow-panel space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Sliders className="w-5 h-5 text-primary" />
          <h3 className="text-bodyStrong text-ink text-base">{t('kagent.agentsTitle')}</h3>
        </div>
        <span className="text-caption text-inkFaint">
          {t('kagent.agentsTarget', { context: targetContext })}
        </span>
      </div>

      <p className="text-caption text-inkMuted">{t('kagent.agentsDesc')}</p>
      <p className="text-caption text-inkFaint">{t('kagent.agentsHelmNote')}</p>

      {toggleable.length === 0 ? (
        <p className="text-caption text-inkFaint">{t('kagent.agentsEmpty')}</p>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-3 pt-2">
          {toggleable.map((agent) => {
            const isActive = active.has(agent);
            return (
              <div
                key={agent}
                className={`p-4 rounded-xl border transition-all flex items-center justify-between ${
                  isActive ? 'bg-primary/5 border-primary/30' : 'bg-surfaceRaised border-hairline/8'
                }`}
              >
                <div className="flex items-center gap-2.5 min-w-0">
                  <span
                    title={isActive ? t('kagent.agentReady') : t('kagent.agentNotReady')}
                    className={`w-3 h-3 rounded-full shrink-0 ${isActive ? 'bg-success' : 'bg-inkFaint'}`}
                  />
                  <span className="text-caption text-ink font-bold truncate">{agent}</span>
                </div>
                <button
                  type="button"
                  onClick={() => onToggle(agent)}
                  className={`px-3 py-1 rounded-lg text-caption font-bold transition-all shrink-0 ${
                    isActive
                      ? 'bg-primaryStrong text-inverse'
                      : 'bg-surface border border-hairline/8 text-inkMuted'
                  }`}
                >
                  {isActive ? t('kagent.agentUninstall') : t('kagent.agentInstall')}
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};
