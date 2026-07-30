import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { RagIndexStatus, RagSearchResult } from '../types/ipc';
import { useTranslation } from '../i18n/i18nContext';

export function useRAG(active: boolean = false) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<RagIndexStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [indexing, setIndexing] = useState(false);
  const [searching, setSearching] = useState(false);
  const [searchResults, setSearchResults] = useState<RagSearchResult[]>([]);

  const fetchStatus = useCallback(async () => {
    try {
      const res = await invoke<RagIndexStatus>('get_rag_status');
      setStatus(res);
    } catch (err) {
      console.error(t('rag.err.statusLoad'), err);
    }
  }, [t]);

  const setupEnv = useCallback(async () => {
    try {
      const res = await invoke<string>('setup_rag_env');
      await message(res, { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(t('rag.toast.envSetupFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
    }
  }, [fetchStatus, t]);

  const triggerIndex = useCallback(
    async (docsPath?: string) => {
      setIndexing(true);
      try {
        const res = await invoke<{ status: string; collection: string; indexed_docs: number; total_chunks: number; db_path: string }>('index_documents', {
          docsPath: docsPath || 'docs',
        });
        await message(t('rag.toast.indexingDone', { docs: res.indexed_docs, chunks: res.total_chunks }), { title: 'KubeMetal', kind: 'info' });
        await fetchStatus();
      } catch (err) {
        await message(t('rag.toast.indexingFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
      } finally {
        setIndexing(false);
      }
    },
    [fetchStatus, t],
  );

  const search = useCallback(async (query: string, topK: number = 3) => {
    if (!query.trim()) return;
    setSearching(true);
    try {
      const res = await invoke<RagSearchResult[]>('query_rag', { query, topK });
      setSearchResults(res);
    } catch (err) {
      await message(t('rag.toast.searchFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
    } finally {
      setSearching(false);
    }
  }, [t]);

  useEffect(() => {
    setLoading(true);
    fetchStatus().finally(() => setLoading(false));
  }, [fetchStatus]);

  const shouldPoll = active && (indexing || status?.status === 'indexing');

  useEffect(() => {
    if (!shouldPoll) return;
    const interval = setInterval(fetchStatus, 3000);
    return () => clearInterval(interval);
  }, [shouldPoll, fetchStatus]);

  return {
    status,
    loading,
    indexing,
    searching,
    searchResults,
    setupEnv,
    triggerIndex,
    search,
    refresh: fetchStatus,
  };
}
