import React, { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Rocket, Loader2, Play, Square, ArrowUpRight, AlertTriangle } from 'lucide-react';
import type { LocalModel, MlxServingState, ServingRuntime } from '../../types/ipc';
import { ModelChatPlayground } from './ModelChatPlayground';
import { useTranslation } from '../../i18n/i18nContext';

interface MlxServingCardProps {
  serving?: MlxServingState;
  lastServingError?: string;
  localModels: LocalModel[];
  adapterPathHint?: string;
  starting: boolean;
  stopping: boolean;
  onStart: (
    modelPath: string,
    adapterPath: string | undefined,
    port: number,
    runtime?: ServingRuntime,
  ) => void;
  /** mlx-vlm 미설치면 VLM 선택지를 잠근다 — 스폰 후 ModuleNotFoundError로 죽는 것보다 낫다. */
  vlmAvailable: boolean;
  onStop: () => void;
}

const inputClass =
  'w-full px-3.5 py-2.5 rounded-md bg-surfaceRaised text-ink text-body placeholder:text-inkFaint border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary';
const labelClass = 'text-label uppercase text-inkFaint mb-1.5 block';

const openEndpoint = (url: string) => {
  openUrl(url).catch(() => window.open(url, '_blank'));
};

export const MlxServingCard: React.FC<MlxServingCardProps> = ({
  serving,
  lastServingError,
  localModels,
  adapterPathHint,
  starting,
  stopping,
  onStart,
  onStop,
  vlmAvailable,
}) => {
  const { t, language } = useTranslation();
  const [modelPath, setModelPath] = useState('');
  const [adapterPath, setAdapterPath] = useState('');
  const [port, setPort] = useState(8080);
  const [runtime, setRuntime] = useState<ServingRuntime>('mlx-lm');
  const prefilledRef = useRef(false);
  const portEditedRef = useRef(false);

  // 파인튜닝 결과 어댑터 경로가 새로 생기면 비어 있는 어댑터 입력을 한 번만 채운다.
  // (베이스 모델 칸은 항상 로컬 베이스 모델 선택을 유지한다.)
  useEffect(() => {
    if (adapterPathHint && !prefilledRef.current && !adapterPath) {
      setAdapterPath(adapterPathHint);
      prefilledRef.current = true;
    }
  }, [adapterPathHint, adapterPath]);

  // 마운트 시 1회 빈 포트를 제안받아 초기값으로 설정한다. 사용자가 그 사이 포트를
  // 직접 변경했다면(portEditedRef) 존중하고 덮어쓰지 않는다. 실패 시 기존 기본값(8080) 유지.
  useEffect(() => {
    let cancelled = false;
    invoke<number>('suggest_serving_port')
      .then((suggested) => {
        if (!cancelled && !portEditedRef.current) {
          setPort(suggested);
        }
      })
      .catch(() => {
        // 제안 실패 — 기본값 8080을 그대로 유지한다.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!modelPath) return;
    onStart(modelPath, adapterPath || undefined, port, runtime);
  };

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">Serving</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Rocket className="w-4 h-4 text-primary" />
          <span>{t('mlx.servingTitle')}</span>
        </h2>
      </div>

      {lastServingError && (
        <div className="mb-4 flex items-start gap-1.5 text-caption text-danger">
          <AlertTriangle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
          <span>{lastServingError}</span>
        </div>
      )}

      <form onSubmit={handleSubmit} className="space-y-4 mb-4">
        <div>
          <label className={labelClass}>{t('mlx.selectBaseModel')}</label>
          <select
            value={modelPath}
            onChange={(e) => setModelPath(e.target.value)}
            disabled={!!serving}
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
          <label className={labelClass}>{t('mlx.adapterPathOptional')}</label>
          <input
            type="text"
            value={adapterPath}
            onChange={(e) => setAdapterPath(e.target.value)}
            disabled={!!serving}
            placeholder={language === 'en' ? 'e.g. ~/.kubemetal/adapters/my-adapter' : '예: ~/.kubemetal/adapters/my-adapter'}
            className={inputClass}
          />
        </div>

        <div>
          <label className={labelClass}>{t('mlx.runtimeLabel')}</label>
          <div className="flex gap-2" role="radiogroup" aria-label={t('mlx.runtimeLabel')}>
            {(['mlx-lm', 'mlx-vlm'] as const).map((rt) => {
              const locked = rt === 'mlx-vlm' && !vlmAvailable;
              return (
                <button
                  key={rt}
                  type="button"
                  role="radio"
                  aria-checked={runtime === rt}
                  disabled={!!serving || locked}
                  title={locked ? t('mlx.runtimeVlmLocked') : undefined}
                  onClick={() => setRuntime(rt)}
                  className={`px-3 py-1.5 rounded-md text-caption transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary ${
                    runtime === rt
                      ? 'bg-primaryStrong text-inverse'
                      : 'bg-surfaceRaised text-inkMuted hover:brightness-95'
                  } ${locked ? 'opacity-50 cursor-not-allowed' : ''}`}
                >
                  {rt === 'mlx-lm' ? t('mlx.runtimeText') : t('mlx.runtimeVision')}
                </button>
              );
            })}
          </div>
        </div>

        <div className="max-w-[160px]">
          <label className={labelClass}>{t('mlx.portLabel')}</label>
          <input
            type="number"
            min={1}
            max={65535}
            value={port}
            onChange={(e) => {
              portEditedRef.current = true;
              setPort(Number(e.target.value));
            }}
            disabled={!!serving}
            className={inputClass}
          />
        </div>

        {serving ? (
          <button
            type="button"
            onClick={onStop}
            disabled={stopping}
            className="py-2.5 px-4 bg-dangerStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
          >
            {stopping ? <Loader2 className="w-4 h-4 animate-spin" /> : <Square className="w-4 h-4" />}
            <span>{t('mlx.stopServingBtn')}</span>
          </button>
        ) : (
          <button
            type="submit"
            disabled={starting || !modelPath}
            className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
          >
            {starting ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />}
            <span>{t('mlx.startServingBtn')}</span>
          </button>
        )}
      </form>

      {serving && (
        <div className="pt-4 border-t border-hairline/8 space-y-4 min-w-0 overflow-hidden">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2">
            <div className="flex items-center gap-1.5 text-caption text-inkMuted">
              <span className="w-2 h-2 rounded-full bg-success" />
              <span>
                {language === 'en' ? `Serving active (PID ${serving.pid}) · ` : `서빙 중 (PID ${serving.pid}) · `}
                {serving.model_path}
                {serving.adapter_path ? (language === 'en' ? ` · adapter ${serving.adapter_path}` : ` · 어댑터 ${serving.adapter_path}`) : ''}
              </span>
            </div>
            <button
              type="button"
              onClick={() => openEndpoint(`http://127.0.0.1:${serving.port}/v1/models`)}
              className="px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 text-primary text-caption flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary self-start sm:self-auto"
            >
              {`http://127.0.0.1:${serving.port}/v1`}
              <ArrowUpRight className="w-3 h-3" />
            </button>
          </div>

          <ModelChatPlayground
            port={serving.port}
            modelPath={serving.model_path}
            adapterPath={serving.adapter_path}
            runtime={serving.runtime}
          />
        </div>
      )}
    </div>
  );
};

