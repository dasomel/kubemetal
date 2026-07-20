import { useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { RegisteredModel } from '../types/ipc';

export function useRegisteredModels() {
  const [models, setModels] = useState<RegisteredModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const res = await invoke<RegisteredModel[]>('list_registered_models');
      setModels(res);
      setError(null);
    } catch (err) {
      // 파이프라인 뷰의 보조 상태이므로 dialog 스팸 없이 상태로만 표기한다.
      setError(String(err));
    } finally {
      setLoading(false);
      setLoaded(true);
    }
  }, []);

  return { models, loading, error, loaded, refresh };
}
