import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Activity, Braces, Copy, RefreshCw, ShieldAlert, TerminalSquare } from 'lucide-react';

type RuntimeKind = 'omlx' | 'mlx-lm';

interface ManagedRuntimeProcess {
  runtime: RuntimeKind;
  endpoint: string;
}

interface RuntimeStatus {
  managed_process?: ManagedRuntimeProcess;
}

interface BridgeStatus {
  running: boolean;
  bind_address?: string;
}

interface ApiRouteProbe {
  name: string;
  path: string;
  supported?: boolean;
  status_code?: number;
  detail?: string;
}

interface ApiCapabilityProbeResult {
  runtime: RuntimeKind;
  endpoint: string;
  routes: ApiRouteProbe[];
}

interface ConnectionProfile {
  runtime: RuntimeKind;
  endpoint: string;
  openai_base_url: string;
  anthropic_base_url?: string;
  k3s_base_url?: string;
  api_key_placeholder?: string;
  environment_lines: string[];
  notes: string[];
}

interface DiagnosticFinding {
  code: string;
  severity: 'info' | 'warning' | 'critical';
  summary: string;
  evidence?: string;
}

interface Diagnostics {
  log_path: string;
  log_exists: boolean;
  metal_wired_limit_mb?: number;
  physical_memory_bytes?: number;
  findings: DiagnosticFinding[];
}

const buttonClass =
  'inline-flex items-center justify-center gap-1.5 px-3 py-2 rounded-md text-caption font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50 disabled:cursor-not-allowed';

const humanBytes = (value?: number) => {
  if (!value) return 'unavailable';
  const gib = value / 1024 / 1024 / 1024;
  return `${gib.toFixed(1)} GiB`;
};

export const LocalInferenceOpsCard: React.FC = () => {
  const [runtime, setRuntime] = useState<RuntimeKind>('omlx');
  const [endpoint, setEndpoint] = useState('http://127.0.0.1:8000');
  const [bridgeEndpoint, setBridgeEndpoint] = useState<string>();
  const [apiKey, setApiKey] = useState('');
  const [probe, setProbe] = useState<ApiCapabilityProbeResult>();
  const [profile, setProfile] = useState<ConnectionProfile>();
  const [diagnostics, setDiagnostics] = useState<Diagnostics>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [copied, setCopied] = useState(false);

  const refreshContext = useCallback(async () => {
    try {
      const [status, bridge] = await Promise.all([
        invoke<RuntimeStatus>('get_local_inference_status'),
        invoke<BridgeStatus>('get_local_inference_bridge_status'),
      ]);
      if (status.managed_process) {
        setRuntime(status.managed_process.runtime);
        setEndpoint(status.managed_process.endpoint);
      }
      setBridgeEndpoint(bridge.running && bridge.bind_address ? `http://${bridge.bind_address}` : undefined);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const refreshDiagnostics = useCallback(async () => {
    try {
      setDiagnostics(await invoke<Diagnostics>('get_local_inference_diagnostics'));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refreshContext();
    refreshDiagnostics();
  }, [refreshContext, refreshDiagnostics]);

  const runProbe = async () => {
    setBusy(true);
    setError(undefined);
    try {
      const result = await invoke<ApiCapabilityProbeResult>('probe_local_inference_api_capabilities', {
        request: { runtime, endpoint, api_key: apiKey || null },
      });
      setProbe(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const buildProfile = async () => {
    setBusy(true);
    setError(undefined);
    try {
      const result = await invoke<ConnectionProfile>('get_local_inference_connection_profile', {
        request: {
          runtime,
          endpoint,
          bridge_endpoint: bridgeEndpoint || null,
          api_key_configured: !!apiKey,
        },
      });
      setProfile(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const profileText = useMemo(() => profile?.environment_lines.join('\n') || '', [profile]);

  const copyProfile = async () => {
    if (!profileText) return;
    await navigator.clipboard.writeText(profileText);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel space-y-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="text-label uppercase text-inkFaint mb-1">Runtime Evidence</div>
          <h3 className="text-body font-medium text-ink flex items-center gap-2">
            <Activity className="w-4 h-4 text-primary" /> API, profile & diagnostics
          </h3>
          <p className="text-caption text-inkMuted mt-1">
            Capability probes distinguish route availability from successful model inference. Diagnostics never turn missing metrics into healthy zeroes.
          </p>
        </div>
        <button
          className={`${buttonClass} bg-surfaceRaised text-ink`}
          onClick={() => Promise.all([refreshContext(), refreshDiagnostics()])}
          disabled={busy}
        >
          <RefreshCw className="w-3.5 h-3.5" /> Refresh
        </button>
      </div>

      {error && <div className="rounded-md bg-danger/10 px-3 py-2 text-caption text-danger break-all">{error}</div>}

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
        <div className="rounded-lg bg-surfaceRaised p-3 space-y-3">
          <div className="flex items-center gap-2">
            <Braces className="w-4 h-4 text-primary" />
            <span className="text-body font-medium text-ink">Compatibility probe</span>
          </div>
          <div className="text-caption text-inkFaint font-mono break-all">{endpoint}</div>
          <label className="block text-caption text-inkMuted">
            Session API key
            <input
              className="mt-1 w-full px-3 py-2 rounded-md bg-surface text-ink border border-hairline/8"
              type="password"
              autoComplete="off"
              value={apiKey}
              onChange={(event) => setApiKey(event.target.value)}
              placeholder="Optional; never written into generated profile"
            />
          </label>
          <button className={`${buttonClass} bg-primaryStrong text-inverse`} onClick={runProbe} disabled={busy}>
            Probe API routes
          </button>
          <div className="space-y-1.5">
            {probe?.routes.map((route) => (
              <div key={route.path} className="flex items-center justify-between gap-3 text-caption">
                <span className="text-inkMuted">{route.name}</span>
                <span className={route.supported === true ? 'text-success' : route.supported === false ? 'text-danger' : 'text-inkFaint'}>
                  {route.supported === true ? `route present (${route.status_code})` : route.supported === false ? `not exposed (${route.status_code})` : 'unavailable'}
                </span>
              </div>
            ))}
          </div>
        </div>

        <div className="rounded-lg bg-surfaceRaised p-3 space-y-3">
          <div className="flex items-center gap-2">
            <TerminalSquare className="w-4 h-4 text-primary" />
            <span className="text-body font-medium text-ink">Client connection profile</span>
          </div>
          <button className={`${buttonClass} bg-primaryStrong text-inverse`} onClick={buildProfile} disabled={busy}>
            Generate profile
          </button>
          {profile && (
            <>
              <pre className="rounded-md bg-surface p-3 text-[11px] text-inkMuted overflow-x-auto whitespace-pre-wrap">{profileText}</pre>
              <button className={`${buttonClass} bg-surface text-ink`} onClick={copyProfile}>
                <Copy className="w-3.5 h-3.5" /> {copied ? 'Copied' : 'Copy environment'}
              </button>
              {profile.k3s_base_url && <div className="text-caption text-inkFaint">K3s private bridge: {profile.k3s_base_url}</div>}
              <div className="space-y-1">
                {profile.notes.map((note) => <div key={note} className="text-[11px] text-inkFaint">• {note}</div>)}
              </div>
            </>
          )}
        </div>
      </div>

      <div className="rounded-lg border border-hairline/8 p-3 space-y-2">
        <div className="flex items-center gap-2">
          <ShieldAlert className="w-4 h-4 text-primary" />
          <span className="text-body font-medium text-ink">Metal / runtime diagnostics</span>
        </div>
        <div className="flex flex-wrap gap-x-4 gap-y-1 text-caption text-inkFaint">
          <span>Physical memory: {humanBytes(diagnostics?.physical_memory_bytes)}</span>
          <span>Metal wired limit: {diagnostics?.metal_wired_limit_mb ? `${(diagnostics.metal_wired_limit_mb / 1024).toFixed(1)} GiB` : 'unavailable'}</span>
          <span className="font-mono break-all">Log: {diagnostics?.log_path || 'unavailable'}</span>
        </div>
        <div className="space-y-2">
          {diagnostics?.findings.map((finding) => (
            <div key={`${finding.code}:${finding.evidence || ''}`} className="rounded-md bg-surfaceRaised px-3 py-2">
              <div className="flex items-center gap-2 text-caption">
                <span className={finding.severity === 'critical' ? 'text-danger' : finding.severity === 'warning' ? 'text-warning' : 'text-inkMuted'}>
                  {finding.severity.toUpperCase()}
                </span>
                <span className="text-ink">{finding.summary}</span>
              </div>
              {finding.evidence && <div className="mt-1 text-[11px] text-inkFaint font-mono break-all">{finding.evidence}</div>}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};
