import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { PrefectStatus, FineTuneConfig, EvalMetric } from '../types/ipc';

/**
 * Prefect 오케스트레이션 상태 훅.
 * 폴링 정책: 파이프라인 탭이 활성(`active`)이고 러너 실행 중 또는 환경 설치 진행 중일 때만
 * 5초 간격 폴링, 그 외에는 마운트(탭 진입) 시 1회만 조회한다.
 */
export function usePrefect(active: boolean = false) {
  const [status, setStatus] = useState<PrefectStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [settingUpEnv, setSettingUpEnv] = useState(false);
  const [installingLocal, setInstallingLocal] = useState(false);
  const [startingRunner, setStartingRunner] = useState(false);
  const [stoppingRunner, setStoppingRunner] = useState(false);
  const [triggeringFlow, setTriggeringFlow] = useState(false);
  const [settingUpEvalEnv, setSettingUpEvalEnv] = useState(false);
  const [evalInstallingLocal, setEvalInstallingLocal] = useState(false);
  const [triggeringEvaluate, setTriggeringEvaluate] = useState(false);
  const [lastEvalRunId, setLastEvalRunId] = useState<string | null>(null);
  const [evalResults, setEvalResults] = useState<EvalMetric[]>([]);
  const [loadingEvalResults, setLoadingEvalResults] = useState(false);

  const fetchStatus = useCallback(async () => {
    try {
      const res = await invoke<PrefectStatus>('get_prefect_status');
      setStatus(res);
    } catch (err) {
      console.error('Prefect 상태 로드 오류:', err);
    }
  }, []);

  const setupEnv = useCallback(async () => {
    setSettingUpEnv(true);
    setInstallingLocal(true);
    try {
      const res = await invoke<string>('setup_prefect_env');
      await message(res || 'Prefect 환경 설치를 시작했습니다.', { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      setInstallingLocal(false);
      await message(`Prefect 환경 설치 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setSettingUpEnv(false);
    }
  }, [fetchStatus]);

  const startRunner = useCallback(async () => {
    setStartingRunner(true);
    try {
      const res = await invoke<string>('start_prefect_runner');
      await message(res || 'Prefect 러너를 시작했습니다.', { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(`Prefect 러너 시작 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setStartingRunner(false);
    }
  }, [fetchStatus]);

  const stopRunner = useCallback(async () => {
    setStoppingRunner(true);
    try {
      const res = await invoke<string>('stop_prefect_runner');
      await message(res || 'Prefect 러너를 정지했습니다.', { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(`Prefect 러너 정지 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setStoppingRunner(false);
    }
  }, [fetchStatus]);

  const triggerFinetuneFlow = useCallback(
    async (config: FineTuneConfig) => {
      setTriggeringFlow(true);
      try {
        const runId = await invoke<string>('trigger_finetune_flow', { config });
        await message(`파인튜닝 플로우를 실행했습니다 (Run ID ${runId}).`, { title: 'KubeMetal', kind: 'info' });
        await fetchStatus();
      } catch (err) {
        await message(`플로우 실행 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
      } finally {
        setTriggeringFlow(false);
      }
    },
    [fetchStatus],
  );

  const setupEvalEnv = useCallback(async () => {
    setSettingUpEvalEnv(true);
    setEvalInstallingLocal(true);
    try {
      const res = await invoke<string>('setup_eval_env');
      await message(res || '평가 환경 설치를 시작했습니다.', { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      setEvalInstallingLocal(false);
      await message(`평가 환경 설치 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setSettingUpEvalEnv(false);
    }
  }, [fetchStatus]);

  const loadEvalResults = useCallback(async () => {
    setLoadingEvalResults(true);
    try {
      const res = await invoke<EvalMetric[]>('get_eval_results');
      setEvalResults(res);
    } catch (err) {
      console.error('평가 결과 로드 오류:', err);
    } finally {
      setLoadingEvalResults(false);
    }
  }, []);

  const triggerEvaluateFlow = useCallback(
    async (tasks: string, limit: number, servingPort: number) => {
      setTriggeringEvaluate(true);
      try {
        const runId = await invoke<string>('trigger_evaluate_flow', { tasks, limit, servingPort });
        setLastEvalRunId(runId);
        await message(`평가 플로우를 실행했습니다 (Run ID ${runId}).`, { title: 'KubeMetal', kind: 'info' });
        await fetchStatus();
        await loadEvalResults();
      } catch (err) {
        await message(`평가 실행 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
      } finally {
        setTriggeringEvaluate(false);
      }
    },
    [fetchStatus, loadEvalResults],
  );

  // 탭 진입(마운트) 시 최소 1회 상태 및 평가 결과 조회
  useEffect(() => {
    setLoading(true);
    fetchStatus().finally(() => setLoading(false));
    loadEvalResults();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 백엔드가 별도의 "설치 진행 중" 상태를 주지 않으므로, env_installed가 true가 되는
  // 시점을 관찰해 로컬 설치-진행 플래그를 해제한다.
  useEffect(() => {
    if (status?.env_installed && installingLocal) setInstallingLocal(false);
  }, [status?.env_installed, installingLocal]);

  useEffect(() => {
    if (status?.eval_env_installed && evalInstallingLocal) setEvalInstallingLocal(false);
  }, [status?.eval_env_installed, evalInstallingLocal]);

  const installing = settingUpEnv || installingLocal;
  const evalInstalling = settingUpEvalEnv || evalInstallingLocal;
  const evalRunRunning =
    !!lastEvalRunId && status?.recent_runs.some((run) => run.id === lastEvalRunId && run.state_type === 'RUNNING');
  const shouldPoll = active && (installing || evalInstalling || !!status?.runner_running || evalRunRunning);

  useEffect(() => {
    if (!shouldPoll) return;
    const interval = setInterval(fetchStatus, 5000);
    return () => clearInterval(interval);
  }, [shouldPoll, fetchStatus]);

  // 평가 run이 RUNNING 상태로 폴링되는 동안에는 평가 결과도 함께 갱신한다.
  useEffect(() => {
    if (!active || !evalRunRunning) return;
    const interval = setInterval(loadEvalResults, 5000);
    return () => clearInterval(interval);
  }, [active, evalRunRunning, loadEvalResults]);

  return {
    status,
    loading,
    installing,
    settingUpEnv,
    setupEnv,
    startingRunner,
    startRunner,
    stoppingRunner,
    stopRunner,
    triggeringFlow,
    triggerFinetuneFlow,
    evalInstalling,
    settingUpEvalEnv,
    setupEvalEnv,
    triggeringEvaluate,
    triggerEvaluateFlow,
    evalResults,
    loadingEvalResults,
    loadEvalResults,
    refresh: fetchStatus,
  };
}
