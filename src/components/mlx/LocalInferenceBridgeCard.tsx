import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Cable, Loader2, Play, RefreshCw, Square } from 'lucide-react';
import type { DeployTarget } from '../../types/ipc';

interface BridgeStatus {
  running: boolean;
  bind_address?: string;
  target_address?: string;
}

const inputClass =
  'w-full px-3 py-2 rounded-md bg-surfaceRaised text-ink text-caption border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary';
const buttonClass =
  'inline-flex items-center justify-center gap-1.5 px-3 py-2 rounded-md text-caption font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50 disabled:cursor-not-allowed';

export const LocalInferenceBridgeCard: React.FC<{ defaultTargetPort?: number }> = ({ defaultTargetPort = 8000 }) => {
  const [status, setStatus] = useState<BridgeStatus>();
  const [bindHost, setBindHost] = useState('192.168.64.1');
  const [bindPort, setBindPort] = useState(18000);
  const [targetPort, setTargetPort] = useState(defaultTargetPort);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke<BridgeStatus>('get_local_inference_bridge_status'));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    // Prefill from the deploy target's D10-verified bridge address when one exists, instead
    // of leaving the hardcoded placeholder — an unverified/keep_base target (e.g. colima's
    // DNS-name bridge) has no stored numeric address, so the placeholder is left as-is.
    invoke<DeployTarget>('get_deploy_target')
      .then((target) => {
        if (target.bridge.kind === 'verified') {
          setBindHost(target.bridge.host);
        }
      })
      .catch(() => {
        // Best-effort prefill only — leave the placeholder if the target can't be read.
      });
  }, [refresh]);

  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(undefined);
    try {
      await action();
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel space-y-3">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="text-label uppercase text-inkFaint mb-1">K3s → Host Bridge</div>
          <h3 className="text-body font-medium text-ink flex items-center gap-2">
            <Cable className="w-4 h-4 text-primary" /> Private inference relay
          </h3>
          <p className="text-caption text-inkMuted mt-1">
            Keeps the inference runtime on 127.0.0.1 and exposes a KubeMetal-owned TCP relay only on an explicitly selected private host IP.
          </p>
        </div>
        <button className={`${buttonClass} bg-surfaceRaised text-ink`} onClick={refresh} disabled={busy}>
          <RefreshCw className="w-3.5 h-3.5" /> Refresh
        </button>
      </div>

      {error && <div className="rounded-md bg-danger/10 px-3 py-2 text-caption text-danger break-all">{error}</div>}

      <div className="grid grid-cols-1 md:grid-cols-3 gap-2">
        <label className="text-caption text-inkMuted">
          Private host IP
          <input className={`${inputClass} mt-1`} value={bindHost} onChange={(e) => setBindHost(e.target.value)} disabled={status?.running} />
        </label>
        <label className="text-caption text-inkMuted">
          Bridge port
          <input className={`${inputClass} mt-1`} type="number" min={1} max={65535} value={bindPort} onChange={(e) => setBindPort(Number(e.target.value))} disabled={status?.running} />
        </label>
        <label className="text-caption text-inkMuted">
          Loopback target port
          <input className={`${inputClass} mt-1`} type="number" min={1} max={65535} value={targetPort} onChange={(e) => setTargetPort(Number(e.target.value))} disabled={status?.running} />
        </label>
      </div>

      <div className="flex flex-wrap items-center gap-3">
        {status?.running ? (
          <button className={`${buttonClass} bg-danger text-inverse`} disabled={busy} onClick={() => run(() => invoke('stop_local_inference_bridge'))}>
            {busy ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Square className="w-3.5 h-3.5" />} Stop bridge
          </button>
        ) : (
          <button
            className={`${buttonClass} bg-primaryStrong text-inverse`}
            disabled={busy}
            onClick={() =>
              run(() =>
                invoke('start_local_inference_bridge', {
                  config: {
                    bind_host: bindHost,
                    bind_port: bindPort,
                    target_port: targetPort,
                  },
                }),
              )
            }
          >
            {busy ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Play className="w-3.5 h-3.5" />} Start bridge
          </button>
        )}
        <span className="text-caption text-inkFaint">
          {status?.running ? `${status.bind_address} → ${status.target_address}` : 'Public and wildcard bind addresses are rejected.'}
        </span>
      </div>
    </div>
  );
};
