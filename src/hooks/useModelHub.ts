import { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { HfModel, DownloadStatus, LocalModel } from '../types/ipc';

export function useModelHub() {
  const [searchResults, setSearchResults] = useState<HfModel[]>([]);
  const [searching, setSearching] = useState(false);
  const [popularModels, setPopularModels] = useState<HfModel[]>([]);
  const [loadingPopular, setLoadingPopular] = useState(false);
  const [downloads, setDownloads] = useState<DownloadStatus[]>([]);
  const [downloadingIds, setDownloadingIds] = useState<Set<string>>(new Set());
  const [localModels, setLocalModels] = useState<LocalModel[]>([]);
  const [uploadingIds, setUploadingIds] = useState<Set<string>>(new Set());
  const [uploadedIds, setUploadedIds] = useState<Set<string>>(new Set());
  const [registeringIds, setRegisteringIds] = useState<Set<string>>(new Set());
  const [registeredIds, setRegisteredIds] = useState<Set<string>>(new Set());

  const prevDownloadsRef = useRef<DownloadStatus[]>([]);

  const fetchLocalModels = useCallback(async () => {
    try {
      const res = await invoke<LocalModel[]>('list_local_models');
      setLocalModels(res);
    } catch (err) {
      console.error('로컬 모델 목록 로드 오류:', err);
    }
  }, []);

  const fetchDownloads = useCallback(async () => {
    try {
      const res = await invoke<DownloadStatus[]>('get_model_downloads');
      setDownloads(res);
    } catch (err) {
      console.error('다운로드 상태 로드 오류:', err);
    }
  }, []);

  const search = useCallback(async (query: string, limit = 20, author?: string) => {
    if (!query.trim()) return;
    setSearching(true);
    try {
      const res = await invoke<HfModel[]>('search_hf_models', { query, limit, author });
      setSearchResults(res);
    } catch (err) {
      await message(`모델 검색 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setSearching(false);
    }
  }, []);

  // 모델 허브 탭 진입 시 검색 없이도 mlx-community의 인기 모델을 보여준다.
  const loadPopularModels = useCallback(async () => {
    setLoadingPopular(true);
    try {
      const res = await invoke<HfModel[]>('search_hf_models', {
        query: '',
        limit: 8,
        author: 'mlx-community',
      });
      setPopularModels(res);
    } catch (err) {
      console.error('인기 모델 로드 오류:', err);
    } finally {
      setLoadingPopular(false);
    }
  }, []);

  const startDownload = useCallback(async (repoId: string) => {
    setDownloadingIds((prev) => new Set(prev).add(repoId));
    try {
      const res = await invoke<string>('download_hf_model', { repoId });
      await message(res || `${repoId} 다운로드를 시작했습니다.`, { title: 'KubeMetal', kind: 'info' });
      await fetchDownloads();
    } catch (err) {
      await message(`다운로드 시작 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setDownloadingIds((prev) => {
        const next = new Set(prev);
        next.delete(repoId);
        return next;
      });
    }
  }, [fetchDownloads]);

  const uploadToStorage = useCallback(async (repoId: string) => {
    setUploadingIds((prev) => new Set(prev).add(repoId));
    try {
      const res = await invoke<string>('upload_model_to_storage', { repoId });
      await message(res || `${repoId} 업로드가 완료되었습니다.`, { title: 'KubeMetal', kind: 'info' });
      setUploadedIds((prev) => new Set(prev).add(repoId));
    } catch (err) {
      await message(`SeaweedFS 업로드 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setUploadingIds((prev) => {
        const next = new Set(prev);
        next.delete(repoId);
        return next;
      });
    }
  }, []);

  const registerModel = useCallback(async (repoId: string) => {
    setRegisteringIds((prev) => new Set(prev).add(repoId));
    try {
      const res = await invoke<string>('register_model_mlflow', { repoId });
      await message(res || `${repoId} MLflow 등록이 완료되었습니다.`, { title: 'KubeMetal', kind: 'info' });
      setRegisteredIds((prev) => new Set(prev).add(repoId));
    } catch (err) {
      await message(`MLflow 등록 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setRegisteringIds((prev) => {
        const next = new Set(prev);
        next.delete(repoId);
        return next;
      });
    }
  }, []);

  useEffect(() => {
    fetchLocalModels();
    fetchDownloads();
    loadPopularModels();
  }, [fetchLocalModels, fetchDownloads, loadPopularModels]);

  const hasActiveDownload = downloads.some((d) => d.state === 'downloading');

  // 다운로드가 진행 중일 때만 3초 간격으로 상태를 폴링한다.
  useEffect(() => {
    if (!hasActiveDownload) return;
    const interval = setInterval(fetchDownloads, 3000);
    return () => clearInterval(interval);
  }, [hasActiveDownload, fetchDownloads]);

  // 다운로드가 새로 완료되면 로컬 모델 목록을 갱신한다.
  useEffect(() => {
    const prev = prevDownloadsRef.current;
    const justFinished = downloads.some(
      (d) => d.state === 'done' && !prev.some((p) => p.repo_id === d.repo_id && p.state === 'done'),
    );
    if (justFinished) {
      fetchLocalModels();
    }
    prevDownloadsRef.current = downloads;
  }, [downloads, fetchLocalModels]);

  return {
    searchResults,
    searching,
    search,
    popularModels,
    loadingPopular,
    downloads,
    downloadingIds,
    startDownload,
    localModels,
    refreshLocalModels: fetchLocalModels,
    uploadToStorage,
    uploadingIds,
    uploadedIds,
    registerModel,
    registeringIds,
    registeredIds,
  };
}
