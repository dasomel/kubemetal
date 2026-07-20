import { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { MlxEnvStatus, MlxStatus, FineTuneConfig, LocalModel, GuardrailStatus } from '../types/ipc';

export function useMlx() {
  const [envStatus, setEnvStatus] = useState<MlxEnvStatus | null>(null);
  const [checkingEnv, setCheckingEnv] = useState(false);
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

  const prevEnvStateRef = useRef<string | undefined>(undefined);

  const checkEnv = useCallback(async () => {
    setCheckingEnv(true);
    try {
      const res = await invoke<MlxEnvStatus>('check_mlx_env');
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

  const startServing = useCallback(async (modelPath: string, port: number) => {
    setStartingServing(true);
    try {
      const res = await invoke<string>('start_model_serving', { modelPath, port });
      await message(res || '모델 서빙을 시작했습니다.', { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(`모델 서빙 시작 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setStartingServing(false);
    }
  }, [fetchStatus]);

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

  useEffect(() => {
    checkEnv();
    fetchStatus();
    fetchLocalModels();
  }, [checkEnv, fetchStatus, fetchLocalModels]);

  const envInstalling = mlxStatus?.env_setup?.state === 'installing';
  const trainingActive =
    !!mlxStatus?.training && mlxStatus.training.status !== 'done' && mlxStatus.training.status !== 'error';
  const shouldPoll = envInstalling || trainingActive;

  // 환경 설치 또는 파인튜닝이 진행 중일 때만 3초 간격으로 상태를 폴링한다.
  useEffect(() => {
    if (!shouldPoll) return;
    const interval = setInterval(fetchStatus, 3000);
    return () => clearInterval(interval);
  }, [shouldPoll, fetchStatus]);

  // 환경 설치가 완료되면 환경 상태와 로컬 모델 목록을 다시 확인한다.
  useEffect(() => {
    const state = mlxStatus?.env_setup?.state;
    if (prevEnvStateRef.current === 'installing' && state !== 'installing') {
      checkEnv();
    }
    prevEnvStateRef.current = state;
  }, [mlxStatus?.env_setup?.state, checkEnv]);

  // 가드레일 상태는 학습이 진행 중일 때만 조회한다(백엔드 감시 루프도 5초 주기이므로 동일하게 맞춘다).
  useEffect(() => {
    if (!trainingActive) return;
    fetchGuardrailStatus();
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
    resumingTraining,
    resumeTraining,
  };
}
