import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { ClusterStatus } from '../types/ipc';
import { useTranslation } from '../i18n/i18nContext';

// src-tauri/src/commands/port_forward.rs 의 JOBS 배열 길이와 일치해야 한다 —
// 어긋나면 부분 실패 카운트(active/total)가 실제 포워드 수와 다르게 보인다.
const PORT_FORWARD_TOTAL = 5;

export interface PortForwardStatus {
  active: number;
  total: number;
}

export function useColima() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<ClusterStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [portForwardStatus, setPortForwardStatus] = useState<PortForwardStatus | null>(null);

  const fetchStatus = useCallback(async () => {
    try {
      const res = await invoke<ClusterStatus>('get_cluster_status');
      setStatus(res);
    } catch (err) {
      console.error(t('cluster.err.statusLoad'), err);
    }
  }, [t]);

  const startCluster = useCallback(async (cpu: number, memory: number) => {
    setLoading(true);
    setActionMessage(t('cluster.toast.starting'));
    try {
      const res = await invoke<string>('start_cluster', { cpu, memory });
      await message(res || t('cluster.toast.started'), { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(t('cluster.toast.startFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
    } finally {
      setLoading(false);
      setActionMessage(null);
    }
  }, [fetchStatus, t]);

  const stopCluster = useCallback(async () => {
    setLoading(true);
    setActionMessage(t('cluster.toast.stopping'));
    try {
      const res = await invoke<string>('stop_cluster');
      await message(res || t('cluster.toast.stopped'), { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(t('cluster.toast.stopFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
    } finally {
      setLoading(false);
      setActionMessage(null);
    }
  }, [fetchStatus, t]);

  const provisionStack = useCallback(async () => {
    setLoading(true);
    setActionMessage(t('cluster.toast.provisioning'));
    try {
      const res = await invoke<string>('provision_mlops_stack');
      await message(res || t('cluster.toast.provisioned'), { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(t('cluster.toast.provisionFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
    } finally {
      setLoading(false);
      setActionMessage(null);
    }
  }, [fetchStatus, t]);

  const startPortForward = useCallback(async () => {
    try {
      const res = await invoke<string>('start_port_forward');
      setPortForwardStatus({ active: PORT_FORWARD_TOTAL, total: PORT_FORWARD_TOTAL });
      await message(res || t('cluster.toast.forwardStarted'), { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      // 백엔드가 부분 실패 시 "name(:port) not responding"을 포트별로 나열한다(D31로 영문화) —
      // 실패 개수를 세어 활성 포트 수를 추정한다(전면 실패면 0/3).
      // NOTE(i18n): 백엔드 port_forward.rs가 실제로 보내는 문자열과 매칭하는 리터럴이라 번역 키가 아니다.
      const errText = String(err);
      const failedCount = (errText.match(/not responding/g) || []).length;
      const active = failedCount > 0 ? Math.max(PORT_FORWARD_TOTAL - failedCount, 0) : 0;
      setPortForwardStatus({ active, total: PORT_FORWARD_TOTAL });
      await message(t('cluster.toast.forwardStartFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
    }
  }, [fetchStatus, t]);

  const stopPortForward = useCallback(async () => {
    try {
      const res = await invoke<string>('stop_port_forward');
      setPortForwardStatus(null);
      await message(res || t('cluster.toast.forwardStopped'), { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(t('cluster.toast.forwardStopFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
    }
  }, [fetchStatus, t]);

  useEffect(() => {
    fetchStatus();
    const interval = setInterval(fetchStatus, 5000);
    return () => clearInterval(interval);
  }, [fetchStatus]);

  return {
    status,
    loading,
    actionMessage,
    portForwardStatus,
    startCluster,
    stopCluster,
    provisionStack,
    startPortForward,
    stopPortForward,
    refresh: fetchStatus,
  };
}
