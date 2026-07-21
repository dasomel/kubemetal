import React from 'react';
import { Cpu, Loader2 } from 'lucide-react';
import type { MlxEnvStatus, MlxEnvSetupState } from '../../types/ipc';

interface MlxEnvCardProps {
  envStatus: MlxEnvStatus | null;
  envSetup?: MlxEnvSetupState;
  checkingEnv: boolean;
  settingUp: boolean;
  onSetup: () => void;
  /** 환경이 이미 준비되어 파인튜닝/서빙에 자리를 내줄 때 한 줄 배지로 접는다. */
  compact?: boolean;
}

const StatusRow: React.FC<{ checking: boolean; ok?: boolean; readyLabel: string; notReadyLabel: string }> = ({
  checking,
  ok,
  readyLabel,
  notReadyLabel,
}) => {
  if (checking) {
    return (
      <div className="flex items-center gap-1.5 text-caption text-inkFaint">
        <Loader2 className="w-3 h-3 animate-spin" />
        <span>확인 중…</span>
      </div>
    );
  }
  return (
    <div className="flex items-center gap-1.5 text-caption text-inkMuted">
      <span className={`w-2 h-2 rounded-full ${ok ? 'bg-success' : 'bg-inkFaint'}`} />
      <span>{ok ? readyLabel : notReadyLabel}</span>
    </div>
  );
};

export const MlxEnvCard: React.FC<MlxEnvCardProps> = ({
  envStatus,
  envSetup,
  checkingEnv,
  settingUp,
  onSetup,
  compact = false,
}) => {
  const installing = settingUp || envSetup?.state === 'installing';
  const ready = envStatus?.venv_exists && envStatus?.mlx_lm_installed;
  // 최초 확인 전/중에는 envStatus가 아직 없거나 checkingEnv가 true다 — 이때는 "확인 필요"
  // 대신 "확인 중…"을 노출해 미확인 상태를 오확인 상태처럼 보이지 않게 한다.
  const checking = checkingEnv || !envStatus;

  if (compact && ready) {
    return (
      <div className="animate-card-in rounded-xl bg-surface p-4 shadow-panel flex items-center gap-2">
        <Cpu className="w-4 h-4 text-primary shrink-0" />
        <span className="text-bodyStrong text-ink">MLX 환경</span>
        <span className="flex items-center gap-1.5 text-caption text-inkMuted">
          <span className="w-2 h-2 rounded-full bg-success" />
          <span>
            준비됨{envStatus?.mlx_lm_version ? ` · mlx-lm v${envStatus.mlx_lm_version}` : ''}
          </span>
        </span>
      </div>
    );
  }

  return (
    <div className="animate-card-in rounded-xl bg-surface p-4 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">Environment</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Cpu className="w-4 h-4 text-primary" />
          <span>MLX 환경</span>
        </h2>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-2 mb-4">
        <div className="p-3 rounded-lg bg-surfaceRaised">
          <div className="text-bodyStrong text-ink mb-2">Python 3</div>
          <StatusRow checking={checking} ok={envStatus?.python_ok} readyLabel="사용 가능" notReadyLabel="확인 필요" />
        </div>

        <div className="p-3 rounded-lg bg-surfaceRaised">
          <div className="text-bodyStrong text-ink mb-2">가상환경 (venv)</div>
          <StatusRow checking={checking} ok={envStatus?.venv_exists} readyLabel="생성됨" notReadyLabel="미생성" />
        </div>

        <div className="p-3 rounded-lg bg-surfaceRaised">
          <div className="text-bodyStrong text-ink mb-2">mlx-lm</div>
          <StatusRow
            checking={checking}
            ok={envStatus?.mlx_lm_installed}
            readyLabel={`설치됨${envStatus?.mlx_lm_version ? ` · v${envStatus.mlx_lm_version}` : ''}`}
            notReadyLabel="미설치"
          />
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
