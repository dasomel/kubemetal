import React from 'react';
import { ShieldCheck, Loader2, PlayCircle, PauseCircle, BatteryCharging, Battery, Coffee } from 'lucide-react';
import type { GuardrailStatus, MlxTrainingState } from '../../types/ipc';
import { useMlx } from '../../hooks/useMlx';
import { useTranslation } from '../../i18n/i18nContext';

interface MlxGuardrailCardProps {
  guardrailStatus: GuardrailStatus | null;
  training?: MlxTrainingState;
  settingBatteryPause: boolean;
  onSetBatteryPause: (enabled: boolean) => void;
  onSetThermalPause?: (enabled: boolean) => void;
  resumingTraining: boolean;
  onResume: () => void;
  pausingTraining?: boolean;
  onPause?: () => void;
}

const tileLabelClass = 'text-label uppercase text-inkFaint mb-1';

const LEVEL_DOT: Record<string, string> = {
  normal: 'bg-success',
  warn: 'bg-warning',
  critical: 'bg-danger',
  unknown: 'bg-inkFaint',
};

export const MlxGuardrailCard: React.FC<MlxGuardrailCardProps> = ({
  guardrailStatus,
  training,
  settingBatteryPause,
  onSetBatteryPause,
  onSetThermalPause,
  resumingTraining,
  onResume,
  pausingTraining: propPausing,
  onPause: propOnPause,
}) => {
  // 이 컴포넌트가 useMlx()를 또 호출하면 부모와 **다른 인스턴스**가 만들어져, 토글이
  // 백엔드에는 써도 화면에 보이는 guardrailStatus(부모 prop)는 갱신되지 않는다.
  // 실제로 그렇게 동작했다(2026-07-27 앱 실측: 토글을 눌러도 스위치가 그대로였다).
  // 그래서 부모가 넘긴 핸들러를 우선 쓰고, 훅 폴백은 마지막 수단으로만 남긴다.
  const { pausingTraining: hookPausing, pauseTraining: hookOnPause, setThermalPause: hookSetThermal } = useMlx();
  const { t } = useTranslation();
  const pausingTraining = propPausing ?? hookPausing;
  const onPause = propOnPause ?? hookOnPause;
  const setThermalPause = onSetThermalPause ?? hookSetThermal;

  const levelLabel: Record<string, string> = {
    normal: t('mlx.guardrail.level.normal'),
    warn: t('mlx.guardrail.level.warn'),
    critical: t('mlx.guardrail.level.critical'),
    unknown: t('mlx.guardrail.level.unknown'),
  };

  const pauseReasonLabel: Record<string, string> = {
    paused: t('mlx.guardrail.pauseReason.paused'),
    paused_memory_pressure: t('mlx.guardrail.pauseReason.pausedMemory'),
    paused_battery: t('mlx.guardrail.pauseReason.pausedBattery'),
    paused_thermal: t('mlx.guardrail.pauseReason.pausedThermal'),
  };

  // 발열은 메모리 압력과 척도가 다르다 — fair는 부하가 걸린 정상 상태라 경고로 칠하지 않는다.
  const thermalDot: Record<string, string> = {
    nominal: 'bg-success',
    fair: 'bg-success',
    serious: 'bg-warning',
    critical: 'bg-danger',
  };
  const thermalLabel: Record<string, string> = {
    nominal: t('mlx.guardrail.thermal.nominal'),
    fair: t('mlx.guardrail.thermal.fair'),
    serious: t('mlx.guardrail.thermal.serious'),
    critical: t('mlx.guardrail.thermal.critical'),
  };
  const thermal = guardrailStatus?.thermal_state ?? null;

  const isPaused = !!training && training.status.startsWith('paused');
  const isRunning = !!training && (training.status === 'running' || training.status === 'training');
  const level = guardrailStatus?.memory_pressure_level ?? 'unknown';

  const causeLabelMap: Record<string, string> = {
    memory_pressure: t('mlx.causeMemoryPressure'),
    battery: t('mlx.causeBattery'),
    thermal: t('mlx.causeThermal'),
  };
  const resumeOverrides = guardrailStatus?.resume_overrides ?? [];
  const hasResumeOverrides = resumeOverrides.length > 0;
  const causes = resumeOverrides.map((cause) => causeLabelMap[cause] ?? cause).join(', ');

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">Safety</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <ShieldCheck className="w-4 h-4 text-primary" />
          <span>{t('mlx.guardrailTitle')}</span>
        </h2>
      </div>

      {!guardrailStatus ? (
        <div className="py-6 text-center text-inkMuted text-body">
          {t('mlx.guardrail.loadingStatus')}
        </div>
      ) : (
        <div className="space-y-4">
          <div className="grid grid-cols-3 gap-2">
            <div className="p-3 rounded-lg bg-surfaceRaised">
              <div className={tileLabelClass}>{t('mlx.guardrail.memoryPressureLabel')}</div>
              <div className="flex items-center gap-1.5 text-bodyStrong text-ink">
                <span className={`w-2 h-2 rounded-full shrink-0 ${LEVEL_DOT[level] ?? LEVEL_DOT.unknown}`} />
                <span>{levelLabel[level] ?? level}</span>
              </div>
            </div>
            <div className="p-3 rounded-lg bg-surfaceRaised">
              <div className={tileLabelClass}>{t('mlx.guardrail.powerLabel')}</div>
              <div className="flex items-center gap-1.5 text-bodyStrong text-ink">
                {guardrailStatus.on_battery ? (
                  <Battery className="w-3.5 h-3.5 text-warning" />
                ) : (
                  <BatteryCharging className="w-3.5 h-3.5 text-success" />
                )}
                <span>
                  {guardrailStatus.on_battery
                    ? t('mlx.guardrail.batteryPower')
                    : t('mlx.guardrail.acPower')}
                </span>
              </div>
            </div>
            <div className="p-3 rounded-lg bg-surfaceRaised">
              <div className={tileLabelClass}>{t('mlx.guardrail.thermalLabel')}</div>
              <div className="flex items-center gap-1.5 text-bodyStrong text-ink">
                {/* 값이 없으면 "정상"으로 채우지 않고 미상으로 둔다(D22). */}
                <span className={`w-2 h-2 rounded-full shrink-0 ${thermal ? thermalDot[thermal] : 'bg-inkFaint'}`} />
                <span>
                  {thermal
                    ? thermalLabel[thermal]
                    : t('mlx.guardrail.thermalUnavailable')}
                </span>
              </div>
            </div>
          </div>

          <label className="flex items-center justify-between gap-3 py-1">
            <span className="text-body text-ink">
              {t('mlx.guardrail.thermalPauseToggleLabel')}
            </span>
            <button
              type="button"
              role="switch"
              aria-checked={guardrailStatus.thermal_pause_enabled}
              onClick={() => setThermalPause(!guardrailStatus.thermal_pause_enabled)}
              className={`relative w-10 h-6 rounded-full transition-colors shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary ${
                guardrailStatus.thermal_pause_enabled ? 'bg-primaryStrong' : 'bg-surfaceRaised'
              }`}
            >
              <span
                className={`absolute top-0.5 w-5 h-5 rounded-full bg-surface shadow-panel transition-transform ${
                  guardrailStatus.thermal_pause_enabled ? 'translate-x-[18px]' : 'translate-x-0.5'
                }`}
              />
            </button>
          </label>

          <label className="flex items-center justify-between gap-3 py-1">
            <span className="text-body text-ink">{t('mlx.batteryPauseToggle')}</span>
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
              <span>
                {t('mlx.guardrail.caffeinateNotice')}
              </span>
            </div>
          )}

          {hasResumeOverrides && isRunning && (
            <div className="flex items-center gap-1.5 text-caption text-inkMuted">
              <ShieldCheck className="w-3.5 h-3.5 text-primary shrink-0" />
              <span>{t('mlx.guardrailOverrideActive', { causes })}</span>
            </div>
          )}

          {isPaused && (
            <div className="pt-3 border-t border-hairline/8">
              <div className="flex items-center justify-between gap-3 p-3 rounded-lg bg-surfaceRaised">
                <div className="flex items-center gap-1.5 text-caption text-warning">
                  <span className="w-2 h-2 rounded-full bg-warning shrink-0" />
                  <span>{pauseReasonLabel[training!.status] ?? t('mlx.guardrail.pausedGenericLabel')}</span>
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
                  <span>{resumingTraining ? t('mlx.resumingBtn') : t('mlx.resumeTrainingBtn')}</span>
                </button>
              </div>
            </div>
          )}

          {isRunning && (
            <div className="pt-3 border-t border-hairline/8">
              <div className="flex items-center justify-between gap-3 p-3 rounded-lg bg-surfaceRaised">
                <div className="flex items-center gap-1.5 text-caption text-success">
                  <span className="w-2 h-2 rounded-full bg-success shrink-0" />
                  <span>{t('mlx.guardrail.runningLabel')}</span>
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
                  <span>{t('mlx.guardrail.pauseBtn')}</span>
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
};
