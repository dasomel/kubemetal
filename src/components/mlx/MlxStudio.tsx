import React from 'react';
import { useMlx } from '../../hooks/useMlx';
import { MlxEnvCard } from './MlxEnvCard';
import { MlxFineTuneCard } from './MlxFineTuneCard';
import { MlxServingCard } from './MlxServingCard';

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
  } = useMlx();

  return (
    <div className="space-y-6">
      <MlxEnvCard
        envStatus={envStatus}
        envSetup={mlxStatus?.env_setup}
        checkingEnv={checkingEnv}
        settingUp={settingUpEnv}
        onSetup={setupEnv}
      />

      <MlxFineTuneCard
        localModels={localModels}
        training={mlxStatus?.training}
        starting={startingTraining}
        killingPid={killingPid}
        onStart={runFinetune}
        onKill={killProcess}
      />

      <MlxServingCard
        serving={mlxStatus?.serving}
        adapterPath={mlxStatus?.training?.adapter_path}
        starting={startingServing}
        stopping={stoppingServing}
        onStart={startServing}
        onStop={stopServing}
      />
    </div>
  );
};
