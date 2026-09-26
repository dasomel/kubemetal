import React, { useEffect, useRef, useState } from 'react';
import { Gauge, Loader2 } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from '../../i18n/i18nContext';
import type { GpuBenchmarkResult } from '../../types/ipc';

// src-tauri/src/services/gpu_benchmark.rs 의 "Cannot run GPU benchmark while ..." 에러
// 문구를 그대로 미러링한다 — 그쪽 문구가 바뀌면 이 접두사도 같이 바뀌어야 한다.
const workloadBlockedPrefix = 'Cannot run GPU benchmark while ';

export const GpuBenchmarkCard: React.FC = () => {
  const { t } = useTranslation();
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<GpuBenchmarkResult | null>(null);
  const [blockedMessage, setBlockedMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const runBenchmark = async () => {
    setRunning(true);
    setResult(null);
    setBlockedMessage(null);
    setError(null);

    try {
      const benchmarkResult = await invoke<GpuBenchmarkResult>('run_gpu_benchmark');
      if (!mountedRef.current) return;
      setResult(benchmarkResult);
    } catch (err) {
      if (!mountedRef.current) return;
      const message = String(err);
      if (message.startsWith(workloadBlockedPrefix)) {
        setBlockedMessage(message);
      } else {
        setError(message);
      }
    } finally {
      if (mountedRef.current) {
        setRunning(false);
      }
    }
  };

  return (
    <div className="animate-card-in rounded-xl bg-surface p-4 shadow-panel space-y-4">
      <div>
        <div className="text-label uppercase text-inkFaint mb-1">{t('mlx.gpuBenchmarkEyebrow')}</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Gauge className="w-4 h-4 text-primary" />
          <span>{t('mlx.gpuBenchmarkTitle')}</span>
        </h2>
        <p className="text-caption text-inkMuted mt-1">{t('mlx.gpuBenchmarkDescription')}</p>
      </div>

      <button
        type="button"
        onClick={runBenchmark}
        disabled={running}
        className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
      >
        {running ? <Loader2 className="w-4 h-4 animate-spin" /> : <Gauge className="w-4 h-4" />}
        <span>{running ? t('mlx.gpuBenchmarkRunning') : t('mlx.gpuBenchmarkRun')}</span>
      </button>

      {blockedMessage && (
        <div className="rounded-lg bg-warning/10 p-3 text-caption text-warning" role="status">
          <div className="font-medium mb-1">{t('mlx.gpuBenchmarkBlocked')}</div>
          <div>{blockedMessage}</div>
        </div>
      )}

      {error && (
        <div className="rounded-lg bg-danger/10 p-3 text-caption text-danger" role="alert">
          <div className="font-medium mb-1">{t('mlx.gpuBenchmarkError')}</div>
          <div>{error}</div>
        </div>
      )}

      {result && (
        <dl className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-5 gap-2">
          <div className="rounded-lg bg-surfaceRaised p-3">
            <dt className="text-caption text-inkMuted">{t('mlx.gpuBenchmarkGflops')}</dt>
            <dd className="text-bodyStrong text-ink mt-1">{result.gflops.toFixed(2)}</dd>
          </div>
          <div className="rounded-lg bg-surfaceRaised p-3">
            <dt className="text-caption text-inkMuted">{t('mlx.gpuBenchmarkMatrixDim')}</dt>
            <dd className="text-bodyStrong text-ink mt-1">{result.matrix_dim}</dd>
          </div>
          <div className="rounded-lg bg-surfaceRaised p-3">
            <dt className="text-caption text-inkMuted">{t('mlx.gpuBenchmarkIterations')}</dt>
            <dd className="text-bodyStrong text-ink mt-1">{result.iterations}</dd>
          </div>
          <div className="rounded-lg bg-surfaceRaised p-3">
            <dt className="text-caption text-inkMuted">{t('mlx.gpuBenchmarkPythonElapsed')}</dt>
            <dd className="text-bodyStrong text-ink mt-1">{result.python_elapsed_seconds.toFixed(2)}</dd>
          </div>
          <div className="rounded-lg bg-surfaceRaised p-3">
            <dt className="text-caption text-inkMuted">{t('mlx.gpuBenchmarkRustElapsed')}</dt>
            <dd className="text-bodyStrong text-ink mt-1">{result.rust_elapsed_seconds.toFixed(2)}</dd>
          </div>
        </dl>
      )}
    </div>
  );
};
