import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { DvcStatus } from '../types/ipc';
import { useTranslation } from '../i18n/i18nContext';

export function useDVC(active: boolean = false) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<DvcStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [initializing, setInitializing] = useState(false);
  const [creatingTag, setCreatingTag] = useState(false);

  const fetchStatus = useCallback(async () => {
    try {
      const res = await invoke<DvcStatus>('get_dvc_status');
      setStatus(res);
    } catch (err) {
      console.error(t('dvc.err.statusLoad'), err);
    }
  }, [t]);

  const initDvc = useCallback(
    async (_remoteUrl?: string) => {
      setInitializing(true);
      try {
        const res = await invoke<string>('dvc_commit_dataset', {
          dataPath: null,
          bucketName: 'dvc-repo',
          commitMessage: 'Initialize DVC dataset repository',
        });
        await message(res || t('dvc.toast.initDone'), { title: 'KubeMetal', kind: 'info' });
        await fetchStatus();
      } catch (err) {
        await message(t('dvc.toast.initFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
      } finally {
        setInitializing(false);
      }
    },
    [fetchStatus, t],
  );

  const createTag = useCallback(
    async (tag: string, messageStr: string, datasetPath?: string) => {
      if (!tag.trim()) return;
      setCreatingTag(true);
      try {
        const res = await invoke<string>('dvc_commit_dataset', {
          dataPath: datasetPath || null,
          bucketName: 'dvc-repo',
          commitMessage: `[${tag}] ${messageStr}`,
        });
        await message(res || t('dvc.toast.tagDone', { tag }), { title: 'KubeMetal', kind: 'info' });
        await fetchStatus();
      } catch (err) {
        await message(t('dvc.toast.tagFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
      } finally {
        setCreatingTag(false);
      }
    },
    [fetchStatus, t],
  );

  useEffect(() => {
    setLoading(true);
    fetchStatus().finally(() => setLoading(false));
  }, [fetchStatus]);

  useEffect(() => {
    if (!active) return;
    const interval = setInterval(fetchStatus, 5000);
    return () => clearInterval(interval);
  }, [active, fetchStatus]);

  return {
    status,
    loading,
    initializing,
    creatingTag,
    initDvc,
    createTag,
    refresh: fetchStatus,
  };
}
