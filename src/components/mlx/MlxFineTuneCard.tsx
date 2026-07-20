import React, { useState } from 'react';
import { Sliders, Loader2, Play, Square } from 'lucide-react';
import type { LocalModel, MlxTrainingState, FineTuneConfig } from '../../types/ipc';

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
  const [modelPath, setModelPath] = useState('');
  const [dataPath, setDataPath] = useState('~/.kubemetal/datasets/smoke');
  const [iters, setIters] = useState(100);
  const [batchSize, setBatchSize] = useState(1);
  const [learningRate, setLearningRate] = useState(1e-5);
  const [adapterName, setAdapterName] = useState('my-adapter');

  const isTraining = !!training && training.status !== 'done' && training.status !== 'error';
  const percent =
    training && training.total_iters > 0 ? Math.min((training.current_iter / training.total_iters) * 100, 100) : 0;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!modelPath) return;
    onStart({
      model_path: modelPath,
      data_path: dataPath,
      iters,
      batch_size: batchSize,
      learning_rate: learningRate,
      adapter_name: adapterName,
    });
  };

  return (
    <div className="rounded-xl bg-surface p-6 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">MLX-LM</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Sliders className="w-4 h-4 text-primary" />
          <span>파인튜닝</span>
        </h2>
      </div>

      {localModels.length === 0 ? (
        <div className="py-6 text-center text-inkMuted text-body mb-5">
          로컬 모델이 없습니다. 모델 허브 탭에서 모델을 먼저 다운로드하세요.
        </div>
      ) : (
        <form onSubmit={handleSubmit} className="space-y-4 mb-5">
          <div>
            <label className={labelClass}>로컬 모델</label>
            <select
              value={modelPath}
              onChange={(e) => setModelPath(e.target.value)}
              disabled={isTraining}
              className={inputClass}
            >
              <option value="">모델 선택...</option>
              {localModels.map((m) => (
                <option key={m.repo_id} value={m.path}>
                  {m.repo_id}
                </option>
              ))}
            </select>
          </div>

          <div>
            <label className={labelClass}>데이터셋 경로</label>
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
              <label className={labelClass}>Iters</label>
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
              <label className={labelClass}>Batch Size</label>
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
              <label className={labelClass}>Learning Rate</label>
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
              <label className={labelClass}>Adapter Name</label>
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
              <span>학습 중지</span>
            </button>
          ) : (
            <button
              type="submit"
              disabled={starting || !modelPath}
              className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
            >
              {starting ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />}
              <span>파인튜닝 시작</span>
            </button>
          )}
        </form>
      )}

      {training && (
        <div className="pt-4 border-t border-hairline/8">
          <div className="p-4 rounded-lg bg-surfaceRaised">
            <div className="flex items-center justify-between mb-2">
              <span className="text-bodyStrong text-ink">진행 상황</span>
              <span className="text-caption text-inkFaint tabular-nums">
                {training.current_iter} / {training.total_iters} iter
                {training.last_loss !== undefined ? ` · loss ${training.last_loss.toFixed(4)}` : ''}
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
                <span>학습 중 (PID {training.pid})</span>
              </div>
            )}
            {training.status === 'done' && (
              <div className="flex items-center gap-1.5 text-caption text-inkMuted">
                <span className="w-2 h-2 rounded-full bg-success" />
                <span>완료{training.adapter_path ? ` · ${training.adapter_path}` : ''}</span>
              </div>
            )}
            {training.status === 'error' && (
              <div className="flex items-center gap-1.5 text-caption text-danger">
                <span className="w-2 h-2 rounded-full bg-danger" />
                <span>오류: {training.error ?? '알 수 없는 오류'}</span>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
};
