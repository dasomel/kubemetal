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

  const triggerIndex = useCallback(
    async (documentPath?: string) => {
      setIndexing(true);
      try {
        const res = await invoke<string>('index_rag_documents', {
          documentPath: documentPath || './data/docs',
        });
        await message(res || '문서 인덱싱을 시작했습니다.', { title: 'KubeMetal', kind: 'info' });
        await fetchStatus();
      } catch (err) {
        await message(`문서 인덱싱 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
      } finally {
        setIndexing(false);
      }
    },
    [fetchStatus],
  );

  const search = useCallback(async (query: string, limit: number = 5) => {
    if (!query.trim()) return;
    setSearching(true);
    try {
      const res = await invoke<RagSearchResult[]>('search_rag', { query, limit });
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
    triggerIndex,
    search,
    refresh: fetchStatus,
  };
}
