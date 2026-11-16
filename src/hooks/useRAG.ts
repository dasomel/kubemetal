import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { RagIndexResult, RagIndexStatus, RagQueryResponse, RagSearchResult, RetrievalMode } from '../types/ipc';
import { useTranslation } from '../i18n/i18nContext';

export function indexResultMessage(
  result: RagIndexResult,
  t: (key: string, params?: Record<string, string | number>) => string,
) {
  const done = t('rag.toast.indexingDone', { docs: result.indexed_docs, chunks: result.total_chunks });
  return result.partial
    ? `${done}\n${t('rag.toast.indexingPartial', { count: result.skipped_count })}\n${result.skipped_files.map(file => `${file.source}: ${file.reason}`).join('\n')}`
    : done;
}

export function useRAG(active: boolean = false) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<RagIndexStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [indexing, setIndexing] = useState(false);
  const [searching, setSearching] = useState(false);
  const [searchResults, setSearchResults] = useState<RagSearchResult[]>([]);
  const [routing, setRouting] = useState<{ resolved_mode: RetrievalMode; rule: string } | null>(null);

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
        const res = await invoke<RagIndexResult>('index_documents', {
          docsPath: docsPath || 'docs',
        });
        await message(indexResultMessage(res, t), { title: 'KubeMetal', kind: res.partial ? 'warning' : 'info' });
        await fetchStatus();
      } catch (err) {
        await message(t('rag.toast.indexingFailed', { error: String(err) }), { title: 'KubeMetal', kind: 'error' });
      } finally {
        setIndexing(false);
      }
    },
    [fetchStatus, t],
  );

  const search = useCallback(async (query: string, topK: number = 3, mode: RetrievalMode = 'auto') => {
    if (!query.trim()) return;
    setSearching(true);
    try {
      const res = await invoke<RagQueryResponse>('query_rag', { query, topK, mode });
      setSearchResults(res.results);
      setRouting({ resolved_mode: res.resolved_mode, rule: res.rule });
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
    routing,
    setupEnv,
    triggerIndex,
    search,
    refresh: fetchStatus,
  };
}
