import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AlertTriangle, CheckCircle2, Clipboard, PackageCheck, RefreshCw, Terminal } from 'lucide-react';

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

interface RuntimeAdapterDescriptor {
  runtime: RuntimeKind;
  display_name: string;
  default_port: number;
  capabilities: RuntimeCapabilities;
  install_hint: string;
  install_commands: string[];
}

interface RuntimeProbe {
  runtime: RuntimeKind;
  installed: boolean;
  version?: string;
  executable?: string;
}

interface RuntimeExitEvidence {
  runtime: RuntimeKind;
  pid: number;
  endpoint: string;
  exit_code?: number;
  success: boolean;
  expected_stop: boolean;
  observed_at_epoch_ms: number;
  detail?: string;
}

interface LocalInferenceStatus {
  runtimes: RuntimeProbe[];
  last_exit?: RuntimeExitEvidence;
}

const buttonClass =
  'inline-flex items-center justify-center gap-1.5 px-3 py-2 rounded-md text-caption font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50 disabled:cursor-not-allowed';

export const LocalInferenceReadinessCard: React.FC = () => {
  const [adapters, setAdapters] = useState<RuntimeAdapterDescriptor[]>([]);
  const [status, setStatus] = useState<LocalInferenceStatus>();
  const [error, setError] = useState<string>();
  const [copied, setCopied] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [nextAdapters, nextStatus] = await Promise.all([
        invoke<RuntimeAdapterDescriptor[]>('list_local_inference_adapters'),
        invoke<LocalInferenceStatus>('get_local_inference_status'),
      ]);
      setAdapters(nextAdapters);
      setStatus(nextStatus);
      setError(undefined);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const omlx = useMemo(() => adapters.find((adapter) => adapter.runtime === 'omlx'), [adapters]);
  const omlxProbe = useMemo(() => status?.runtimes.find((probe) => probe.runtime === 'omlx'), [status]);
  const installText = omlx?.install_commands.join('\n') || '';

  const copyInstall = async () => {
    if (!installText) return;
    await navigator.clipboard.writeText(installText);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  const lastExit = status?.last_exit;
  const unexpected = !!lastExit && !lastExit.expected_stop;
  const observedAt = lastExit
    ? new Date(Number(lastExit.observed_at_epoch_ms)).toLocaleString()
    : undefined;

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel space-y-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="text-label uppercase text-inkFaint mb-1">Runtime Readiness</div>
          <h3 className="text-body font-medium text-ink flex items-center gap-2">
            <PackageCheck className="w-4 h-4 text-primary" /> Installation & exit evidence
          </h3>
          <p className="text-caption text-inkMuted mt-1">
            KubeMetal detects runtimes and shows official install guidance, but never installs or upgrades oMLX implicitly.
          </p>
        </div>
        <button className={`${buttonClass} bg-surfaceRaised text-ink`} onClick={refresh}>
          <RefreshCw className="w-3.5 h-3.5" /> Refresh
        </button>
      </div>

      {error && <div className="rounded-md bg-danger/10 px-3 py-2 text-caption text-danger break-all">{error}</div>}

      <div className="rounded-lg bg-surfaceRaised p-3 space-y-2">
        <div className="flex flex-wrap items-center gap-2">
          {omlxProbe?.installed ? (
            <CheckCircle2 className="w-4 h-4 text-success" />
          ) : (
            <Terminal className="w-4 h-4 text-primary" />
          )}
          <span className="text-body font-medium text-ink">oMLX</span>
          <span className={omlxProbe?.installed ? 'text-caption text-success' : 'text-caption text-inkFaint'}>
            {omlxProbe?.installed ? omlxProbe.version || 'Installed' : 'Not detected'}
          </span>
          {omlxProbe?.executable && <span className="text-[11px] text-inkFaint font-mono break-all">{omlxProbe.executable}</span>}
        </div>
        {omlx && <p className="text-caption text-inkMuted">{omlx.install_hint}</p>}
        {!omlxProbe?.installed && installText && (
          <>
            <pre className="rounded-md bg-surface p-3 text-[11px] text-inkMuted overflow-x-auto whitespace-pre-wrap">{installText}</pre>
            <button className={`${buttonClass} bg-surface text-ink`} onClick={copyInstall}>
              <Clipboard className="w-3.5 h-3.5" /> {copied ? 'Copied' : 'Copy official Homebrew commands'}
            </button>
          </>
        )}
      </div>

      <div className={`rounded-lg p-3 ${unexpected ? 'bg-danger/10' : 'bg-surfaceRaised'}`}>
        <div className="flex items-center gap-2">
          {unexpected ? <AlertTriangle className="w-4 h-4 text-danger" /> : <CheckCircle2 className="w-4 h-4 text-success" />}
          <span className="text-body font-medium text-ink">Last managed runtime exit</span>
        </div>
        {!lastExit ? (
          <div className="mt-2 text-caption text-inkFaint">No managed runtime exit has been observed in this app session.</div>
        ) : (
          <div className="mt-2 grid grid-cols-1 md:grid-cols-2 gap-x-4 gap-y-1 text-caption text-inkMuted">
            <span>{lastExit.expected_stop ? 'Expected stop' : 'Unexpected exit'}</span>
            <span>PID {lastExit.pid} · exit {lastExit.exit_code ?? 'signal/unknown'}</span>
            <span>{lastExit.endpoint}</span>
            <span>{observedAt}</span>
            {lastExit.detail && <span className="md:col-span-2 font-mono text-[11px] break-all">{lastExit.detail}</span>}
          </div>
        )}
      </div>
    </div>
  );
};
