import React, { useState } from 'react';
import { isTrainingActive } from '../../lib/trainingStatus';
import { Sliders, Loader2, Play, Square } from 'lucide-react';
import type { LocalModel, MlxTrainingState, FineTuneConfig, MlxRuntime } from '../../types/ipc';
import { useTranslation } from '../../i18n/i18nContext';

interface MlxFineTuneCardProps {
  localModels: LocalModel[];
  training?: MlxTrainingState;
  starting: boolean;
  killingPid: number | null;
  onStart: (config: FineTuneConfig) => void;
  onKill: (pid: number) => void;
}

const inputClass =
  'w-full px-3.5 py-2.5 rounded-md bg-surfaceRaised text-ink text-body placeholder:text-inkFaint border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary';
const labelClass = 'text-label uppercase text-inkFaint mb-1.5 block';

export const MlxFineTuneCard: React.FC<MlxFineTuneCardProps> = ({
  localModels,
  training,
  starting,
  killingPid,
  onStart,
  onKill,
}) => {
  const { t } = useTranslation();
  const [modelPath, setModelPath] = useState('');
  const [dataPath, setDataPath] = useState('~/.kubemetal/datasets/smoke');
  const [iters, setIters] = useState(100);
  const [batchSize, setBatchSize] = useState(1);
  const [learningRate, setLearningRate] = useState(1e-5);
  const [adapterName, setAdapterName] = useState('my-adapter');
  const [runtime, setRuntime] = useState<MlxRuntime>('mlx-lm');
  const [trainVision, setTrainVision] = useState(false);

  // 종료 상태를 배제하는 방식이 아니라 **비종료 상태를 열거**한다. 예전에는
  // `!== 'done' && !== 'error'`였는데 `killed`가 그 집합에 없어, 사용자가 중지를 누르고
  // 프로세스가 실제로 죽은 뒤에도 스피너가 영원히 "학습 중"을 돌렸다(실측 2026-08-21).
  // 백엔드 `should_record_exit`가 같은 이유로 같은 방향으로 고쳐졌다 — 상태 집합을
  // 배제로 정의하면 값이 늘어날 때마다 조용히 틀린다.
  const isTraining = !!training && isTrainingActive(training.status);
  const percent =
    training && training.total_iters > 0 ? Math.min((training.current_iter / training.total_iters) * 100, 100) : 0;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!modelPath) return;
    onStart({
      runtime,
      train_vision: runtime === 'mlx-vlm' && trainVision,
      model_path: modelPath,
      data_path: dataPath,
      iters,
      batch_size: batchSize,
      learning_rate: learningRate,
      adapter_name: adapterName,
    });
  };

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">MLX</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Sliders className="w-4 h-4 text-primary" />
          <span>{t('mlx.finetuneTitle')}</span>
        </h2>
      </div>

      {localModels.length === 0 ? (
        <div className="py-6 text-center text-inkMuted text-body mb-4">
          {t('orch.noLocalModelsNotice')}
        </div>
      ) : (
        <form onSubmit={handleSubmit} className="space-y-4 mb-4">
          <div>
            <label className={labelClass}>{t('mlx.selectBaseModel')}</label>
            <select
              value={modelPath}
              onChange={(e) => setModelPath(e.target.value)}
              disabled={isTraining}
              className={inputClass}
            >
              <option value="">{t('mlx.selectModelPlaceholder')}</option>
              {localModels.map((m) => (
                <option key={m.repo_id} value={m.path}>
                  {m.repo_id}
                </option>
              ))}
            </select>
          </div>

          <div>
            <label className={labelClass}>{t('mlx.runtimeLabel')}</label>
            <div className="flex gap-2" role="radiogroup" aria-label={t('mlx.runtimeLabel')}>
              {(['mlx-lm', 'mlx-vlm'] as const).map((rt) => (
                <button
                  key={rt}
                  type="button"
                  role="radio"
                  aria-checked={runtime === rt}
                  disabled={isTraining}
                  onClick={() => setRuntime(rt)}
                  className={`px-3 py-1.5 rounded-md text-caption transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary ${
                    runtime === rt
                      ? 'bg-primaryStrong text-inverse'
                      : 'bg-surfaceRaised text-inkMuted hover:brightness-95'
                  }`}
                >
                  {rt === 'mlx-lm' ? t('mlx.runtimeText') : t('mlx.runtimeVision')}
                </button>
              ))}
            </div>
            {runtime === 'mlx-vlm' && (
              <>
                <p className="text-caption text-inkFaint mt-1.5">{t('mlx.vlmDatasetHint')}</p>
                <div className="mt-2.5 flex flex-col gap-1">
                  <label className="flex items-center gap-2 cursor-pointer text-body text-ink">
                    <input
                      type="checkbox"
                      checked={trainVision}
                      onChange={(e) => setTrainVision(e.target.checked)}
                      disabled={isTraining}
                      className="w-4 h-4 rounded border-hairline accent-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50"
                    />
                    <span className="text-body text-ink">{t('mlx.trainVisionLabel')}</span>
                  </label>
                  <p className="text-caption text-inkFaint pl-6">{t('mlx.trainVisionHint')}</p>
                </div>
              </>
            )}
          </div>

          <div>
            <label className={labelClass}>{t('mlx.datasetPathLabel')}</label>
            <input
              type="text"
              value={dataPath}
              onChange={(e) => setDataPath(e.target.value)}
              disabled={isTraining}
              className={inputClass}
            />
          </div>

          <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
            <div>
              <label className={labelClass}>{t('mlx.itersLabel')}</label>
              <input
                type="number"
                min={1}
                value={iters}
                onChange={(e) => setIters(Number(e.target.value))}
                disabled={isTraining}
                className={inputClass}
              />
            </div>
            <div>
              <label className={labelClass}>{t('mlx.batchSizeLabel')}</label>
              <input
                type="number"
                min={1}
                value={batchSize}
                onChange={(e) => setBatchSize(Number(e.target.value))}
                disabled={isTraining}
                className={inputClass}
              />
            </div>
            <div>
              <label className={labelClass}>{t('mlx.learningRateLabel')}</label>
              <input
                type="number"
                step="0.00001"
                min={0}
                value={learningRate}
                onChange={(e) => setLearningRate(Number(e.target.value))}
                disabled={isTraining}
                className={inputClass}
              />
            </div>
            <div>
              <label className={labelClass}>{t('mlx.adapterNameLabel')}</label>
              <input
                type="text"
                value={adapterName}
                onChange={(e) => setAdapterName(e.target.value)}
                disabled={isTraining}
                className={inputClass}
              />
            </div>
          </div>

          {isTraining ? (
            <button
              type="button"
              onClick={() => onKill(training.pid)}
              disabled={killingPid === training.pid}
              className="py-2.5 px-4 bg-dangerStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
            >
              {killingPid === training.pid ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <Square className="w-4 h-4" />
              )}
              <span>{t('mlx.stopFinetuneBtn')}</span>
            </button>
          ) : (
            <button
              type="submit"
              disabled={starting || !modelPath}
              className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
            >
              {starting ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />}
              <span>{t('mlx.startFinetuneBtn')}</span>
            </button>
          )}
        </form>
      )}

      {training && (
        <div className="pt-4 border-t border-hairline/8">
          <div className="p-3 rounded-lg bg-surfaceRaised">
            <div className="flex items-center justify-between mb-2">
              <span className="text-bodyStrong text-ink">{t('mlx.trainingProgressTitle')}</span>
              <span className="text-caption text-inkFaint tabular-nums">
                {training.current_iter} / {training.total_iters} iter
                {training.last_loss != null ? ` · loss ${training.last_loss.toFixed(4)}` : ''}
              </span>
            </div>
            <div
              className="w-full h-1.5 bg-base rounded-full overflow-hidden mb-2"
              role="progressbar"
              aria-valuenow={percent}
              aria-valuemin={0}
              aria-valuemax={100}
            >
              <div
                className="h-full bg-primary transition-all duration-500 rounded-full"
                style={{ width: `${percent}%` }}
              />
            </div>
            {isTraining && (
              <div className="flex items-center gap-1.5 text-caption text-inkMuted">
                <Loader2 className="w-3.5 h-3.5 animate-spin text-primary" />
                <span>{t('mlx.finetune.trainingInProgress', { pid: training.pid })}</span>
              </div>
            )}
            {training.status === 'done' && (
              <div className="flex items-center gap-1.5 text-caption text-inkMuted">
                <span className="w-2 h-2 rounded-full bg-success" />
                <span>{t('pipeline.trainDone')}{training.adapter_path ? ` · ${training.adapter_path}` : ''}</span>
              </div>
            )}
            {training.status === 'killed' && (
              <div className="flex items-center gap-1.5 text-caption text-inkMuted">
                <span className="w-2 h-2 rounded-full bg-inkFaint" />
                <span>{t('mlx.finetune.trainingKilled')}</span>
              </div>
            )}
            {training.status === 'error' && (
              <div className="flex items-center gap-1.5 text-caption text-danger">
                <span className="w-2 h-2 rounded-full bg-danger" />
                <span>{t('dataIngest.statusError')}: {training.error ?? t('modelhub.dl.unknownError')}</span>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
};
