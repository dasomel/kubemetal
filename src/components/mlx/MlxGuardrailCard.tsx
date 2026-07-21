import React from 'react';
import { ShieldCheck, Loader2, PlayCircle, PauseCircle, BatteryCharging, Battery, Coffee } from 'lucide-react';
import type { GuardrailStatus, MlxTrainingState } from '../../types/ipc';
import { useMlx } from '../../hooks/useMlx';

interface MlxGuardrailCardProps {
  guardrailStatus: GuardrailStatus | null;
  training?: MlxTrainingState;
  settingBatteryPause: boolean;
  onSetBatteryPause: (enabled: boolean) => void;
  resumingTraining: boolean;
  onResume: () => void;
  pausingTraining?: boolean;
  onPause?: () => void;
}

const tileLabelClass = 'text-label uppercase text-inkFaint mb-1';

const LEVEL_LABEL: Record<string, string> = {
  normal: '정상',
  warn: '경고',
  critical: '위험',
  unknown: '알 수 없음',
};

const LEVEL_DOT: Record<string, string> = {
  normal: 'bg-success',
  warn: 'bg-warning',
  critical: 'bg-danger',
  unknown: 'bg-inkFaint',
};

const PAUSE_REASON_LABEL: Record<string, string> = {
  paused: '수동으로 일시정지됨',
  paused_memory_pressure: '메모리 압력 감지로 자동 일시정지됨',
  paused_battery: '배터리 구동 감지로 자동 일시정지됨',
};

export const MlxGuardrailCard: React.FC<MlxGuardrailCardProps> = ({
  guardrailStatus,
  training,
  settingBatteryPause,
  onSetBatteryPause,
  resumingTraining,
  onResume,
  pausingTraining: propPausing,
  onPause: propOnPause,
}) => {
  const { pausingTraining: hookPausing, pauseTraining: hookOnPause } = useMlx();
  const pausingTraining = propPausing ?? hookPausing;
  const onPause = propOnPause ?? hookOnPause;

  const isPaused = !!training && training.status.startsWith('paused');
  const isRunning = !!training && (training.status === 'running' || training.status === 'training');
  const level = guardrailStatus?.memory_pressure_level ?? 'unknown';

  return (
    <div className="rounded-xl bg-surface p-6 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">Safety</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <ShieldCheck className="w-4 h-4 text-primary" />
          <span>가드레일</span>
        </h2>
      </div>

      {!guardrailStatus ? (
        <div className="py-6 text-center text-inkMuted text-body">가드레일 상태를 불러오는 중...</div>
      ) : (
        <div className="space-y-4">
          <div className="grid grid-cols-2 gap-3">
            <div className="p-3 rounded-lg bg-surfaceRaised">
              <div className={tileLabelClass}>메모리 압력</div>
              <div className="flex items-center gap-1.5 text-bodyStrong text-ink">
                <span className={`w-2 h-2 rounded-full shrink-0 ${LEVEL_DOT[level] ?? LEVEL_DOT.unknown}`} />
                <span>{LEVEL_LABEL[level] ?? level}</span>
              </div>
            </div>
            <div className="p-3 rounded-lg bg-surfaceRaised">
              <div className={tileLabelClass}>전원</div>
              <div className="flex items-center gap-1.5 text-bodyStrong text-ink">
                {guardrailStatus.on_battery ? (
                  <Battery className="w-3.5 h-3.5 text-warning" />
                ) : (
                  <BatteryCharging className="w-3.5 h-3.5 text-success" />
                )}
                <span>{guardrailStatus.on_battery ? '배터리 구동' : 'AC 전원'}</span>
              </div>
            </div>
          </div>

          <label className="flex items-center justify-between gap-3 py-1">
            <span className="text-body text-ink">배터리 구동 시 학습 자동 일시정지</span>
            <button
              type="button"
              role="switch"
              aria-checked={guardrailStatus.battery_pause_enabled}
              onClick={() => onSetBatteryPause(!guardrailStatus.battery_pause_enabled)}
              disabled={settingBatteryPause}
              className={`relative w-10 h-6 rounded-full shrink-0 transition-colors disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary ${
                guardrailStatus.battery_pause_enabled
                  ? 'bg-primaryStrong'
                  : 'bg-surfaceRaised border border-hairline/8'
              }`}
            >
              <span
                className={`absolute top-0.5 w-5 h-5 rounded-full bg-inverse shadow-panel transition-transform ${
                  guardrailStatus.battery_pause_enabled ? 'translate-x-[18px]' : 'translate-x-0.5'
                }`}
              />
            </button>
          </label>

          {guardrailStatus.caffeinate_active && (
            <div className="flex items-center gap-1.5 text-caption text-inkMuted">
              <Coffee className="w-3.5 h-3.5 text-primary" />
              <span>caffeinate 활성 — 학습 중 슬립 진입을 방지합니다.</span>
            </div>
          )}

          {isPaused && (
            <div className="pt-3 border-t border-hairline/8">
              <div className="flex items-center justify-between gap-3 p-3 rounded-lg bg-surfaceRaised">
                <div className="flex items-center gap-1.5 text-caption text-warning">
                  <span className="w-2 h-2 rounded-full bg-warning shrink-0" />
                  <span>{PAUSE_REASON_LABEL[training!.status] ?? '학습이 일시정지되었습니다.'}</span>
                </div>
                <button
                  type="button"
                  onClick={onResume}
                  disabled={resumingTraining}
                  className="py-1.5 px-3 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-caption rounded-md transition-all flex items-center gap-1.5 shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
                >
                  {resumingTraining ? (
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  ) : (
                    <PlayCircle className="w-3.5 h-3.5" />
                  )}
                  <span>재개</span>
                </button>
              </div>
            </div>
          )}

          {isRunning && (
            <div className="pt-3 border-t border-hairline/8">
              <div className="flex items-center justify-between gap-3 p-3 rounded-lg bg-surfaceRaised">
                <div className="flex items-center gap-1.5 text-caption text-success">
                  <span className="w-2 h-2 rounded-full bg-success shrink-0" />
                  <span>학습이 진행 중입니다.</span>
                </div>
                <button
                  type="button"
                  onClick={onPause}
                  disabled={pausingTraining}
                  className="py-1.5 px-3 bg-transparent hover:bg-surfaceRaised border border-hairline/8 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-caption rounded-md transition-all flex items-center gap-1.5 shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
                >
                  {pausingTraining ? (
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  ) : (
                    <PauseCircle className="w-3.5 h-3.5" />
                  )}
                  <span>일시정지</span>
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
};
