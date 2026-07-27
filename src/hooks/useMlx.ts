import { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { MlxEnvStatus, MlxStatus, FineTuneConfig, LocalModel, GuardrailStatus } from '../types/ipc';

// 모듈 레벨 캐시 — 탭을 벗어났다 재진입해도 이미 확인된 MLX 환경 상태를 재사용해
// 불필요한 재확인 호출을 피한다. 환경 설치가 완료됐을 때만(useMlx 내부에서) 갱신한다.
let cachedEnvStatus: MlxEnvStatus | null = null;
let hasCachedEnvStatus = false;

export function useMlx() {
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
      console.error('MLX 환경 확인 오류:', err);
    } finally {
      setCheckingEnv(false);
    }
  }, []);

  const fetchStatus = useCallback(async () => {
    try {
      const res = await invoke<MlxStatus>('get_mlx_status');
      setMlxStatus(res);
    } catch (err) {
      console.error('MLX 상태 로드 오류:', err);
    }
  }, []);

  const fetchLocalModels = useCallback(async () => {
    try {
      const res = await invoke<LocalModel[]>('list_local_models');
      setLocalModels(res);
    } catch (err) {
      console.error('로컬 모델 목록 로드 오류:', err);
    }
  }, []);

  const setupEnv = useCallback(async () => {
    setSettingUpEnv(true);
    try {
      const res = await invoke<string>('setup_mlx_env');
      await message(res || 'MLX 환경 설치를 시작했습니다.', { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(`MLX 환경 설치 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setSettingUpEnv(false);
    }
  }, [fetchStatus]);

  const runFinetune = useCallback(async (config: FineTuneConfig) => {
    setStartingTraining(true);
    try {
      const pid = await invoke<number>('run_mlx_finetune', { config });
      await message(`파인튜닝을 시작했습니다 (PID ${pid}).`, { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(`파인튜닝 시작 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setStartingTraining(false);
    }
  }, [fetchStatus]);

  const killProcess = useCallback(async (pid: number) => {
    setKillingPid(pid);
    try {
      const res = await invoke<string>('kill_mlx_process', { pid });
      await message(res || '프로세스를 종료했습니다.', { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(`프로세스 종료 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setKillingPid(null);
    }
  }, [fetchStatus]);

  const startServing = useCallback(
    async (modelPath: string, adapterPath: string | undefined, port: number) => {
      setStartingServing(true);
      try {
        const res = await invoke<string>('start_model_serving', {
          modelPath,
          adapterPath: adapterPath || null,
          port,
        });
        await message(res || '모델 서빙을 시작했습니다.', { title: 'KubeMetal', kind: 'info' });
        await fetchStatus();
      } catch (err) {
        await message(`모델 서빙 시작 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
      } finally {
        setStartingServing(false);
      }
    },
    [fetchStatus],
  );

  const stopServing = useCallback(async () => {
    setStoppingServing(true);
    try {
      const res = await invoke<string>('stop_model_serving');
      await message(res || '모델 서빙을 정지했습니다.', { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(`모델 서빙 정지 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setStoppingServing(false);
    }
  }, [fetchStatus]);

  const fetchGuardrailStatus = useCallback(async () => {
    try {
      const res = await invoke<GuardrailStatus>('get_guardrail_status');
      setGuardrailStatus(res);
    } catch (err) {
      console.error('가드레일 상태 로드 오류:', err);
    }
  }, []);

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
        await message(`발열 일시정지 설정 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
      }
    },
    [fetchGuardrailStatus, guardrailStatus],
  );

  const setBatteryPause = useCallback(
    async (enabled: boolean) => {
      setSettingBatteryPause(true);
      try {
        await invoke('set_guardrail_config', { batteryPause: enabled });
        await fetchGuardrailStatus();
      } catch (err) {
        await message(`배터리 일시정지 설정 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
      } finally {
        setSettingBatteryPause(false);
      }
    },
    [fetchGuardrailStatus],
  );

  const resumeTraining = useCallback(async () => {
    setResumingTraining(true);
    try {
      await invoke('resume_mlx_training');
      await fetchStatus();
      await fetchGuardrailStatus();
    } catch (err) {
      await message(`학습 재개 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setResumingTraining(false);
    }
  }, [fetchStatus, fetchGuardrailStatus]);

  const pauseTraining = useCallback(async () => {
    setPausingTraining(true);
    try {
      await invoke('pause_mlx_training');
      await fetchStatus();
      await fetchGuardrailStatus();
    } catch (err) {
      await message(`학습 일시정지 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setPausingTraining(false);
    }
  }, [fetchStatus, fetchGuardrailStatus]);

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
