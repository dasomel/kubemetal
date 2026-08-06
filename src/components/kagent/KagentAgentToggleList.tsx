import React from 'react';
import { AlertTriangle, Sliders } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';

interface Props {
  /** 백엔드가 실제로 설치/삭제할 수 있는 에이전트 목록(`available_agents`). */
  toggleable: string[];
  /** kubectl 실측으로 Ready인 에이전트 이름 집합(`active_agents`). */
  active: Set<string>;
  /**
   * 선택한 컨텍스트에 kagent이 설치돼 있는지. `null`이면 아직 진단 결과가 없어 알 수 없다.
   * `available_agents`는 설치 여부와 무관하게 항상 채워지므로, 이 값을 따로 받지 않으면
   * kagent이 없는 클러스터에서도 버튼이 멀쩡히 활성으로 보인다 — Agent CRD가 없어
   * apply가 반드시 실패하는데도.
   */
  installed: boolean | null;
  targetContext: string;
  onToggle: (agentName: string) => void;
}

export const KagentAgentToggleList: React.FC<Props> = ({
  toggleable,
  active,
  installed,
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

      {/* 미설치 판정이 먼저다 — 이 경우에도 toggleable은 3종이 채워져 오므로
          순서를 바꾸면 실패가 확정된 버튼을 그대로 보여주게 된다. */}
      {installed === false ? (
        <div className="p-4 rounded-xl bg-warning/10 border border-warning/20 flex items-start gap-2">
          <AlertTriangle className="w-4 h-4 text-warning shrink-0 mt-0.5" />
          <p className="text-caption text-inkMuted">{t('kagent.agentsNotInstalled')}</p>
        </div>
      ) : toggleable.length === 0 ? (
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
