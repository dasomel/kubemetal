import React from 'react';
import { useMlx } from '../../hooks/useMlx';
import { useTranslation } from '../../i18n/i18nContext';
import { MlxEnvCard } from './MlxEnvCard';
import { MlxFineTuneCard } from './MlxFineTuneCard';
import { MlxGuardrailCard } from './MlxGuardrailCard';
import { MlxServingCard } from './MlxServingCard';
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

  // 환경이 준비되기 전엔 설치 카드만 히어로로 노출하고, 파인튜닝/가드레일/서빙은
  // "다음 단계" 잠긴 프리뷰로 축소한다. 준비되면 환경 카드는 한 줄 배지로 접혀
  // 파인튜닝 카드(진행 상황 포함)가 자연스럽게 상단으로 올라온다.
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
