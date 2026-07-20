import React, { useState, useEffect, useRef } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Rocket, Loader2, Play, Square, ArrowUpRight } from 'lucide-react';
import type { MlxServingState } from '../../types/ipc';

interface MlxServingCardProps {
  serving?: MlxServingState;
  adapterPath?: string;
  starting: boolean;
  stopping: boolean;
  onStart: (modelPath: string, port: number) => void;
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
  adapterPath,
  starting,
  stopping,
  onStart,
  onStop,
}) => {
  const [modelPath, setModelPath] = useState('');
  const [port, setPort] = useState(8080);
  const prefilledRef = useRef(false);

  // 파인튜닝 결과 어댑터 경로가 새로 생기면 비어 있는 입력을 한 번만 채운다.
  useEffect(() => {
    if (adapterPath && !prefilledRef.current && !modelPath) {
      setModelPath(adapterPath);
      prefilledRef.current = true;
    }
  }, [adapterPath, modelPath]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!modelPath) return;
    onStart(modelPath, port);
  };

  return (
    <div className="rounded-xl bg-surface p-6 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">Serving</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Rocket className="w-4 h-4 text-primary" />
          <span>모델 서빙</span>
        </h2>
      </div>

      <form onSubmit={handleSubmit} className="space-y-4 mb-5">
        <div>
          <label className={labelClass}>어댑터/모델 경로</label>
          <input
            type="text"
            value={modelPath}
            onChange={(e) => setModelPath(e.target.value)}
            disabled={!!serving}
            placeholder="예: /path/to/adapter 또는 모델 경로"
            className={inputClass}
          />
        </div>

        <div className="max-w-[160px]">
          <label className={labelClass}>포트</label>
          <input
            type="number"
            min={1}
            max={65535}
            value={port}
            onChange={(e) => setPort(Number(e.target.value))}
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
            <span>서빙 정지</span>
          </button>
        ) : (
          <button
            type="submit"
            disabled={starting || !modelPath}
            className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
          >
            {starting ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />}
            <span>서빙 시작</span>
          </button>
        )}
      </form>

      {serving && (
        <div className="pt-4 border-t border-hairline/8">
          <div className="flex items-center gap-1.5 text-caption text-inkMuted mb-3">
            <span className="w-2 h-2 rounded-full bg-success" />
            <span>서빙 중 (PID {serving.pid}) · {serving.model_path}</span>
          </div>
          <button
            type="button"
            onClick={() => openEndpoint(`http://localhost:${serving.port}/v1`)}
            className="px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 text-primary text-caption flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            {`http://localhost:${serving.port}/v1`}
            <ArrowUpRight className="w-3 h-3" />
          </button>
        </div>
      )}
    </div>
  );
};
