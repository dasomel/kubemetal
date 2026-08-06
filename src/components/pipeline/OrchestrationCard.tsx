import React, { useEffect, useState } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Workflow, Loader2, Play, Square, Cpu, ArrowUpRight, Rocket, FlaskConical } from 'lucide-react';
import { usePrefect } from '../../hooks/usePrefect';
import { useMlx } from '../../hooks/useMlx';
import { useTranslation } from '../../i18n/i18nContext';
import type { FlowRunInfo, FineTuneConfig } from '../../types/ipc';

const openEndpoint = (url: string) => {
  openUrl(url).catch(() => window.open(url, '_blank'));
};

type DotColor = 'success' | 'warning' | 'danger' | 'inkFaint';

const dotClass: Record<DotColor, string> = {
  success: 'bg-success',
  warning: 'bg-warning',
  danger: 'bg-danger',
  inkFaint: 'bg-inkFaint',
};

const runStateDot = (stateType: string): { color: DotColor; pulse?: boolean } => {
  switch (stateType) {
    case 'COMPLETED':
      return { color: 'success' };
    case 'RUNNING':
      return { color: 'warning', pulse: true };
    case 'FAILED':
    case 'CRASHED':
      return { color: 'danger' };
    default:
      return { color: 'inkFaint' };
  }
};

const FlowRunRow: React.FC<{ run: FlowRunInfo }> = ({ run }) => {
  const { color, pulse } = runStateDot(run.state_type);
  return (
    <div className="flex items-center justify-between py-1.5 px-3 rounded-lg bg-surfaceRaised">
      <span className="text-body text-ink truncate">{run.name}</span>
      <div className="flex items-center gap-1.5 text-caption text-inkMuted shrink-0 ml-2">
        <span className={`w-2 h-2 rounded-full shrink-0 ${dotClass[color]} ${pulse ? 'animate-pulse' : ''}`} />
        <span>{run.state_name}</span>
      </div>
    </div>
  );
};

const inputClass =
  'w-full px-3.5 py-2.5 rounded-md bg-surfaceRaised text-ink text-body placeholder:text-inkFaint border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary';
const labelClass = 'text-label uppercase text-inkFaint mb-1.5 block';

/** 파이프라인 탭 활성 중에만 마운트되므로 usePrefect(true)로 5초 폴링을 활성화한다. */
export const OrchestrationCard: React.FC = () => {
  const {
    status,
    installing,
    setupEnv,
    startingRunner,
    startRunner,
    stoppingRunner,
    stopRunner,
    triggeringFlow,
    triggerFinetuneFlow,
    evalInstalling,
    setupEvalEnv,
    triggeringEvaluate,
    triggerEvaluateFlow,
  } = usePrefect(true);
  const { localModels, mlxStatus } = useMlx();
  const { t } = useTranslation();
  const [showFlowForm, setShowFlowForm] = useState(false);
  const [modelPath, setModelPath] = useState('');
  const [showEvalForm, setShowEvalForm] = useState(false);
  const [evalTasks, setEvalTasks] = useState('gsm8k');
  const [evalLimit, setEvalLimit] = useState(8);
  const [evalPort, setEvalPort] = useState(8080);

  const serverReady = status?.server_ready ?? false;
  const servingPort = mlxStatus?.serving?.port;

  // 서빙 활성 포트가 바뀌면 평가 폼의 포트를 자동으로 맞춘다(수동 입력은 계속 허용).
  useEffect(() => {
    if (servingPort) setEvalPort(servingPort);
  }, [servingPort]);

  const handleTrigger = (e: React.FormEvent) => {
    e.preventDefault();
    if (!modelPath) return;
    const config: FineTuneConfig = {
      model_path: modelPath,
      data_path: '~/.kubemetal/datasets/smoke',
      iters: 100,
      batch_size: 1,
      learning_rate: 1e-5,
      adapter_name: 'flow-adapter',
    };
    triggerFinetuneFlow(config);
    setShowFlowForm(false);
    setModelPath('');
  };

  const handleEvaluate = (e: React.FormEvent) => {
    e.preventDefault();
    if (!servingPort) return;
    triggerEvaluateFlow(evalTasks || 'gsm8k', evalLimit, evalPort);
    setShowEvalForm(false);
  };

  return (
    <div className="animate-card-in rounded-xl bg-surface p-4 shadow-panel">
      <div className="flex items-center justify-between mb-4">
        <div>
          <div className="text-label uppercase text-inkFaint mb-1">Prefect</div>
          <h2 className="text-heading text-ink flex items-center gap-2">
            <Workflow className="w-4 h-4 text-primary" />
            <span>{t('orch.title')}</span>
          </h2>
        </div>
        {serverReady && (
          <button
            type="button"
            onClick={() => openEndpoint('http://127.0.0.1:4200')}
            className="px-2.5 py-1 rounded-md bg-surfaceRaised hover:brightness-95 text-primary text-caption flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            Prefect UI
            <ArrowUpRight className="w-3 h-3" />
          </button>
        )}
      </div>

      {/* ① 서버 상태 */}
      <div className="flex items-center gap-1.5 text-caption text-inkMuted mb-1.5">
        <span className={`w-2 h-2 rounded-full shrink-0 ${serverReady ? 'bg-success' : 'bg-inkFaint'}`} />
        <span>{t('orch.prefectServer')} {serverReady ? t('orch.ready') : t('orch.waiting')}</span>
      </div>

      {!serverReady ? (
        <div className="text-caption text-inkFaint">
          {t('orch.serverNotice')}
        </div>
      ) : (
        <div className="space-y-3 mt-3">
          {/* ② 환경 상태 & ③ 플로우 러너 (한 줄 표시) */}
          <div className="p-3 rounded-lg bg-surfaceRaised flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-6">
              <div className="flex items-center gap-2">
                <span className="text-bodyStrong text-ink">{t('orch.envLabel')}</span>
                <span className={`w-2 h-2 rounded-full shrink-0 ${status?.env_installed ? 'bg-success' : 'bg-inkFaint'}`} />
                <span className="text-caption text-inkMuted">{status?.env_installed ? t('orch.installed') : t('orch.notInstalled')}</span>
                {!status?.env_installed && (
                  <button
                    type="button"
                    onClick={() => setupEnv()}
                    disabled={installing}
                    className="ml-2 py-1 px-2.5 bg-primaryStrong hover:brightness-110 disabled:opacity-50 text-inverse text-caption rounded-md transition-all flex items-center gap-1 shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                  >
                    {installing ? <Loader2 className="w-3 h-3 animate-spin" /> : <Cpu className="w-3 h-3" />}
                    <span>{installing ? t('orch.installingEnv') : t('orch.installEnv')}</span>
                  </button>
                )}
              </div>

              <div className="flex items-center gap-2 border-l border-hairline/8 pl-6">
                <span className="text-bodyStrong text-ink">{t('orch.runnerLabel')}</span>
                <span className={`w-2 h-2 rounded-full shrink-0 ${status?.runner_running ? 'bg-success' : 'bg-inkFaint'}`} />
                <span className="text-caption text-inkMuted">{status?.runner_running ? t('orch.runnerRunning', { pid: status.runner_pid ?? 0 }) : t('orch.runnerStopped')}</span>
              </div>
            </div>

            {status?.runner_running ? (
              <button
                type="button"
                onClick={() => stopRunner()}
                disabled={stoppingRunner}
                className="py-1.5 px-3 bg-surface hover:brightness-95 disabled:opacity-50 text-ink text-caption font-medium rounded-md transition-all flex items-center gap-1.5 shrink-0 border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
              >
                {stoppingRunner ? <Loader2 className="w-3 h-3 animate-spin" /> : <Square className="w-3 h-3" />}
                <span>{t('orch.stop')}</span>
              </button>
            ) : (
              <button
                type="button"
                onClick={() => startRunner()}
                disabled={startingRunner || !status?.env_installed}
                className="py-1.5 px-3 bg-primaryStrong hover:brightness-110 disabled:opacity-50 text-inverse text-caption font-medium rounded-md transition-all flex items-center gap-1.5 shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
              >
                {startingRunner ? <Loader2 className="w-3 h-3 animate-spin" /> : <Play className="w-3 h-3" />}
                <span>{t('orch.start')}</span>
              </button>
            )}
          </div>

          {/* ④ 최근 flow run */}
          <div>
            <h3 className="text-label uppercase text-inkFaint mb-2">{t('orch.recentRuns')}</h3>
            {status && status.recent_runs.length > 0 ? (
              <div className="space-y-1.5">
                {status.recent_runs.slice(0, 5).map((run) => (
                  <FlowRunRow key={run.id} run={run} />
                ))}
              </div>
            ) : (
              <div className="py-4 text-center text-inkFaint text-caption">{t('orch.noRecentRuns')}</div>
            )}
          </div>

          {/* ⑤ 파인튜닝 플로우 실행 */}
          <div className="pt-3 border-t border-hairline/8">
            {!showFlowForm ? (
              <>
                <button
                  type="button"
                  onClick={() => setShowFlowForm(true)}
                  disabled={localModels.length === 0}
                  className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
                >
                  <Rocket className="w-4 h-4" />
                  <span>{t('orch.runFinetuneFlow')}</span>
                </button>
                {localModels.length === 0 && (
                  <div className="text-caption text-inkFaint mt-2">
                    {t('orch.noLocalModelsNotice')}
                  </div>
                )}
              </>
            ) : (
              <form onSubmit={handleTrigger} className="space-y-3">
                <div>
                  <label className={labelClass}>{t('orch.localModelLabel')}</label>
                  <select value={modelPath} onChange={(e) => setModelPath(e.target.value)} className={inputClass}>
                    <option value="">{t('orch.selectModelPlaceholder')}</option>
                    {localModels.map((m) => (
                      <option key={m.repo_id} value={m.path}>
                        {m.repo_id}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="flex gap-2">
                  <button
                    type="submit"
                    disabled={triggeringFlow || !modelPath}
                    className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
                  >
                    {triggeringFlow ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />}
                    <span>{t('orch.runDefaultBtn')}</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => setShowFlowForm(false)}
                    className="py-2.5 px-4 bg-surfaceRaised hover:brightness-95 text-inkMuted text-bodyStrong rounded-md transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                  >
                    {t('orch.cancelBtn')}
                  </button>
                </div>
              </form>
            )}
          </div>

          {/* ⑥ 평가 */}
          <div className="pt-3 border-t border-hairline/8">
            <h3 className="text-label uppercase text-inkFaint mb-2">{t('orch.evalHeader')}</h3>
            {!status?.eval_env_installed ? (
              <button
                type="button"
                onClick={() => setupEvalEnv()}
                disabled={evalInstalling}
                className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
              >
                {evalInstalling ? <Loader2 className="w-4 h-4 animate-spin" /> : <FlaskConical className="w-4 h-4" />}
                <span>{evalInstalling ? t('orch.installingEvalEnv') : t('orch.installEvalEnvBtn')}</span>
              </button>
            ) : !showEvalForm ? (
              <>
                <button
                  type="button"
                  onClick={() => setShowEvalForm(true)}
                  className="py-2.5 px-4 bg-surfaceRaised hover:brightness-95 text-ink text-bodyStrong rounded-md transition-all flex items-center gap-1.5 border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                >
                  <FlaskConical className="w-4 h-4" />
                  <span>{t('orch.runEvalBtn')}</span>
                </button>
                {!servingPort && (
                  <div className="text-caption text-inkFaint mt-2">
                    {t('orch.noServingNotice')}
                  </div>
                )}
              </>
            ) : (
              <form onSubmit={handleEvaluate} className="space-y-3">
                <div>
                  <label className={labelClass}>{t('orch.taskLabel')}</label>
                  <input
                    type="text"
                    value={evalTasks}
                    onChange={(e) => setEvalTasks(e.target.value)}
                    placeholder="gsm8k"
                    className={inputClass}
                  />
                </div>
                <div>
                  <label className={labelClass}>{t('orch.sampleCountLabel')}</label>
                  <input
                    type="number"
                    min={1}
                    value={evalLimit}
                    onChange={(e) => setEvalLimit(Math.max(1, Number(e.target.value) || 1))}
                    className={inputClass}
                  />
                </div>
                <div>
                  <label className={labelClass}>{t('orch.servingPortLabel')}</label>
                  <input
                    type="number"
                    min={1}
                    value={evalPort}
                    onChange={(e) => setEvalPort(Number(e.target.value) || 0)}
                    className={inputClass}
                  />
                  {servingPort ? (
                    <div className="text-caption text-inkFaint mt-1">{t('orch.autoPortDetected', { port: servingPort })}</div>
                  ) : (
                    <div className="text-caption text-danger mt-1">
                      {t('orch.servingNotRunningError')}
                    </div>
                  )}
                </div>
                <div className="flex gap-2">
                  <button
                    type="submit"
                    disabled={triggeringEvaluate || !servingPort}
                    className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
                  >
                    {triggeringEvaluate ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />}
                    <span>{t('orch.runEvalBtn')}</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => setShowEvalForm(false)}
                    className="py-2.5 px-4 bg-surfaceRaised hover:brightness-95 text-inkMuted text-bodyStrong rounded-md transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                  >
                    {t('orch.cancelBtn')}
                  </button>
                </div>
              </form>
            )}
          </div>
        </div>
      )}
    </div>
  );
};
