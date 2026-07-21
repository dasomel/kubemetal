import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { ClusterStatus } from '../types/ipc';

const PORT_FORWARD_TOTAL = 3;

export interface PortForwardStatus {
  active: number;
  total: number;
}

export function useColima() {
  const [status, setStatus] = useState<ClusterStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [portForwardStatus, setPortForwardStatus] = useState<PortForwardStatus | null>(null);

  const fetchStatus = useCallback(async () => {
    try {
      const res = await invoke<ClusterStatus>('get_cluster_status');
      setStatus(res);
    } catch (err) {
      console.error('클러스터 상태 로드 오류:', err);
    }
  }, []);

  const startCluster = useCallback(async (cpu: number, memory: number) => {
    setLoading(true);
    setActionMessage('Colima K8s 클러스터 구동 중...');
    try {
      const res = await invoke<string>('start_cluster', { cpu, memory });
      await message(res || 'Colima K8s 클러스터가 시작되었습니다.', { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(`클러스터 구동 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setLoading(false);
      setActionMessage(null);
    }
  }, [fetchStatus]);

  const stopCluster = useCallback(async () => {
    setLoading(true);
    setActionMessage('Colima K8s 클러스터 정지 중...');
    try {
      const res = await invoke<string>('stop_cluster');
      await message(res || 'Colima 클러스터가 정지되었습니다.', { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(`클러스터 정지 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setLoading(false);
      setActionMessage(null);
    }
  }, [fetchStatus]);

  const provisionStack = useCallback(async () => {
    setLoading(true);
    setActionMessage('MLflow, SeaweedFS 및 GPU 브리지 매니페스트 적용 중...');
    try {
      const res = await invoke<string>('provision_mlops_stack');
      await message(res || 'MLOps 스택 프로비저닝이 완료되었습니다.', { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(`스택 배포 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setLoading(false);
      setActionMessage(null);
    }
  }, [fetchStatus]);

  const startPortForward = useCallback(async () => {
    try {
      const res = await invoke<string>('start_port_forward');
      setPortForwardStatus({ active: PORT_FORWARD_TOTAL, total: PORT_FORWARD_TOTAL });
      await message(res || '포트포워딩이 시작되었습니다.', { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      // 백엔드가 부분 실패 시 "n(:port) 응답 없음"을 포트별로 나열한다 — 실패 개수를 세어
      // 활성 포트 수를 추정한다(전면 실패면 0/3).
      const errText = String(err);
      const failedCount = (errText.match(/응답 없음/g) || []).length;
      const active = failedCount > 0 ? Math.max(PORT_FORWARD_TOTAL - failedCount, 0) : 0;
      setPortForwardStatus({ active, total: PORT_FORWARD_TOTAL });
      await message(`포트포워딩 시작 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    }
  }, [fetchStatus]);

  const stopPortForward = useCallback(async () => {
    try {
      const res = await invoke<string>('stop_port_forward');
      setPortForwardStatus(null);
      await message(res || '포트포워딩이 정지되었습니다.', { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(`포트포워딩 정지 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    }
  }, [fetchStatus]);

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
