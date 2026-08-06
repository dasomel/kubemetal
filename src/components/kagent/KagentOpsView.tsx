import React, { useEffect, useState } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import {
  AlertTriangle,
  ArrowUpRight,
  Bot,
  CheckCircle2,
  DownloadCloud,
  Layers,
  RefreshCw,
  Sliders,
  Terminal,
} from 'lucide-react';
import { useColima } from '../../hooks/useColima';
import { useHostPorts } from '../../hooks/useHostPorts';
import { useTranslation } from '../../i18n/i18nContext';
import type { KagentDiagnosticReport } from '../../types/ipc';
import { publishKagentDiagnostics } from '../../state/kagentDiagnosticsStore';
import { KagentAgentToggleList } from './KagentAgentToggleList';
import { KagentE2ELoopCard } from './KagentE2ELoopCard';
import { KagentModelArchitectureCards } from './KagentModelArchitectureCards';
import { KagentModelStatusCard } from './KagentModelStatusCard';

type SubTab = 'diagnostics' | 'models' | 'agents' | 'e2e';

const SUB_TABS: { id: SubTab; labelKey: string; icon: React.ElementType }[] = [
  { id: 'diagnostics', labelKey: 'kagent.subtabDiagnostics', icon: Terminal },
  { id: 'models', labelKey: 'kagent.subtabModels', icon: Bot },
  { id: 'agents', labelKey: 'kagent.subtabAgents', icon: Sliders },
  { id: 'e2e', labelKey: 'kagent.subtabE2e', icon: RefreshCw },
];

export const KagentOpsView: React.FC = () => {
  const { status: cluster } = useColima();
  // kagent UI 포워딩 포트는 백엔드 배정을 따른다(D1 기본 8090, 점유 시 대체 포트).
  const { urlFor } = useHostPorts();
  const kagentUiUrl = urlFor('kagent-ui');
  const { t } = useTranslation();
  const [subTab, setSubTab] = useState<SubTab>('diagnostics');
  const [contexts, setContexts] = useState<string[]>([]);
  const [selectedContext, setSelectedContext] = useState<string>('colima');
  const [report, setReport] = useState<KagentDiagnosticReport | null>(null);
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [fetchedAt, setFetchedAt] = useState<string | null>(null);
  const [actionMsg, setActionMsg] = useState<string | null>(null);
  const [installBusy, setInstallBusy] = useState<boolean>(false);

  const activeAgents = new Set(report?.active_agents ?? []);
  const toggleableAgents = report?.available_agents ?? [];

  const fetchContexts = async () => {
    try {
      setContexts(await invoke<string[]>('list_kubeconfig_contexts'));
    } catch (e) {
      setError(String(e));
    }
  };

  // 조회 실패는 그대로 노출한다 — "정상"으로 폴백하면 장애를 정상으로 위장하게 된다.
  const fetchDiagnostics = async (ctx: string) => {
    setLoading(true);
    try {
      const res = await invoke<KagentDiagnosticReport>('get_kagent_diagnostics', { context: ctx });
      setReport(res);
      setFetchedAt(new Date().toLocaleTimeString());
      setError(null);
      // 탭을 열지 않아도 사이드바 배지가 최신 상태를 알 수 있도록 브로드캐스트한다(Feature C).
      publishKagentDiagnostics(res, new Date().toISOString());
    } catch (e) {
      setReport(null);
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  // 설치 결과는 백엔드가 반환한 helm 출력을 인용한다 — "설치 완료"를 단정하지 않는다(D22).
  // 다이얼로그는 NOTES: 이전(STATUS/REVISION/NAMESPACE 포함) 요약까지만 보여준다 — 백엔드
  // 반환 문자열 자체는 자르지 않고, 잘랐을 때만 안내 문구를 덧붙인다(인용 원칙은 유지).
  const summarizeInstallOutput = (raw: string): string => {
    const notesIdx = raw.indexOf('NOTES:');
    if (notesIdx === -1) return raw;
    return `${raw.slice(0, notesIdx).trimEnd()}\n\n${t('kagent.install.truncated')}`;
  };

  const handleInstallKagent = async () => {
    setInstallBusy(true);
    try {
      // 설치 대상은 이 패널의 kubeconfig 선택기와 같은 축이다(D33 개정) — 진단이 narwhal을
      // 보는데 설치만 저장된 DeployTarget(colima)으로 가면 재조회는 영원히 "미설치"가 된다.
      const res = await invoke<string>('install_kagent', { context: selectedContext });
      const shown = res ? summarizeInstallOutput(res) : t('kagent.install.fallbackAck');
      await message(shown, { title: 'KubeMetal', kind: 'info' });
    } catch (e) {
      await message(t('kagent.install.failed', { error: String(e) }), { title: 'KubeMetal', kind: 'error' });
    } finally {
      setInstallBusy(false);
      fetchDiagnostics(selectedContext);
    }
  };

  useEffect(() => {
    fetchContexts();
    fetchDiagnostics(selectedContext);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedContext]);

  const handleToggleAgent = async (agentName: string) => {
    const enable = !activeAgents.has(agentName);

    setActionMsg(t('kagent.actionRequesting', { agent: agentName }));
    try {
      setActionMsg(
        await invoke<string>('toggle_kagent_agent', {
          agentName,
          enable,
          context: selectedContext,
        }),
      );
      // 파드 기동에는 시간이 걸리므로 즉시/3초/6초 후 실제 상태를 다시 읽는다.
      fetchDiagnostics(selectedContext);
      setTimeout(() => fetchDiagnostics(selectedContext), 3000);
      setTimeout(() => fetchDiagnostics(selectedContext), 6000);
    } catch (e) {
      setActionMsg(t('kagent.actionFailed', { agent: agentName, error: String(e) }));
    }
  };

  const contextOptions = contexts.length > 0 ? contexts : [selectedContext];

  return (
    <div className="space-y-4">
      {/* 상단 컨트롤러 패널 & Kubeconfig 선택 */}
      <div className="rounded-xl bg-surface p-4 shadow-panel flex flex-col md:flex-row md:items-center justify-between gap-3">
        <div className="flex items-center gap-3 min-w-0">
          <div className="p-2 rounded-xl bg-primary/10 text-primary shrink-0">
            <Bot className="w-6 h-6" />
          </div>
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <h2 className="text-heading text-ink">{t('kagent.title')}</h2>
              <span className="px-2 py-0.5 rounded-full bg-primary/10 text-primary text-label font-bold uppercase shrink-0">
                CNCF Sandbox
              </span>
            </div>
            <p className="text-caption text-inkMuted truncate">{t('kagent.subtitle')}</p>
          </div>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-surfaceRaised border border-hairline/8">
            <Layers className="w-3.5 h-3.5 text-primary" />
            <span className="text-caption text-inkFaint font-medium">
              {t('kagent.kubeconfigLabel')}
            </span>
            <select
              value={selectedContext}
              onChange={(e) => setSelectedContext(e.target.value)}
              className="bg-transparent text-caption text-ink font-bold border-none focus:outline-none cursor-pointer"
            >
              {contextOptions.map((ctx) => (
                <option key={ctx} value={ctx} className="bg-surface text-ink">
                  {ctx}
                </option>
              ))}
            </select>
          </div>

          {cluster?.is_running && kagentUiUrl && (
            <button
              type="button"
              onClick={() => openUrl(kagentUiUrl).catch((e) => setActionMsg(String(e)))}
              className="px-3 py-1.5 rounded-lg bg-primaryStrong hover:brightness-110 text-inverse text-caption font-bold flex items-center gap-1.5 transition-all shadow-xs"
            >
              {t('kagent.openUI')}
              <ArrowUpRight className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      </div>

      {/* 서브 탭 내비게이션 */}
      <div className="border-b border-hairline/8 flex items-center gap-2 overflow-x-auto pb-0.5">
        {SUB_TABS.map(({ id, labelKey, icon: Icon }) => (
          <button
            key={id}
            type="button"
            onClick={() => setSubTab(id)}
            className={`px-4 py-2.5 rounded-t-xl text-caption font-bold flex items-center gap-2 border-b-2 transition-all whitespace-nowrap ${
              subTab === id
                ? 'border-primary text-primary bg-primary/5'
                : 'border-transparent text-inkMuted hover:text-ink hover:bg-surfaceRaised/50'
            }`}
          >
            <Icon className="w-4 h-4" />
            <span>{t(labelKey)}</span>
          </button>
        ))}
      </div>

      {actionMsg && (
        <div className="px-3.5 py-2 rounded-xl bg-primary/10 border border-primary/20 text-primary text-caption break-words">
          {actionMsg}
        </div>
      )}

      {subTab === 'diagnostics' && (
        <div className="rounded-xl bg-surface p-5 shadow-panel space-y-4">
          <div className="flex items-center justify-between gap-3 flex-wrap">
            <div className="flex items-center gap-2">
              <Terminal className="w-5 h-5 text-primary" />
              <h3 className="text-bodyStrong text-ink text-base">
                {t('kagent.inspectorTitle', { context: selectedContext })}
              </h3>
              {report &&
                (report.pod_issues_count > 0 ? (
                  <span className="px-2.5 py-0.5 rounded-full bg-danger/10 text-danger text-caption font-bold flex items-center gap-1">
                    <AlertTriangle className="w-3.5 h-3.5" />
                    {t('kagent.issuesDetected', { count: report.pod_issues_count })}
                  </span>
                ) : (
                  <span className="px-2.5 py-0.5 rounded-full bg-success/10 text-success text-caption font-bold flex items-center gap-1">
                    <CheckCircle2 className="w-3.5 h-3.5" />
                    {t('kagent.clusterHealthy')}
                  </span>
                ))}
            </div>

            <button
              type="button"
              onClick={() => fetchDiagnostics(selectedContext)}
              disabled={loading}
              className="px-3 py-1.5 rounded-lg bg-surfaceRaised hover:brightness-95 text-primary text-caption font-medium flex items-center gap-1.5"
            >
              <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
              <span>{loading ? t('kagent.refreshing') : t('kagent.refresh')}</span>
            </button>
          </div>

          {error && (
            <div className="p-4 rounded-xl bg-danger/10 border border-danger/20 text-danger text-caption space-y-3">
              <div className="font-bold">{t('kagent.errorTitle')}</div>
              <div className="font-sans whitespace-pre-line leading-relaxed text-ink font-medium bg-surface/60 p-3 rounded-lg border border-hairline/10">{error}</div>
              {selectedContext !== 'colima' && (
                <button
                  type="button"
                  onClick={() => {
                    setSelectedContext('colima');
                    fetchDiagnostics('colima');
                  }}
                  className="px-3 py-1.5 rounded-lg bg-primaryStrong hover:brightness-110 text-inverse text-caption font-bold flex items-center gap-1.5 transition-all shadow-xs"
                >
                  <span>로컬 colima 컨텍스트로 전환하여 진단</span>
                </button>
              )}
            </div>
          )}

          {!error && !report && !loading && (
            <p className="text-caption text-inkFaint">{t('kagent.noReport')}</p>
          )}

          {report && !report.kagent_installed && (
            <div className="p-4 rounded-xl bg-warning/10 border border-warning/20 space-y-3">
              <div className="flex items-start gap-2">
                <DownloadCloud className="w-4 h-4 text-warning shrink-0 mt-0.5" />
                <div className="min-w-0 space-y-1">
                  <p className="text-caption font-bold text-ink">{t('kagent.install.notInstalledTitle')}</p>
                  <p className="text-caption text-inkMuted">{t('kagent.install.notInstalledDesc')}</p>
                </div>
              </div>
              <button
                type="button"
                onClick={handleInstallKagent}
                disabled={installBusy}
                className="px-3 py-1.5 rounded-lg bg-primaryStrong hover:brightness-110 text-inverse text-caption font-bold flex items-center gap-1.5 transition-all shadow-xs disabled:opacity-60"
              >
                <DownloadCloud className={`w-3.5 h-3.5 ${installBusy ? 'animate-pulse' : ''}`} />
                <span>{installBusy ? t('kagent.install.installing') : t('kagent.install.button')}</span>
              </button>
            </div>
          )}

          {report && (
            <div className="p-4 rounded-xl bg-surfaceRaised border border-hairline/8 space-y-3 font-mono text-caption">
              <div className="flex items-start gap-2">
                <span className="text-primary font-bold shrink-0">{t('kagent.diagnosisLabel')}</span>
                <span className="text-ink break-words">{report.recent_diagnosis}</span>
              </div>
              <div className="flex items-start gap-2 pt-2.5 border-t border-hairline/6">
                <span className="text-success font-bold shrink-0">{t('kagent.actionLabel')}</span>
                <span className="text-inkMuted break-words">{report.recommended_action}</span>
              </div>
              <div className="pt-2 border-t border-hairline/6 flex items-center justify-between gap-2 text-inkFaint font-sans text-label flex-wrap">
                <span>
                  {t('kagent.targetLabel', { context: report.target_context })} ·{' '}
                  {report.kagent_ready ? t('kagent.readyLabel') : t('kagent.notReadyLabel')}
                </span>
                {fetchedAt && <span>{t('kagent.fetchedAt', { time: fetchedAt })}</span>}
              </div>
            </div>
          )}
        </div>
      )}

      {subTab === 'models' && (
        <div className="space-y-4">
          <KagentModelStatusCard />
          <KagentModelArchitectureCards />
        </div>
      )}

      {subTab === 'agents' && (
        <KagentAgentToggleList
          toggleable={toggleableAgents}
          active={activeAgents}
          // 진단 결과가 없으면 설치 여부를 모른다 — 모르는 것을 아는 척하지 않는다(D22).
          installed={report ? report.kagent_installed : null}
          targetContext={selectedContext}
          onToggle={handleToggleAgent}
        />
      )}

      {subTab === 'e2e' && <KagentE2ELoopCard />}
    </div>
  );
};
