import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { SupportBundleResult } from '../types/ipc';

export function useSupportBundle() {
  const [result, setResult] = useState<SupportBundleResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const mounted = useRef(false);
  const pending = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  const create = useCallback(async () => {
    // D1 (local, #17): serialize clicks to avoid duplicate disk bundles; no queue,
    // retry after completion. Ignore UI updates after unmount; IPC still finishes.
    if (pending.current) return;
    pending.current = true;
    setLoading(true);
    setResult(null);
    setError(null);
    try {
      const response = await invoke<SupportBundleResult>('create_support_bundle');
      if (mounted.current) setResult(response);
    } catch (err) {
      if (mounted.current) setError(String(err));
    } finally {
      pending.current = false;
      if (mounted.current) setLoading(false);
    }
  }, []);

  return { result, error, loading, create };
}
