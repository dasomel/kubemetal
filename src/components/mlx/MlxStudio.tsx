import React from 'react';
import { useMlx } from '../../hooks/useMlx';
import { useTranslation } from '../../i18n/i18nContext';
import { MlxEnvCard } from './MlxEnvCard';
import { MlxFineTuneCard } from './MlxFineTuneCard';
import { MlxGuardrailCard } from './MlxGuardrailCard';
import { MlxServingCard } from './MlxServingCard';
import { LocalInferenceRuntimeCard } from './LocalInferenceRuntimeCard';
import { LocalInferenceBridgeCard } from './LocalInferenceBridgeCard';
import { LockedPreview } from '../dashboard/LockedPreview';

export const MlxStudio: React.FC = () => {
  const {
    envStatus,
    checkingEnv,
    settingUpEnv,
    setupEnv,
    mlxStatus,
    localModels,
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
  } = useMlx();
  const { t } = useTranslation();

  // The oMLX runtime is independent from KubeMetal's private mlx-lm venv, so expose its
  // discovery/lifecycle card even when the fine-tuning environment has not been installed.
  const envReady = !!(envStatus?.venv_exists && envStatus?.mlx_lm_installed);

  const fineTune = (
    <MlxFineTuneCard
      localModels={localModels}
      training={mlxStatus?.training}
      starting={startingTraining}
      killingPid={killingPid}
      onStart={runFinetune}
      onKill={killProcess}
    />
  );

  const guardrail = (
    <MlxGuardrailCard
      guardrailStatus={guardrailStatus}
      training={mlxStatus?.training}
      settingBatteryPause={settingBatteryPause}
      onSetBatteryPause={setBatteryPause}
      onSetThermalPause={setThermalPause}
      resumingTraining={resumingTraining}
      onResume={resumeTraining}
    />
  );

  const serving = (
    <MlxServingCard
      serving={mlxStatus?.serving}
      lastServingError={mlxStatus?.last_serving_error}
      localModels={localModels}
      adapterPathHint={mlxStatus?.training?.adapter_path}
      starting={startingServing}
      stopping={stoppingServing}
      onStart={startServing}
      onStop={stopServing}
      vlmAvailable={!!envStatus?.mlx_vlm_installed}
    />
  );

  return (
    <div className="space-y-4">
      <MlxEnvCard
        envStatus={envStatus}
        envSetup={mlxStatus?.env_setup}
        checkingEnv={checkingEnv}
        settingUp={settingUpEnv}
        onSetup={setupEnv}
        compact={envReady}
      />

      <LocalInferenceRuntimeCard />
      <LocalInferenceBridgeCard />

      {envReady ? (
        <>
          {fineTune}
          {guardrail}
          {serving}
        </>
      ) : (
        <>
          <LockedPreview caption={t('mlx.lockedFinetune')}>{fineTune}</LockedPreview>
          <LockedPreview caption={t('mlx.lockedGuardrail')}>{guardrail}</LockedPreview>
          <LockedPreview caption={t('mlx.lockedServing')}>{serving}</LockedPreview>
        </>
      )}
    </div>
  );
};
