import React, { useEffect } from 'react';
import { AlertTriangle, CheckCircle2, Info, Link2, RefreshCw } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';
import { useKagentModel } from '../../hooks/useKagentModel';

/**
 * D32: 저장된 DeployTarget(D26)의 kagent LLM 백엔드가 현재 서빙 중인 MLX 모델을 가리키는지
 * 보여주고, "현재 서빙 모델로 연결"로 갱신한다. 이 카드의 대상은 KagentOpsView 상단의 로컬
 * kubeconfig 선택기와 무관하다 — 화면에 대상을 별도로 명시한다.
 */
export const KagentModelStatusCard: React.FC = () => {
  const { t } = useTranslation();
  const { status, loading, error, busy, fetchStatus, connect } = useKagentModel();

  useEffect(() => {
    fetchStatus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const canConnect = !!status?.gate_ok && !!status?.serving && !busy;

  return (
    <div className="rounded-xl bg-surface p-5 shadow-panel space-y-4 border-t-4 border-t-primary">
      <div className="flex items-center justify-between gap-3 flex-wrap">
        <div className="flex items-center gap-2">
          <Link2 className="w-5 h-5 text-primary" />
          <h3 className="text-bodyStrong text-ink text-base">{t('kagent.modelStatus.title')}</h3>
        </div>
        <button
          type="button"
          onClick={() => fetchStatus()}
          disabled={loading}
          className="px-3 py-1.5 rounded-lg bg-surfaceRaised hover:brightness-95 text-primary text-caption font-medium flex items-center gap-1.5"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
          <span>{loading ? t('kagent.refreshing') : t('kagent.refresh')}</span>
        </button>
      </div>

      {error && (
        <div className="p-4 rounded-xl bg-danger/10 border border-danger/20 text-danger text-caption space-y-1">
          <div className="font-bold">{t('kagent.modelStatus.errorTitle')}</div>
          <div className="font-mono break-words">{error}</div>
        </div>
      )}

      {!error && !status && !loading && (
        <p className="text-caption text-inkFaint">{t('kagent.noReport')}</p>
      )}

      {status && (
        <div className="space-y-3">
          <p className="text-caption text-inkMuted">
            {t('kagent.modelStatus.targetLabel', {
              context: status.target_context,
              namespace: status.target_namespace,
            })}
          </p>

          {!status.gate_ok && (
            <div className="p-3 rounded-xl bg-warning/10 border border-warning/20 text-caption text-ink space-y-1">
              <div className="font-bold flex items-center gap-1.5">
                <AlertTriangle className="w-3.5 h-3.5 text-warning" />
                {t('kagent.modelStatus.gateBlockedTitle')}
              </div>
              {status.gate_reason && (
                <div className="font-mono text-inkMuted break-words">{status.gate_reason}</div>
              )}
            </div>
          )}

          {status.gate_ok && !status.serving && (
            <div className="p-3 rounded-xl bg-surfaceRaised border border-hairline/8 text-caption text-inkMuted">
              {t('kagent.modelStatus.noServing')}
            </div>
          )}

          {/* bridge_state_unknown은 "잘못됨"이 아니라 "확인 못함"이라 경고 톤이 아닌
              중립 톤 배너로 구분한다 — 브리지 조회 실패를 문제로 오인시키지 않는다. */}
          {status.gate_ok && status.serving && status.stale_code === 'bridge_state_unknown' && (
            <div className="p-3 rounded-xl bg-surfaceRaised border border-hairline/8 text-caption text-inkMuted flex items-start gap-2">
              <Info className="w-3.5 h-3.5 text-inkFaint shrink-0 mt-0.5" />
              <span className="break-words">{t('kagent.modelStatus.stale.bridge_state_unknown')}</span>
            </div>
          )}

          {status.gate_ok &&
            status.serving &&
            status.stale_code &&
            status.stale_code !== 'bridge_state_unknown' && (
              <div className="p-3 rounded-xl bg-warning/10 border border-warning/20 text-caption text-ink flex items-start gap-2">
                <AlertTriangle className="w-3.5 h-3.5 text-warning shrink-0 mt-0.5" />
                <span className="break-words">
                  {t(`kagent.modelStatus.stale.${status.stale_code}`)}
                </span>
              </div>
            )}

          {status.gate_ok && status.serving && !status.stale_code && status.model_config && (
            <div className="p-3 rounded-xl bg-success/10 border border-success/20 text-caption text-ink flex items-start gap-2">
              <CheckCircle2 className="w-3.5 h-3.5 text-success shrink-0 mt-0.5" />
              <span>{t('kagent.modelStatus.matched')}</span>
            </div>
          )}

          {status.model_config && (
            <div className="p-3 rounded-xl bg-surfaceRaised border border-hairline/8 space-y-1.5 font-mono text-caption">
              <div className="flex items-start gap-2">
                <span className="text-inkFaint shrink-0">{t('kagent.modelStatus.baseUrlLabel')}</span>
                <span className="text-ink break-all">{status.model_config.base_url ?? '—'}</span>
              </div>
              <div className="flex items-start gap-2">
                <span className="text-inkFaint shrink-0">{t('kagent.modelStatus.modelLabel')}</span>
                <span className="text-ink break-all">{status.model_config.model ?? '—'}</span>
              </div>
            </div>
          )}

          <button
            type="button"
            onClick={connect}
            disabled={!canConnect}
            className="px-3 py-1.5 rounded-lg bg-primaryStrong hover:brightness-110 text-inverse text-caption font-bold flex items-center gap-1.5 transition-all shadow-xs disabled:opacity-50"
          >
            <Link2 className={`w-3.5 h-3.5 ${busy ? 'animate-pulse' : ''}`} />
            <span>{busy ? t('kagent.modelStatus.connecting') : t('kagent.modelStatus.connectButton')}</span>
          </button>
        </div>
      )}
    </div>
  );
};
