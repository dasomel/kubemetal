import React from 'react';
import { Cpu, Loader2 } from 'lucide-react';
import type { MlxEnvStatus, MlxEnvSetupState } from '../../types/ipc';

interface MlxEnvCardProps {
  envStatus: MlxEnvStatus | null;
  envSetup?: MlxEnvSetupState;
  checkingEnv: boolean;
  settingUp: boolean;
  onSetup: () => void;
}

export const MlxEnvCard: React.FC<MlxEnvCardProps> = ({ envStatus, envSetup, checkingEnv, settingUp, onSetup }) => {
  const installing = settingUp || envSetup?.state === 'installing';
  const ready = envStatus?.venv_exists && envStatus?.mlx_lm_installed;

  return (
    <div className="rounded-xl bg-surface p-6 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">Environment</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Cpu className="w-4 h-4 text-primary" />
          <span>MLX 환경</span>
        </h2>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-3 mb-5">
        <div className="p-4 rounded-lg bg-surfaceRaised">
          <div className="text-bodyStrong text-ink mb-2">Python 3</div>
          <div className="flex items-center gap-1.5 text-caption text-inkMuted">
            <span className={`w-2 h-2 rounded-full ${envStatus?.python_ok ? 'bg-success' : 'bg-inkFaint'}`} />
            <span>{envStatus?.python_ok ? '사용 가능' : '확인 필요'}</span>
          </div>
        </div>

        <div className="p-4 rounded-lg bg-surfaceRaised">
          <div className="text-bodyStrong text-ink mb-2">가상환경 (venv)</div>
          <div className="flex items-center gap-1.5 text-caption text-inkMuted">
            <span className={`w-2 h-2 rounded-full ${envStatus?.venv_exists ? 'bg-success' : 'bg-inkFaint'}`} />
            <span>{envStatus?.venv_exists ? '생성됨' : '미생성'}</span>
          </div>
        </div>

        <div className="p-4 rounded-lg bg-surfaceRaised">
          <div className="text-bodyStrong text-ink mb-2">mlx-lm</div>
          <div className="flex items-center gap-1.5 text-caption text-inkMuted">
            <span className={`w-2 h-2 rounded-full ${envStatus?.mlx_lm_installed ? 'bg-success' : 'bg-inkFaint'}`} />
            <span>
              {envStatus?.mlx_lm_installed
                ? `설치됨${envStatus.mlx_lm_version ? ` · v${envStatus.mlx_lm_version}` : ''}`
                : '미설치'}
            </span>
          </div>
        </div>
      </div>

      {envSetup?.state === 'error' && envSetup.error && (
        <div className="mb-4 flex items-center gap-1.5 text-caption text-danger">
          <span className="w-2 h-2 rounded-full bg-danger" />
          <span>환경 설치 오류: {envSetup.error}</span>
        </div>
      )}

      {!ready && (
        <button
          type="button"
          onClick={onSetup}
          disabled={installing || checkingEnv}
          className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        >
          {installing ? <Loader2 className="w-4 h-4 animate-spin" /> : <Cpu className="w-4 h-4" />}
          <span>{installing ? '환경 설치 중...' : '환경 설치'}</span>
        </button>
      )}
    </div>
  );
};
