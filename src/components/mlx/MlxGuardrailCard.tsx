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
  const { t, language } = useTranslation();
  const pausingTraining = propPausing ?? hookPausing;
  const onPause = propOnPause ?? hookOnPause;
  const setThermalPause = onSetThermalPause ?? hookSetThermal;

  const levelLabel: Record<string, string> = {
    normal: language === 'en' ? 'Normal' : '정상',
    warn: language === 'en' ? 'Warning' : '경고',
    critical: language === 'en' ? 'Critical' : '위험',
    unknown: language === 'en' ? 'Unknown' : '알 수 없음',
  };

  const pauseReasonLabel: Record<string, string> = {
    paused: language === 'en' ? 'Manually paused' : '수동으로 일시정지됨',
    paused_memory_pressure: language === 'en' ? 'Auto paused due to memory pressure' : '메모리 압력 감지로 자동 일시정지됨',
    paused_battery: language === 'en' ? 'Auto paused due to battery power' : '배터리 구동 감지로 자동 일시정지됨',
    paused_thermal: language === 'en' ? 'Auto paused due to thermal pressure' : '발열 상승 감지로 자동 일시정지됨',
  };

  // 발열은 메모리 압력과 척도가 다르다 — fair는 부하가 걸린 정상 상태라 경고로 칠하지 않는다.
  const thermalDot: Record<string, string> = {
    nominal: 'bg-success',
    fair: 'bg-success',
    serious: 'bg-warning',
    critical: 'bg-danger',
  };
  const thermalLabel: Record<string, string> = {
    nominal: language === 'en' ? 'Nominal' : '정상',
    fair: language === 'en' ? 'Fair' : '보통',
    serious: language === 'en' ? 'Serious' : '높음',
    critical: language === 'en' ? 'Critical' : '위험',
  };
  const thermal = guardrailStatus?.thermal_state ?? null;

  const isPaused = !!training && training.status.startsWith('paused');
  const isRunning = !!training && (training.status === 'running' || training.status === 'training');
  const level = guardrailStatus?.memory_pressure_level ?? 'unknown';

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
          {language === 'en' ? 'Loading guardrail status...' : '가드레일 상태를 불러오는 중...'}
        </div>
      ) : (
        <div className="space-y-4">
          <div className="grid grid-cols-3 gap-2">
            <div className="p-3 rounded-lg bg-surfaceRaised">
              <div className={tileLabelClass}>{language === 'en' ? 'Memory Pressure' : '메모리 압력'}</div>
              <div className="flex items-center gap-1.5 text-bodyStrong text-ink">
                <span className={`w-2 h-2 rounded-full shrink-0 ${LEVEL_DOT[level] ?? LEVEL_DOT.unknown}`} />
                <span>{levelLabel[level] ?? level}</span>
              </div>
            </div>
            <div className="p-3 rounded-lg bg-surfaceRaised">
              <div className={tileLabelClass}>{language === 'en' ? 'Power' : '전원'}</div>
              <div className="flex items-center gap-1.5 text-bodyStrong text-ink">
                {guardrailStatus.on_battery ? (
                  <Battery className="w-3.5 h-3.5 text-warning" />
                ) : (
                  <BatteryCharging className="w-3.5 h-3.5 text-success" />
                )}
                <span>
                  {guardrailStatus.on_battery
                    ? (language === 'en' ? 'Battery Power' : '배터리 구동')
                    : (language === 'en' ? 'AC Power' : 'AC 전원')}
                </span>
              </div>
            </div>
            <div className="p-3 rounded-lg bg-surfaceRaised">
              <div className={tileLabelClass}>{language === 'en' ? 'Thermal' : '발열'}</div>
              <div className="flex items-center gap-1.5 text-bodyStrong text-ink">
                {/* 값이 없으면 "정상"으로 채우지 않고 미상으로 둔다(D22). */}
                <span className={`w-2 h-2 rounded-full shrink-0 ${thermal ? thermalDot[thermal] : 'bg-inkFaint'}`} />
                <span>
                  {thermal
                    ? thermalLabel[thermal]
                    : language === 'en' ? 'Unavailable' : '조회 불가'}
                </span>
              </div>
            </div>
          </div>

          <label className="flex items-center justify-between gap-3 py-1">
            <span className="text-body text-ink">
              {language === 'en'
                ? 'Pause training when thermal pressure is serious'
                : '발열이 높음 이상이면 학습 일시정지'}
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
                {language === 'en'
                  ? 'caffeinate active — Prevents system sleep during training.'
                  : 'caffeinate 활성 — 학습 중 슬립 진입을 방지합니다.'}
              </span>
            </div>
          )}

          {isPaused && (
            <div className="pt-3 border-t border-hairline/8">
              <div className="flex items-center justify-between gap-3 p-3 rounded-lg bg-surfaceRaised">
                <div className="flex items-center gap-1.5 text-caption text-warning">
                  <span className="w-2 h-2 rounded-full bg-warning shrink-0" />
                  <span>{pauseReasonLabel[training!.status] ?? (language === 'en' ? 'Training is paused.' : '학습이 일시정지되었습니다.')}</span>
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
                  <span>{language === 'en' ? 'Training in progress.' : '학습이 진행 중입니다.'}</span>
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
                  <span>{language === 'en' ? 'Pause' : '일시정지'}</span>
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
};
