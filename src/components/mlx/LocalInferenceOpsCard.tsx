import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Activity, Braces, Copy, Database, RefreshCw, ShieldAlert, TerminalSquare, Trash2 } from 'lucide-react';

type RuntimeKind = 'omlx' | 'mlx-lm';

interface ManagedRuntimeProcess {
  runtime: RuntimeKind;
  endpoint: string;
}
interface RuntimeStatus { managed_process?: ManagedRuntimeProcess; }
interface BridgeStatus { running: boolean; bind_address?: string; }
interface ApiRouteProbe {
  name: string;
  path: string;
  advertised: boolean;
  route_present?: boolean;
  supported?: boolean;
  status_code?: number;
  detail?: string;
}
interface ApiCapabilityProbeResult { runtime: RuntimeKind; endpoint: string; routes: ApiRouteProbe[]; }
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
interface CacheCleanupResult {
  path: string;
  dry_run: boolean;
  max_age_hours: number;
  candidate_files: number;
  candidate_bytes: number;
  removed_files: number;
  removed_bytes: number;
  removed_directories: number;
  errors: string[];
}

const buttonClass =
  'inline-flex items-center justify-center gap-1.5 px-3 py-2 rounded-md text-caption font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50 disabled:cursor-not-allowed';
const inputClass =
  'w-full px-3 py-2 rounded-md bg-surface text-ink text-caption border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary';

const humanBytes = (value?: number) => {
  if (value === undefined) return 'unavailable';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) { size /= 1024; unit += 1; }
  return `${size.toFixed(unit >= 3 ? 1 : 0)} ${units[unit]}`;
};

export const LocalInferenceOpsCard: React.FC = () => {
  const [runtime, setRuntime] = useState<RuntimeKind>('omlx');
  const [endpoint, setEndpoint] = useState('http://127.0.0.1:8000');
  const [bridgeEndpoint, setBridgeEndpoint] = useState<string>();
  const [managedRunning, setManagedRunning] = useState(false);
  const [apiKey, setApiKey] = useState('');
  const [probe, setProbe] = useState<ApiCapabilityProbeResult>();
  const [profile, setProfile] = useState<ConnectionProfile>();
  const [diagnostics, setDiagnostics] = useState<Diagnostics>();
  const [cacheDir, setCacheDir] = useState('~/.omlx/cache');
  const [retentionHours, setRetentionHours] = useState(168);
  const [cleanupPreview, setCleanupPreview] = useState<CacheCleanupResult>();
  const [cleanupResult, setCleanupResult] = useState<CacheCleanupResult>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [copied, setCopied] = useState(false);

  const refreshContext = useCallback(async () => {
    try {
      const [status, bridge] = await Promise.all([
        invoke<RuntimeStatus>('get_local_inference_status'),
        invoke<BridgeStatus>('get_local_inference_bridge_status'),
      ]);
      setManagedRunning(!!status.managed_process);
      if (status.managed_process) {
        setRuntime(status.managed_process.runtime);
        setEndpoint(status.managed_process.endpoint);
      }
      setBridgeEndpoint(bridge.running && bridge.bind_address ? `http://${bridge.bind_address}` : undefined);
    } catch (e) { setError(String(e)); }
  }, []);

  const refreshDiagnostics = useCallback(async () => {
    try { setDiagnostics(await invoke<Diagnostics>('get_local_inference_diagnostics')); }
    catch (e) { setError(String(e)); }
  }, []);

  useEffect(() => { refreshContext(); refreshDiagnostics(); }, [refreshContext, refreshDiagnostics]);

  const runProbe = async () => {
    setBusy(true); setError(undefined);
    try {
      setProbe(await invoke<ApiCapabilityProbeResult>('probe_local_inference_api_capabilities', {
        request: { runtime, endpoint, api_key: apiKey || null },
      }));
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };

  const buildProfile = async () => {
    setBusy(true); setError(undefined);
    try {
      setProfile(await invoke<ConnectionProfile>('get_local_inference_connection_profile', {
        request: { runtime, endpoint, bridge_endpoint: bridgeEndpoint || null, api_key_configured: !!apiKey },
      }));
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };

  const runCleanup = async (dryRun: boolean) => {
    setBusy(true); setError(undefined);
    try {
      const result = await invoke<CacheCleanupResult>('cleanup_local_inference_cache', {
        request: { path: cacheDir, max_age_hours: Math.max(1, retentionHours), dry_run: dryRun },
      });
      if (dryRun) { setCleanupPreview(result); setCleanupResult(undefined); }
      else { setCleanupResult(result); setCleanupPreview(undefined); }
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };

  const profileText = useMemo(() => profile?.environment_lines.join('\n') || '', [profile]);
  const copyProfile = async () => {
    if (!profileText) return;
    await navigator.clipboard.writeText(profileText);
    setCopied(true); window.setTimeout(() => setCopied(false), 1500);
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
            Route evidence, secret-free client profiles, Metal diagnostics and guarded disposable-cache lifecycle.
          </p>
        </div>
        <button className={`${buttonClass} bg-surfaceRaised text-ink`} onClick={() => Promise.all([refreshContext(), refreshDiagnostics()])} disabled={busy}>
          <RefreshCw className="w-3.5 h-3.5" /> Refresh
        </button>
      </div>

      {error && <div className="rounded-md bg-danger/10 px-3 py-2 text-caption text-danger break-all">{error}</div>}

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
        <div className="rounded-lg bg-surfaceRaised p-3 space-y-3">
          <div className="flex items-center gap-2"><Braces className="w-4 h-4 text-primary" /><span className="text-body font-medium text-ink">Compatibility probe</span></div>
          <div className="text-caption text-inkFaint font-mono break-all">{endpoint}</div>
          <label className="block text-caption text-inkMuted">
            Session API key
            <input className={`${inputClass} mt-1`} type="password" autoComplete="off" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder="Optional; never written into generated profile" />
          </label>
          <button className={`${buttonClass} bg-primaryStrong text-inverse`} onClick={runProbe} disabled={busy}>Probe API routes</button>
          <div className="space-y-1.5">
            {probe?.routes.map((route) => (
              <div key={route.path} className="flex items-center justify-between gap-3 text-caption">
                <span className="text-inkMuted">{route.name}</span>
                <span className={route.supported === true ? 'text-success' : route.route_present === false ? 'text-danger' : 'text-inkFaint'}>
                  {route.route_present === true
                    ? `${route.advertised ? 'advertised' : 'unadvertised'} route (${route.status_code})`
                    : route.route_present === false ? `not exposed (${route.status_code})` : 'unavailable'}
                </span>
              </div>
            ))}
          </div>
        </div>

        <div className="rounded-lg bg-surfaceRaised p-3 space-y-3">
          <div className="flex items-center gap-2"><TerminalSquare className="w-4 h-4 text-primary" /><span className="text-body font-medium text-ink">Client connection profile</span></div>
          <button className={`${buttonClass} bg-primaryStrong text-inverse`} onClick={buildProfile} disabled={busy}>Generate profile</button>
          {profile && <>
            <pre className="rounded-md bg-surface p-3 text-[11px] text-inkMuted overflow-x-auto whitespace-pre-wrap">{profileText}</pre>
            <button className={`${buttonClass} bg-surface text-ink`} onClick={copyProfile}><Copy className="w-3.5 h-3.5" /> {copied ? 'Copied' : 'Copy environment'}</button>
            {profile.k3s_base_url && <div className="text-caption text-inkFaint">K3s private bridge: {profile.k3s_base_url}</div>}
            <div className="space-y-1">{profile.notes.map((note) => <div key={note} className="text-[11px] text-inkFaint">• {note}</div>)}</div>
          </>}
        </div>
      </div>

      <div className="rounded-lg border border-hairline/8 p-3 space-y-2">
        <div className="flex items-center gap-2"><ShieldAlert className="w-4 h-4 text-primary" /><span className="text-body font-medium text-ink">Metal / runtime diagnostics</span></div>
        <div className="flex flex-wrap gap-x-4 gap-y-1 text-caption text-inkFaint">
          <span>Physical memory: {humanBytes(diagnostics?.physical_memory_bytes)}</span>
          <span>Metal wired limit: {diagnostics?.metal_wired_limit_mb ? `${(diagnostics.metal_wired_limit_mb / 1024).toFixed(1)} GiB` : 'unavailable'}</span>
          <span className="font-mono break-all">Log: {diagnostics?.log_path || 'unavailable'}</span>
        </div>
        <div className="space-y-2">{diagnostics?.findings.map((finding) => (
          <div key={`${finding.code}:${finding.evidence || ''}`} className="rounded-md bg-surfaceRaised px-3 py-2">
            <div className="flex items-center gap-2 text-caption">
              <span className={finding.severity === 'critical' ? 'text-danger' : 'text-inkMuted'}>{finding.severity.toUpperCase()}</span>
              <span className="text-ink">{finding.summary}</span>
            </div>
            {finding.evidence && <div className="mt-1 text-[11px] text-inkFaint font-mono break-all">{finding.evidence}</div>}
          </div>
        ))}</div>
      </div>

      <div className="rounded-lg border border-hairline/8 p-3 space-y-3">
        <div className="flex items-center gap-2"><Database className="w-4 h-4 text-primary" /><span className="text-body font-medium text-ink">Disposable KV cache cleanup</span></div>
        <p className="text-caption text-inkMuted">Only explicit cache/KV-cache paths under HOME are eligible. Symlinks are never followed. Cleanup is disabled while the KubeMetal-managed runtime is running.</p>
        <div className="grid grid-cols-1 md:grid-cols-[1fr_160px] gap-2">
          <label className="text-caption text-inkMuted">Cache directory<input className={`${inputClass} mt-1`} value={cacheDir} onChange={(event) => { setCacheDir(event.target.value); setCleanupPreview(undefined); }} /></label>
          <label className="text-caption text-inkMuted">Retention hours<input className={`${inputClass} mt-1`} type="number" min={1} value={retentionHours} onChange={(event) => { setRetentionHours(Number(event.target.value)); setCleanupPreview(undefined); }} /></label>
        </div>
        <div className="flex flex-wrap gap-2 items-center">
          <button className={`${buttonClass} bg-surfaceRaised text-ink`} onClick={() => runCleanup(true)} disabled={busy || managedRunning || !cacheDir.trim()}>Preview cleanup</button>
          <button className={`${buttonClass} bg-danger text-inverse`} onClick={() => runCleanup(false)} disabled={busy || managedRunning || !cleanupPreview || cleanupPreview.candidate_files === 0}>
            <Trash2 className="w-3.5 h-3.5" /> Delete {cleanupPreview?.candidate_files || 0} expired files
          </button>
          {managedRunning && <span className="text-caption text-inkFaint">Stop managed oMLX before cleanup.</span>}
        </div>
        {cleanupPreview && <div className="text-caption text-inkMuted">Dry-run: {cleanupPreview.candidate_files} files · {humanBytes(cleanupPreview.candidate_bytes)} eligible; no files changed.</div>}
        {cleanupResult && <div className="text-caption text-inkMuted">Removed: {cleanupResult.removed_files} files · {humanBytes(cleanupResult.removed_bytes)} · {cleanupResult.removed_directories} empty directories.</div>}
      </div>
    </div>
  );
};
