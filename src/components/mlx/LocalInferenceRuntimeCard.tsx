import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Activity,
  Box,
  Database,
  Loader2,
  Play,
  RefreshCw,
  Server,
  Square,
} from 'lucide-react';

type RuntimeKind = 'omlx' | 'mlx-lm';

interface RuntimeCapabilities {
  openai_chat: boolean;
  openai_responses: boolean;
  anthropic_messages: boolean;
  embeddings: boolean;
  rerank: boolean;
  multi_model: boolean;
  model_load_unload: boolean;
  model_pinning: boolean;
  model_ttl: boolean;
  continuous_batching: boolean;
  tiered_kv_cache: boolean;
  mcp: boolean;
}

interface RuntimeProbe {
  runtime: RuntimeKind;
  installed: boolean;
  executable?: string;
  version?: string;
  endpoint: string;
  managed_by_kubemetal: boolean;
  capabilities: RuntimeCapabilities;
  detail?: string;
}

interface ManagedRuntimeProcess {
  runtime: RuntimeKind;
  pid: number;
  endpoint: string;
  running: boolean;
}

interface LocalInferenceStatus {
  preferred_runtime?: RuntimeKind;
  runtimes: RuntimeProbe[];
  managed_process?: ManagedRuntimeProcess;
}

interface RuntimeModel {
  id: string;
  display_name?: string;
  model_path?: string;
  loaded: boolean;
  loading: boolean;
  pinned?: boolean;
  is_default?: boolean;
  model_type?: string;
  engine_type?: string;
  estimated_size?: number;
  actual_size?: number;
  alias?: string;
  ttl_seconds?: number;
}

interface RuntimeLiveStatus {
  runtime: RuntimeKind;
  endpoint: string;
  reachable: boolean;
  healthy: boolean;
  health_status_code?: number;
  health_detail?: string;
  models: RuntimeModel[];
}

interface RuntimeActionResult {
  ok: boolean;
  status_code?: number;
  detail?: string;
}

interface CacheInspection {
  path: string;
  exists: boolean;
  bytes: number;
  files: number;
  directories: number;
  partial: boolean;
  errors: string[];
}

const inputClass =
  'w-full px-3 py-2 rounded-md bg-surfaceRaised text-ink text-caption border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary';
const buttonClass =
  'inline-flex items-center justify-center gap-1.5 px-3 py-2 rounded-md text-caption font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50 disabled:cursor-not-allowed';

const humanBytes = (value?: number) => {
  if (!value) return value === 0 ? '0 B' : '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size.toFixed(unit > 1 ? 1 : 0)} ${units[unit]}`;
};

const capabilityLabels: Array<[keyof RuntimeCapabilities, string]> = [
  ['multi_model', 'Multi-model'],
  ['continuous_batching', 'Continuous batching'],
  ['tiered_kv_cache', 'RAM + SSD KV cache'],
  ['openai_chat', 'OpenAI API'],
  ['anthropic_messages', 'Anthropic API'],
  ['embeddings', 'Embeddings'],
  ['rerank', 'Rerank'],
  ['mcp', 'MCP'],
];

export const LocalInferenceRuntimeCard: React.FC = () => {
  const [status, setStatus] = useState<LocalInferenceStatus>();
  const [live, setLive] = useState<RuntimeLiveStatus>();
  const [cacheEvidence, setCacheEvidence] = useState<CacheInspection>();
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<string>();
  const [port, setPort] = useState(8000);
  const [modelDir, setModelDir] = useState('~/.omlx/models');
  const [apiKey, setApiKey] = useState('');
  const [cacheEnabled, setCacheEnabled] = useState(true);
  const [ssdCacheDir, setSsdCacheDir] = useState('~/.omlx/cache');
  const [ssdCacheSize, setSsdCacheSize] = useState('20GB');
  const [hotCacheSize, setHotCacheSize] = useState('4GB');
  const [maxConcurrency, setMaxConcurrency] = useState(4);
  const [memoryGuard, setMemoryGuard] = useState(true);
  const [modelTtl, setModelTtl] = useState<Record<string, string>>({});
  const [modelAlias, setModelAlias] = useState<Record<string, string>>({});

  const endpoint = status?.managed_process?.endpoint ?? `http://127.0.0.1:${port}`;
  const omlx = useMemo(() => status?.runtimes.find((runtime) => runtime.runtime === 'omlx'), [status]);

  const refresh = useCallback(async () => {
    try {
      const next = await invoke<LocalInferenceStatus>('get_local_inference_status');
      setStatus(next);
      const target = next.managed_process?.endpoint ?? `http://127.0.0.1:${port}`;
      const runtime: RuntimeKind = next.managed_process?.runtime ?? 'omlx';
      const nextLive = await invoke<RuntimeLiveStatus>('probe_local_inference_live', {
        request: {
          runtime,
          endpoint: target,
          api_key: apiKey || null,
        },
      });
      setLive(nextLive);
      setError(undefined);
    } catch (e) {
      setError(String(e));
    }
  }, [apiKey, port]);

  const inspectCache = useCallback(async () => {
    if (!cacheEnabled || !ssdCacheDir.trim()) {
      setCacheEvidence(undefined);
      return;
    }
    try {
      const evidence = await invoke<CacheInspection>('inspect_local_inference_cache', {
        request: { path: ssdCacheDir.trim() },
      });
      setCacheEvidence(evidence);
    } catch (e) {
      setError(String(e));
    }
  }, [cacheEnabled, ssdCacheDir]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    const timer = window.setInterval(refresh, 5000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const run = async (name: string, action: () => Promise<unknown>) => {
    setBusy(name);
    setError(undefined);
    try {
      await action();
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(undefined);
    }
  };

  const runtimeConfig = () => ({
    runtime: 'omlx' as const,
    port,
    model_dir: modelDir || null,
    pinned_models: [],
    cache_enabled: cacheEnabled,
    paged_ssd_cache_dir: cacheEnabled && ssdCacheDir ? ssdCacheDir : null,
    paged_ssd_cache_max_size: cacheEnabled && ssdCacheSize ? ssdCacheSize : null,
    hot_cache_max_size: cacheEnabled && hotCacheSize ? hotCacheSize : null,
    max_concurrent_requests: maxConcurrency,
    memory_guard: memoryGuard,
  });

  const start = () =>
    run('start', () =>
      invoke('start_local_inference_runtime', {
        config: runtimeConfig(),
      }),
    );

  const stop = () => run('stop', () => invoke('stop_local_inference_runtime'));

  const restart = () =>
    run('restart', async () => {
      if (status?.managed_process) {
        await invoke('stop_local_inference_runtime');
      }
      await invoke('start_local_inference_runtime', { config: runtimeConfig() });
    });

  const modelAction = (model: RuntimeModel, action: 'load' | 'unload') =>
    run(`${action}:${model.id}`, async () => {
      const result = await invoke<RuntimeActionResult>(`${action}_omlx_model`, {
        request: {
          endpoint,
          model_id: model.id,
          api_key: apiKey || null,
        },
      });
      if (!result.ok) throw new Error(result.detail || `${action} failed (${result.status_code})`);
    });

  const updateModel = (model: RuntimeModel, patch: Record<string, unknown>, op: string) =>
    run(`${op}:${model.id}`, async () => {
      const result = await invoke<RuntimeActionResult>('set_omlx_model_settings_sparse', {
        request: {
          endpoint,
          model_id: model.id,
          patch,
          api_key: apiKey || null,
        },
      });
      if (!result.ok) throw new Error(result.detail || `settings update failed (${result.status_code})`);
    });

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel space-y-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="text-label uppercase text-inkFaint mb-1">Local AI Runtime</div>
          <h2 className="text-heading text-ink flex items-center gap-2">
            <Server className="w-4 h-4 text-primary" />
            oMLX Multi-model Runtime
          </h2>
          <p className="text-caption text-inkMuted mt-1">
            KubeMetal owns only processes it starts. Existing oMLX services are discovered but never stopped automatically.
          </p>
        </div>
        <button className={`${buttonClass} bg-surfaceRaised text-ink`} onClick={refresh} disabled={!!busy}>
          <RefreshCw className={`w-3.5 h-3.5 ${busy ? 'animate-spin' : ''}`} /> Refresh
        </button>
      </div>

      {error && <div className="rounded-md bg-danger/10 px-3 py-2 text-caption text-danger break-all">{error}</div>}

      <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
        {status?.runtimes.map((runtime) => (
          <div key={runtime.runtime} className="rounded-lg bg-surfaceRaised p-3">
            <div className="flex items-center justify-between gap-2">
              <div className="font-medium text-body text-ink">{runtime.runtime === 'omlx' ? 'oMLX' : 'mlx-lm'}</div>
              <span className={`text-caption ${runtime.installed ? 'text-success' : 'text-inkFaint'}`}>
                {runtime.installed ? runtime.version || 'Installed' : 'Not detected'}
              </span>
            </div>
            <div className="mt-2 flex flex-wrap gap-1">
              {capabilityLabels
                .filter(([key]) => runtime.capabilities[key])
                .map(([, label]) => (
                  <span key={label} className="px-2 py-0.5 rounded bg-surface text-inkMuted text-[11px]">
                    {label}
                  </span>
                ))}
            </div>
          </div>
        ))}
      </div>

      <div className="rounded-lg border border-hairline/8 p-3 space-y-3">
        <div className="flex flex-wrap items-center gap-3 text-caption">
          <span className={`inline-flex items-center gap-1.5 ${live?.healthy ? 'text-success' : 'text-inkMuted'}`}>
            <Activity className="w-3.5 h-3.5" />
            {live?.healthy ? 'Healthy' : live?.reachable ? `Reachable (${live.health_status_code})` : 'Stopped / unreachable'}
          </span>
          <span className="font-mono text-inkMuted">{endpoint}</span>
          {status?.managed_process && (
            <span className="text-inkFaint">KubeMetal-managed PID {status.managed_process.pid}</span>
          )}
        </div>

        <div className="grid grid-cols-1 md:grid-cols-4 gap-2">
          <label className="text-caption text-inkMuted">
            Port
            <input className={`${inputClass} mt-1`} type="number" min={1} max={65535} value={port} onChange={(e) => setPort(Number(e.target.value))} disabled={!!status?.managed_process} />
          </label>
          <label className="text-caption text-inkMuted md:col-span-2">
            Model directory
            <input className={`${inputClass} mt-1`} value={modelDir} onChange={(e) => setModelDir(e.target.value)} disabled={!!status?.managed_process} />
          </label>
          <label className="text-caption text-inkMuted">
            Max concurrency
            <input className={`${inputClass} mt-1`} type="number" min={1} max={1024} value={maxConcurrency} onChange={(e) => setMaxConcurrency(Number(e.target.value))} disabled={!!status?.managed_process} />
          </label>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-2">
          <label className="text-caption text-inkMuted">
            SSD cache directory
            <input className={`${inputClass} mt-1`} value={ssdCacheDir} onChange={(e) => setSsdCacheDir(e.target.value)} disabled={!cacheEnabled || !!status?.managed_process} />
          </label>
          <label className="text-caption text-inkMuted">
            SSD cache max
            <input className={`${inputClass} mt-1`} value={ssdCacheSize} onChange={(e) => setSsdCacheSize(e.target.value)} disabled={!cacheEnabled || !!status?.managed_process} />
          </label>
          <label className="text-caption text-inkMuted">
            Hot RAM cache max
            <input className={`${inputClass} mt-1`} value={hotCacheSize} onChange={(e) => setHotCacheSize(e.target.value)} disabled={!cacheEnabled || !!status?.managed_process} />
          </label>
        </div>

        <div className="flex flex-wrap gap-4 text-caption text-inkMuted">
          <label className="inline-flex items-center gap-2">
            <input type="checkbox" checked={cacheEnabled} onChange={(e) => setCacheEnabled(e.target.checked)} disabled={!!status?.managed_process} />
            Tiered KV cache
          </label>
          <label className="inline-flex items-center gap-2">
            <input type="checkbox" checked={memoryGuard} onChange={(e) => setMemoryGuard(e.target.checked)} disabled={!!status?.managed_process} />
            Memory guard
          </label>
          <button className={`${buttonClass} bg-surfaceRaised text-ink`} onClick={inspectCache} disabled={!cacheEnabled || !ssdCacheDir.trim()}>
            <Database className="w-3.5 h-3.5" /> Inspect cache
          </button>
          {cacheEvidence && (
            <span className="self-center text-inkFaint">
              {cacheEvidence.exists ? `${humanBytes(cacheEvidence.bytes)} · ${cacheEvidence.files} files` : 'Cache not created yet'}
              {cacheEvidence.partial ? ' · partial' : ''}
            </span>
          )}
        </div>

        <label className="block text-caption text-inkMuted max-w-xl">
          oMLX API key (session only; never persisted)
          <input className={`${inputClass} mt-1`} type="password" autoComplete="off" value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder="Optional" />
        </label>

        <div className="flex gap-2">
          {status?.managed_process ? (
            <>
              <button className={`${buttonClass} bg-danger text-inverse`} onClick={stop} disabled={!!busy}>
                {busy === 'stop' ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Square className="w-3.5 h-3.5" />} Stop managed oMLX
              </button>
              <button className={`${buttonClass} bg-surfaceRaised text-ink`} onClick={restart} disabled={!!busy}>
                {busy === 'restart' ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <RefreshCw className="w-3.5 h-3.5" />} Restart
              </button>
            </>
          ) : (
            <button className={`${buttonClass} bg-primaryStrong text-inverse`} onClick={start} disabled={!!busy || !omlx?.installed}>
              {busy === 'start' ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Play className="w-3.5 h-3.5" />} Start oMLX
            </button>
          )}
        </div>
      </div>

      <div>
        <div className="flex items-center gap-2 mb-2">
          <Box className="w-4 h-4 text-primary" />
          <h3 className="text-body font-medium text-ink">Model Pool</h3>
          <span className="text-caption text-inkFaint">{live?.models.length || 0} discovered</span>
        </div>
        {!live?.reachable ? (
          <div className="rounded-lg bg-surfaceRaised p-3 text-caption text-inkMuted">Start or connect to a loopback oMLX endpoint to inspect the model pool.</div>
        ) : live.models.length === 0 ? (
          <div className="rounded-lg bg-surfaceRaised p-3 text-caption text-inkMuted">No models returned. If admin auth is enabled, provide the API key above.</div>
        ) : (
          <div className="space-y-2">
            {live.models.map((model) => {
              const ttlValue = modelTtl[model.id] ?? String(model.ttl_seconds ?? '');
              const aliasValue = modelAlias[model.id] ?? model.alias ?? '';
              const actionBusy = busy?.endsWith(`:${model.id}`);
              return (
                <div key={model.id} className="rounded-lg bg-surfaceRaised p-3 grid grid-cols-1 lg:grid-cols-[1fr_auto] gap-3">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-medium text-body text-ink truncate">{model.display_name || model.id}</span>
                      <span className={`text-[11px] px-2 py-0.5 rounded ${model.loaded ? 'bg-success/10 text-success' : 'bg-surface text-inkFaint'}`}>
                        {model.loading ? 'Loading' : model.loaded ? 'Loaded' : 'Unloaded'}
                      </span>
                      {model.pinned && <span className="text-[11px] px-2 py-0.5 rounded bg-primary/10 text-primary">Pinned</span>}
                      {model.is_default && <span className="text-[11px] px-2 py-0.5 rounded bg-surface text-inkMuted">Default</span>}
                    </div>
                    <div className="mt-1 text-caption text-inkFaint flex flex-wrap gap-x-3">
                      <span>{model.model_type || model.engine_type || 'model'}</span>
                      <span>{humanBytes(model.actual_size || model.estimated_size)}</span>
                      {model.alias && <span>alias: {model.alias}</span>}
                    </div>
                  </div>
                  <div className="flex flex-wrap items-end gap-2">
                    <label className="text-[11px] text-inkFaint w-28">
                      Alias
                      <input className={`${inputClass} mt-1 py-1.5`} value={aliasValue} onChange={(e) => setModelAlias((current) => ({ ...current, [model.id]: e.target.value }))} />
                    </label>
                    <button className={`${buttonClass} bg-surface text-ink`} disabled={!!actionBusy || !aliasValue.trim()} onClick={() => updateModel(model, { model_alias: aliasValue.trim() }, 'alias')}>
                      Alias
                    </button>
                    <label className="text-[11px] text-inkFaint w-24">
                      TTL (sec)
                      <input className={`${inputClass} mt-1 py-1.5`} value={ttlValue} onChange={(e) => setModelTtl((current) => ({ ...current, [model.id]: e.target.value }))} />
                    </label>
                    <button className={`${buttonClass} bg-surface text-ink`} disabled={!!actionBusy || !ttlValue.trim() || !Number.isFinite(Number(ttlValue)) || Number(ttlValue) < 0} onClick={() => updateModel(model, { ttl_seconds: Number(ttlValue) }, 'ttl')}>
                      TTL
                    </button>
                    <button className={`${buttonClass} bg-surface text-ink`} disabled={!!actionBusy} onClick={() => updateModel(model, { is_pinned: !model.pinned }, 'pin')}>
                      {model.pinned ? 'Unpin' : 'Pin'}
                    </button>
                    <button className={`${buttonClass} bg-surface text-ink`} disabled={!!actionBusy || !!model.is_default} onClick={() => updateModel(model, { is_default: true }, 'default')}>
                      {model.is_default ? 'Default' : 'Set default'}
                    </button>
                    <button className={`${buttonClass} ${model.loaded ? 'bg-danger/10 text-danger' : 'bg-primaryStrong text-inverse'}`} disabled={!!actionBusy || model.loading} onClick={() => modelAction(model, model.loaded ? 'unload' : 'load')}>
                      {actionBusy ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : model.loaded ? <Square className="w-3.5 h-3.5" /> : <Play className="w-3.5 h-3.5" />}
                      {model.loaded ? 'Unload' : 'Load'}
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      <div className="flex items-start gap-2 rounded-lg bg-surfaceRaised p-3 text-caption text-inkMuted">
        <Database className="w-4 h-4 mt-0.5 shrink-0" />
        <span>
          Tiered KV cache remains a disposable performance artifact. KubeMetal configures and observes it, while oMLX owns scheduling, prefix sharing, eviction, and cache implementation.
        </span>
      </div>
    </div>
  );
};
