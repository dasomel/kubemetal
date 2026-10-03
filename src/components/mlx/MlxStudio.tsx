import React, { useState } from 'react';
import { useMlx } from '../../hooks/useMlx';
import { useServingRollback } from '../../hooks/useServingRollback';
import { useTranslation } from '../../i18n/i18nContext';
import { MlxEnvCard } from './MlxEnvCard';
import { MlxFineTuneCard } from './MlxFineTuneCard';
import { MlxGuardrailCard } from './MlxGuardrailCard';
import { GpuBenchmarkCard } from './GpuBenchmarkCard';
import { MlxServingCard } from './MlxServingCard';
import { MlxOrphanedProcessesCard } from './MlxOrphanedProcessesCard';
import { LocalInferenceRuntimeCard } from './LocalInferenceRuntimeCard';
import { LocalInferenceBridgeCard } from './LocalInferenceBridgeCard';
import { LocalInferenceOpsCard } from './LocalInferenceOpsCard';
import { LocalInferenceBenchmarkCard } from './LocalInferenceBenchmarkCard';
import { LocalInferenceReadinessCard } from './LocalInferenceReadinessCard';
import { LockedPreview } from '../dashboard/LockedPreview';
import { MlxStudioTabs, getStoredMlxSubTab, setStoredMlxSubTab, type MlxSubTab } from './MlxStudioTabs';

export const MlxStudio: React.FC = () => {
  const [activeTab, setActiveTabState] = useState<MlxSubTab>(getStoredMlxSubTab);

  const handleTabChange = (tab: MlxSubTab) => {
    setActiveTabState(tab);
    setStoredMlxSubTab(tab);
  };

  const {
    envStatus,
    checkingEnv,
    settingUpEnv,
    setupEnv,
    mlxStatus,
    fetchStatus,
    localModels,
    startingTraining,
    runFinetune,
    deleteAdapterCheckpoint,
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
  const { lastKnownGoodServing, revertingServing, revertServing } = useServingRollback(mlxStatus?.serving, fetchStatus);
  const { t } = useTranslation();

  const envReady = !!(envStatus?.venv_exists && envStatus?.mlx_lm_installed);

  const fineTune = (
    <MlxFineTuneCard
      localModels={localModels}
      training={mlxStatus?.training}
      starting={startingTraining}
      killingPid={killingPid}
      onStart={runFinetune}
      onKill={killProcess}
      onDeleteAdapter={deleteAdapterCheckpoint}
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
      lastKnownGoodServing={lastKnownGoodServing}
      reverting={revertingServing}
      onRevert={revertServing}
    />
  );

  return (
    <div className="space-y-4">
      <MlxStudioTabs activeTab={activeTab} onTabChange={handleTabChange} />

      {/* 환경·진단 / Environment */}
      <div
        role="tabpanel"
        id="mlx-panel-env"
        aria-labelledby="mlx-tab-env"
        hidden={activeTab !== 'env'}
        className={activeTab === 'env' ? 'space-y-4 focus-visible:outline-none' : 'hidden'}
        tabIndex={0}
      >
        <MlxEnvCard
          envStatus={envStatus}
          envSetup={mlxStatus?.env_setup}
          checkingEnv={checkingEnv}
          settingUp={settingUpEnv}
          onSetup={setupEnv}
          compact={envReady}
        />
        <GpuBenchmarkCard />
        <LocalInferenceReadinessCard />
        <MlxOrphanedProcessesCard />
      </div>

      {/* 추론 런타임 / Runtime */}
      <div
        role="tabpanel"
        id="mlx-panel-runtime"
        aria-labelledby="mlx-tab-runtime"
        hidden={activeTab !== 'runtime'}
        className={activeTab === 'runtime' ? 'space-y-4 focus-visible:outline-none' : 'hidden'}
        tabIndex={0}
      >
        <LocalInferenceRuntimeCard />
        <LocalInferenceBridgeCard />
        <LocalInferenceOpsCard />
        <LocalInferenceBenchmarkCard />
      </div>

      {/* 학습 / Training */}
      <div
        role="tabpanel"
        id="mlx-panel-training"
        aria-labelledby="mlx-tab-training"
        hidden={activeTab !== 'training'}
        className={activeTab === 'training' ? 'space-y-4 focus-visible:outline-none' : 'hidden'}
        tabIndex={0}
      >
        {envReady ? (
          <>
            {fineTune}
            {guardrail}
          </>
        ) : (
          <>
            <LockedPreview caption={t('mlx.lockedFinetune')}>{fineTune}</LockedPreview>
            <LockedPreview caption={t('mlx.lockedGuardrail')}>{guardrail}</LockedPreview>
          </>
        )}
      </div>

      {/* 서빙·채팅 / Serving & Chat */}
      <div
        role="tabpanel"
        id="mlx-panel-serving"
        aria-labelledby="mlx-tab-serving"
        hidden={activeTab !== 'serving'}
        className={activeTab === 'serving' ? 'space-y-4 focus-visible:outline-none' : 'hidden'}
        tabIndex={0}
      >
        {envReady ? (
          serving
        ) : (
          <LockedPreview caption={t('mlx.lockedServing')}>{serving}</LockedPreview>
        )}
      </div>
    </div>
  );
};
