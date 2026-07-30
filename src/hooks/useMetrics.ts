import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { SystemMetrics } from '../types/ipc';
import { useTranslation } from '../i18n/i18nContext';

export function useMetrics(intervalMs = 1000) {
  const { t } = useTranslation();
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
        console.error(t('metrics.err.load'), err);
      }
    };

    tick();
    const id = setInterval(tick, intervalMs);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [intervalMs, t]);

  return metrics;
}
