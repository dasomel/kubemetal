import { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type {
  MlxEnvStatus,
  MlxStatus,
  FineTuneConfig,
  LocalModel,
  GuardrailStatus,
  MlxRuntime,
} from '../types/ipc';
import { useTranslation } from '../i18n/i18nContext';

// 모듈 레벨 캐시 — 탭을 벗어났다 재진입해도 이미 확인된 MLX 환경 상태를 재사용해
// 불필요한 재확인 호출을 피한다. 환경 설치가 완료됐을 때만(useMlx 내부에서) 갱신한다.
let cachedEnvStatus: MlxEnvStatus | null = null;
let hasCachedEnvStatus = false;

export function useMlx() {
  const { t } = useTranslation();
  const [envStatus, setEnvStatus] = useState<MlxEnvStatus | null>(cachedEnvStatus);
  const [checkingEnv, setCheckingEnv] = useState(!hasCachedEnvStatus);
  const [settingUpEnv, setSettingUpEnv] = useState(false);
  const [mlxStatus, setMlxStatus] = useState<MlxStatus | null>(null);
  const [localModels, setLocalModels] = useState<LocalModel[]>([]);
  const [startingTraining, setStartingTraining] = useState(false);
  const [killingPid, setKillingPid] = useState<number | null>(null);
  const [startingServing, setStartingServing] = useState(false);
  const [stoppingServing, setStoppingServing] = useState(false);
  const [guardrailStatus, setGuardrailStatus] = useState<GuardrailStatus | null>(null);
  const [settingBatteryPause, setSettingBatteryPause] = useState(false);
  const [resumingTraining, setResumingTraining] = useState(false);
  const [pausingTraining, setPausingTraining] = useState(false);

  const prevEnvStateRef = useRef<string | undefined>(undefined);

  const checkEnv = useCallback(async () => {
    setCheckingEnv(true);
    try {
      const res = await invoke<MlxEnvStatus>('check_mlx_env');
      cachedEnvStatus = res;
      hasCachedEnvStatus = true;
      setEnvStatus(res);
    } catch (err) {
      console.error(t('mlx.err.envCheck'), err);
    } finally {
      setCheckingEnv(false);
    }
  }, [t]);

  const fetchStatus = useCallback(async () => {
    try {
      const res = await invoke<MlxStatus>('get_mlx_status');
      setMlxStatus(res);
    } catch (err) {
      console.error(t('mlx.err.statusLoad'), err);
    }
  }, [t]);

  const fetchLocalModels = useCallback(async () => {
    try {
      const res = await invoke<LocalModel[]>('list_local_models');
      setLocalModels(res);
    } catch (err) {
      console.error(t('mlx.err.localModelsLoad'), err);
    }
  }, [t]);

  const setupEnv = useCallback(async () => {
    setSettingUpEnv(true);
    try {
      const res = await invoke<string>('setup_mlx_env');
      await message(res || t('mlx.toast.envSetupStarted'), { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(t('mlx.toast.envSetupFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
    } finally {
      setSettingUpEnv(false);
    }
  }, [fetchStatus, t]);

  const runFinetune = useCallback(async (config: FineTuneConfig) => {
    setStartingTraining(true);
    try {
      const pid = await invoke<number>('run_mlx_finetune', { config });
      await message(t('mlx.toast.finetuneStarted', { pid }), { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(t('mlx.toast.finetuneStartFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
    } finally {
      setStartingTraining(false);
    }
  }, [fetchStatus, t]);

  const killProcess = useCallback(async (pid: number) => {
    setKillingPid(pid);
    try {
      // 백엔드는 bool을 돌려준다. 예전에는 invoke<string>으로 선언해두고 `res || 기본문구`를
      // 썼는데, `true`는 truthy라 기본 문구가 절대 쓰이지 않고 message(true)가 그대로
      // 호출돼 "expected a string"으로 던졌다 — 그 예외가 catch로 떨어져 프로세스가
      // **정상 종료됐는데도** "종료 실패"가 표시됐다(실측 2026-08-21). 타입 주석이
      // 거짓이면 tsc는 아무것도 잡지 못한다.
      await invoke<boolean>('kill_mlx_process', { pid });
      await message(t('mlx.toast.processKilled'), { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(t('mlx.toast.processKillFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
    } finally {
      setKillingPid(null);
    }
  }, [fetchStatus, t]);

  const startServing = useCallback(
    async (
      modelPath: string,
      adapterPath: string | undefined,
      port: number,
      runtime?: MlxRuntime,
    ) => {
      setStartingServing(true);
      try {
        const res = await invoke<string>('start_model_serving', {
          modelPath,
          adapterPath: adapterPath || null,
          port,
          // 생략하면 백엔드 기본(mlx-lm) — 기존 호출부의 동작이 바뀌지 않는다(D29).
          runtime: runtime ?? null,
        });
        await message(res || t('mlx.toast.servingStarted'), { title: 'KubeMetal', kind: 'info' });
        await fetchStatus();
      } catch (err) {
        await message(t('mlx.toast.servingStartFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
      } finally {
        setStartingServing(false);
      }
    },
    [fetchStatus, t],
  );

  const stopServing = useCallback(async () => {
    setStoppingServing(true);
    try {
      const res = await invoke<string>('stop_model_serving');
      await message(res || t('mlx.toast.servingStopped'), { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(t('mlx.toast.servingStopFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
    } finally {
      setStoppingServing(false);
    }
  }, [fetchStatus, t]);

  const fetchGuardrailStatus = useCallback(async () => {
    try {
      const res = await invoke<GuardrailStatus>('get_guardrail_status');
      setGuardrailStatus(res);
    } catch (err) {
      console.error(t('mlx.err.guardrailLoad'), err);
    }
  }, [t]);

  // 백엔드는 thermalPause를 Option으로 받는다 — 여기서 함께 보내지 않으면 발열 설정이
  // 그대로 유지되고, 두 토글이 서로의 값을 덮어쓰지 않는다.
  const setThermalPause = useCallback(
    async (enabled: boolean) => {
      try {
        await invoke('set_guardrail_config', {
          batteryPause: guardrailStatus?.battery_pause_enabled ?? false,
          thermalPause: enabled,
        });
        await fetchGuardrailStatus();
      } catch (err) {
        await message(t('mlx.toast.thermalPauseSetFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
      }
    },
    [fetchGuardrailStatus, guardrailStatus, t],
  );

  const setBatteryPause = useCallback(
    async (enabled: boolean) => {
      setSettingBatteryPause(true);
      try {
        await invoke('set_guardrail_config', { batteryPause: enabled });
        await fetchGuardrailStatus();
      } catch (err) {
        await message(t('mlx.toast.batteryPauseSetFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
      } finally {
        setSettingBatteryPause(false);
      }
    },
    [fetchGuardrailStatus, t],
  );

  const resumeTraining = useCallback(async () => {
    setResumingTraining(true);
    try {
      await invoke('resume_mlx_training');
      await fetchStatus();
      await fetchGuardrailStatus();
    } catch (err) {
      await message(t('mlx.toast.resumeTrainingFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
    } finally {
      setResumingTraining(false);
    }
  }, [fetchStatus, fetchGuardrailStatus, t]);

  const pauseTraining = useCallback(async () => {
    setPausingTraining(true);
    try {
      await invoke('pause_mlx_training');
      await fetchStatus();
      await fetchGuardrailStatus();
    } catch (err) {
      await message(t('mlx.toast.pauseTrainingFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
    } finally {
      setPausingTraining(false);
    }
  }, [fetchStatus, fetchGuardrailStatus, t]);

  useEffect(() => {
    checkEnv();
    fetchStatus();
    fetchLocalModels();
    fetchGuardrailStatus();
  }, [checkEnv, fetchStatus, fetchLocalModels, fetchGuardrailStatus]);

  const envInstalling = mlxStatus?.env_setup?.state === 'installing';
  const trainingActive =
    !!mlxStatus?.training && mlxStatus.training.status !== 'done' && mlxStatus.training.status !== 'error';
  const shouldPoll = envInstalling || trainingActive;

  useEffect(() => {
    if (!shouldPoll) return;
    const interval = setInterval(fetchStatus, 3000);
    return () => clearInterval(interval);
  }, [shouldPoll, fetchStatus]);

  useEffect(() => {
    const state = mlxStatus?.env_setup?.state;
    if (prevEnvStateRef.current === 'installing' && state === 'done') {
      hasCachedEnvStatus = false;
      checkEnv();
    }
    prevEnvStateRef.current = state;
  }, [mlxStatus?.env_setup?.state, checkEnv]);

  useEffect(() => {
    if (!trainingActive) return;
    const interval = setInterval(fetchGuardrailStatus, 5000);
    return () => clearInterval(interval);
  }, [trainingActive, fetchGuardrailStatus]);

  return {
    envStatus,
    checkingEnv,
    settingUpEnv,
    setupEnv,
    mlxStatus,
    localModels,
    refreshLocalModels: fetchLocalModels,
    startingTraining,
    runFinetune,
    killingPid,
    killProcess,
    startingServing,
    startServing,
    stoppingServing,
    stopServing,
    guardrailStatus,
    settingBatteryPause,
    setBatteryPause,
    setThermalPause,
    resumingTraining,
    resumeTraining,
    pausingTraining,
    pauseTraining,
  };
}
