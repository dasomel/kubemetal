import { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { ServingStatus } from '../types/ipc';
import { useTranslation } from '../i18n/i18nContext';

export function useServingRollback(serving: ServingStatus | null | undefined, onRefreshStatus?: () => Promise<void>) {
  const { t } = useTranslation();
  const [lastKnownGoodServing, setLastKnownGoodServing] = useState<ServingStatus | null>(null);
  const [revertingServing, setRevertingServing] = useState(false);
  // This fetch runs on every status poll; alert once per failure streak, not once per tick.
  const loadErrorShownRef = useRef(false);

  const fetchLastKnownGoodServing = useCallback(async () => {
    try {
      const res = await invoke<ServingStatus | null>('get_last_known_good_serving');
      setLastKnownGoodServing(res);
      loadErrorShownRef.current = false;
    } catch (err) {
      console.error(t('mlx.err.lastKnownGoodLoad'), err);
      setLastKnownGoodServing(null);
      if (!loadErrorShownRef.current) {
        loadErrorShownRef.current = true;
        await message(t('mlx.err.lastKnownGoodLoad'), { title: 'KubeMetal', kind: 'error' });
      }
    }
  }, [t]);

  const revertServing = useCallback(async () => {
    setRevertingServing(true);
    try {
      if (!lastKnownGoodServing) return;
      const res = await invoke<string>('revert_to_last_serving', {
        expectedModelPath: lastKnownGoodServing.model_path,
        expectedAdapterPath: lastKnownGoodServing.adapter_path ?? null,
        expectedRuntime: lastKnownGoodServing.runtime,
      });
      await message(res || t('mlx.toast.servingReverted'), { title: 'KubeMetal', kind: 'info' });
      if (onRefreshStatus) {
        await onRefreshStatus();
      }
      await fetchLastKnownGoodServing();
    } catch (err) {
      await message(t('mlx.toast.servingRevertFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
      if (onRefreshStatus) {
        await onRefreshStatus();
      }
      // A mismatch refusal means the saved config moved; show the current one right away.
      await fetchLastKnownGoodServing();
    } finally {
      setRevertingServing(false);
    }
  }, [onRefreshStatus, fetchLastKnownGoodServing, lastKnownGoodServing, t]);

  useEffect(() => {
    void serving;
    fetchLastKnownGoodServing();
  }, [serving, fetchLastKnownGoodServing]);

  return {
    lastKnownGoodServing,
    fetchLastKnownGoodServing,
    revertingServing,
    revertServing,
  };
}
