import { useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { ServiceAccess } from '../types/ipc';

export function useServiceAccess() {
  const [services, setServices] = useState<ServiceAccess[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const res = await invoke<ServiceAccess[]>('get_service_access');
      setServices(res);
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
      setLoaded(true);
    }
  }, []);

  return { services, loading, error, loaded, refresh };
}
