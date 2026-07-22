import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { DvcStatus } from '../types/ipc';

export function useDVC(active: boolean = false) {
  const [status, setStatus] = useState<DvcStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [initializing, setInitializing] = useState(false);
  const [creatingTag, setCreatingTag] = useState(false);

  const fetchStatus = useCallback(async () => {
    try {
      const res = await invoke<DvcStatus>('get_dvc_status');
      setStatus(res);
    } catch (err) {
      console.error('DVC 상태 로드 오류:', err);
    }
  }, []);

  const initDvc = useCallback(
    async (_remoteUrl?: string) => {
      setInitializing(true);
      try {
        const res = await invoke<string>('dvc_commit_dataset', {
          dataPath: null,
          bucketName: 'dvc-repo',
          commitMessage: 'Initialize DVC dataset repository',
        });
        await message(res || 'DVC 저장소가 초기화되었습니다.', { title: 'KubeMetal', kind: 'info' });
        await fetchStatus();
      } catch (err) {
        await message(`DVC 초기화 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
      } finally {
        setInitializing(false);
      }
    },
    [fetchStatus],
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
        await message(res || `DVC 데이터셋 버전 '${tag}' 커밋 완료`, { title: 'KubeMetal', kind: 'info' });
        await fetchStatus();
      } catch (err) {
        await message(`DVC 버전 생성 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
      } finally {
        setCreatingTag(false);
      }
    },
    [fetchStatus],
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
