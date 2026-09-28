import React, { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ask } from '@tauri-apps/plugin-dialog';
import { AlertTriangle, Loader2, RefreshCw, ShieldAlert, Square } from 'lucide-react';
import type { OrphanScan, OrphanedProcessInfo } from '../../types/ipc';
import { useTranslation } from '../../i18n/i18nContext';

export const MlxOrphanedProcessesCard: React.FC = () => {
  const { t } = useTranslation();
  const [scan, setScan] = useState<OrphanScan | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [terminatingPid, setTerminatingPid] = useState<number | null>(null);
  const mountedRef = useRef(true);
  const requestIdRef = useRef(0);

  const refresh = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    if (mountedRef.current) {
      setLoading(true);
      setError(null);
    }
    try {
      const nextScan = await invoke<OrphanScan>('check_for_orphaned_mlx_processes');
      if (mountedRef.current && requestId === requestIdRef.current) {
        setScan(nextScan);
      }
    } catch (err) {
      if (mountedRef.current && requestId === requestIdRef.current) {
        setScan(null);
        setError(String(err));
      }
    } finally {
      if (mountedRef.current && requestId === requestIdRef.current) {
        setLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    void refresh();
    return () => {
      mountedRef.current = false;
    };
  }, [refresh]);

  const terminate = async (orphan: OrphanedProcessInfo) => {
    const confirmed = await ask(
      t('mlx.orphans.terminateConfirm', { pid: orphan.pid, cmdline: orphan.cmdline }),
      { title: t('mlx.orphans.terminateTitle'), kind: 'warning' },
    );
    if (!confirmed || !mountedRef.current) return;

    // 진행 중인 refresh의 늦은 응답이 종료 명령이 반환한 최신 scan을 덮지 못하게 한다.
    ++requestIdRef.current;
    setTerminatingPid(orphan.pid);
    setError(null);
    try {
      const nextScan = await invoke<OrphanScan>('terminate_orphaned_mlx_process', { pid: orphan.pid });
      if (mountedRef.current) {
        setScan(nextScan);
      }
    } catch (err) {
      if (mountedRef.current) {
        setError(String(err));
      }
    } finally {
      if (mountedRef.current) {
        setTerminatingPid(null);
      }
    }
  };

  const isEmpty = scan?.orphans.length === 0 && scan.unreadable.length === 0;

  return (
    <section className="rounded-xl bg-surface p-4 shadow-panel" aria-live="polite">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div>
          <div className="text-label uppercase text-inkFaint mb-1">MLX</div>
          <h2 className="text-heading text-ink flex items-center gap-2">
            <ShieldAlert className="w-4 h-4 text-warning" />
            <span>{t('mlx.orphans.title')}</span>
          </h2>
        </div>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={loading || terminatingPid !== null}
          className="px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-caption flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
          <span>{t('mlx.orphans.refresh')}</span>
        </button>
      </div>

      {loading && scan === null && !error && (
        <div className="flex items-center gap-2 py-4 text-body text-inkMuted">
          <Loader2 className="w-4 h-4 animate-spin text-primary" />
          <span>{t('mlx.orphans.loading')}</span>
        </div>
      )}

      {error && (
        <div className="mb-3 flex items-start gap-1.5 text-caption text-danger">
          <AlertTriangle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {scan && isEmpty && <p className="py-4 text-body text-inkMuted">{t('mlx.orphans.empty')}</p>}

      {scan && scan.orphans.length > 0 && (
        <div className="space-y-2">
          <p className="text-label uppercase text-inkFaint">{t('mlx.orphans.processesLabel')}</p>
          {scan.orphans.map((orphan) => (
            <div key={`${orphan.kind}-${orphan.pid}`} className="rounded-lg bg-surfaceRaised p-3 space-y-2 min-w-0">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="text-bodyStrong text-ink tabular-nums">
                  {t('mlx.orphans.pidKind', { pid: orphan.pid, kind: orphan.kind })}
                </span>
                <button
                  type="button"
                  onClick={() => void terminate(orphan)}
                  disabled={loading || terminatingPid !== null}
                  className="py-1.5 px-3 bg-dangerStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-caption rounded-md transition-all flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                >
                  {terminatingPid === orphan.pid ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Square className="w-3.5 h-3.5" />}
                  <span>{t('mlx.orphans.terminate')}</span>
                </button>
              </div>
              <code className="block break-all text-caption text-inkMuted">{orphan.cmdline}</code>
            </div>
          ))}
        </div>
      )}

      {scan && scan.unreadable.length > 0 && (
        <div className={scan.orphans.length > 0 ? 'mt-4 space-y-2' : 'space-y-2'}>
          <p className="text-label uppercase text-inkFaint">{t('mlx.orphans.unreadableLabel')}</p>
          {scan.unreadable.map((marker) => (
            <div key={marker.path} className="rounded-lg bg-surfaceRaised p-3 space-y-1 min-w-0">
              <code className="block break-all text-caption text-inkMuted">{marker.path}</code>
              <p className="break-all text-caption text-danger">{marker.error}</p>
            </div>
          ))}
        </div>
      )}
    </section>
  );
};
