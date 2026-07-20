import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { SystemMetrics } from '../types/ipc';

export function useMetrics(intervalMs = 1000) {
  const [metrics, setMetrics] = useState<SystemMetrics | null>(null);

  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const res = await invoke<SystemMetrics>('get_system_metrics');
        if (alive) {
          setMetrics(res);
        }
      } catch (err) {
        console.error('시스템 메트릭 로드 오류:', err);
      }
    };

    tick();
    const id = setInterval(tick, intervalMs);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [intervalMs]);

  return metrics;
}
