import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { RagIndexStatus, RagSearchResult } from '../types/ipc';

export function useRAG(active: boolean = false) {
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
      console.error('RAG 상태 로드 오류:', err);
    }
  }, []);

  const setupEnv = useCallback(async () => {
    try {
      const res = await invoke<string>('setup_rag_env');
      await message(res, { title: 'KubeMetal', kind: 'info' });
      await fetchStatus();
    } catch (err) {
      await message(`RAG 환경 설치 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    }
  }, [fetchStatus]);

  const triggerIndex = useCallback(
    async (docsPath?: string) => {
      setIndexing(true);
      try {
        const res = await invoke<{ status: string; collection: string; indexed_docs: number; total_chunks: number; db_path: string }>('index_documents', {
          docsPath: docsPath || 'docs',
        });
        await message(`문서 인덱싱 완료 (${res.indexed_docs} 문서, ${res.total_chunks} 청크)`, { title: 'KubeMetal', kind: 'info' });
        await fetchStatus();
      } catch (err) {
        await message(`문서 인덱싱 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
      } finally {
        setIndexing(false);
      }
    },
    [fetchStatus],
  );

  const search = useCallback(async (query: string, topK: number = 3) => {
    if (!query.trim()) return;
    setSearching(true);
    try {
      const res = await invoke<RagSearchResult[]>('query_rag', { query, topK });
      setSearchResults(res);
    } catch (err) {
      await message(`시맨틱 검색 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setSearching(false);
    }
  }, []);

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
