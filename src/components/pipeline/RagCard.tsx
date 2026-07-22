import React, { useState } from 'react';
import { Database, Search, FileText, Loader2, Play } from 'lucide-react';
import { useRAG } from '../../hooks/useRAG';
import { useTranslation } from '../../i18n/i18nContext';

const inputClass =
  'w-full px-3.5 py-2 rounded-md bg-surfaceRaised text-ink text-body placeholder:text-inkFaint border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary';
const labelClass = 'text-label uppercase text-inkFaint mb-1 block';

export const RagCard: React.FC = () => {
  const { status, indexing, searching, searchResults, triggerIndex, search } = useRAG(true);
  const { t } = useTranslation();
  const [docPath, setDocPath] = useState('docs');
  const [searchQuery, setSearchQuery] = useState('');

  const handleIndexSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    triggerIndex(docPath);
  };

  const handleSearchSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (searchQuery.trim()) {
      search(searchQuery);
    }
  };

  const isReady = status?.status === 'ready' || (status?.document_count ?? 0) > 0;

  return (
    <div className="animate-card-in rounded-xl bg-surface p-4 shadow-panel">
      <div className="flex items-center justify-between mb-4">
        <div>
          <div className="text-label uppercase text-inkFaint mb-1">LanceDB Embedded</div>
          <h2 className="text-heading text-ink flex items-center gap-2">
            <Database className="w-4 h-4 text-primary" />
            <span>{t('rag.title')}</span>
          </h2>
        </div>
        <div className="flex items-center gap-1.5 text-caption text-inkMuted">
          <span
            className={`w-2 h-2 rounded-full ${
              indexing
                ? 'bg-warning animate-pulse'
                : isReady
                ? 'bg-success'
                : 'bg-inkFaint'
            }`}
          />
          <span>
            {indexing
              ? t('rag.indexingStatus')
              : isReady
              ? t('rag.readyStatus', { docs: status?.document_count || 0, chunks: status?.total_chunks || 0 })
              : t('rag.idleStatus')}
          </span>
        </div>
      </div>

      <p className="text-caption text-inkMuted mb-4">
        {t('rag.desc')}
      </p>

      {/* ① 문서 인덱싱 폼 및 요약 */}
      <div className="p-3 rounded-lg bg-surfaceRaised mb-4 space-y-3">
        <h3 className="text-bodyStrong text-ink flex items-center gap-1.5">
          <FileText className="w-4 h-4 text-primary" />
          <span>{t('rag.docIndexingTitle')}</span>
        </h3>

        <form onSubmit={handleIndexSubmit} className="flex gap-2 items-end">
          <div className="flex-1">
            <label className={labelClass}>{t('rag.docDirLabel')}</label>
            <input
              type="text"
              value={docPath}
              onChange={(e) => setDocPath(e.target.value)}
              placeholder="./data/docs"
              className={inputClass}
            />
          </div>
          <button
            type="submit"
            disabled={indexing || !docPath.trim()}
            className="py-2 px-3.5 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            {indexing ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Play className="w-3.5 h-3.5" />}
            <span>{indexing ? t('rag.indexingInProgress') : t('rag.startIndexingBtn')}</span>
          </button>
        </form>

        {status && (
          <div className="grid grid-cols-2 sm:grid-cols-3 gap-2 pt-2 border-t border-hairline/8 text-caption">
            <div>
              <span className="text-inkFaint">{t('rag.indexedDocs')} </span>
              <span className="text-ink font-semibold">{status.document_count}</span>
            </div>
            <div>
              <span className="text-inkFaint">{t('rag.vectorChunks')} </span>
              <span className="text-ink font-semibold">{status.total_chunks}</span>
            </div>
            {status.last_indexed_at && (
              <div className="col-span-2 sm:col-span-1">
                <span className="text-inkFaint">{t('rag.lastUpdated')} </span>
                <span className="text-ink">{status.last_indexed_at}</span>
              </div>
            )}
          </div>
        )}
      </div>

      {/* ② 시맨틱 검색 테스트 */}
      <div className="pt-3 border-t border-hairline/8 space-y-3">
        <h3 className="text-bodyStrong text-ink flex items-center gap-1.5">
          <Search className="w-4 h-4 text-primary" />
          <span>{t('rag.searchTitle')}</span>
        </h3>

        <form onSubmit={handleSearchSubmit} className="flex gap-2">
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder={t('rag.searchPlaceholder')}
            className={inputClass}
          />
          <button
            type="submit"
            disabled={searching || !searchQuery.trim()}
            className="py-2 px-3.5 bg-surfaceRaised hover:brightness-95 disabled:opacity-50 text-ink text-bodyStrong rounded-md transition-all flex items-center gap-1.5 shrink-0 border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            {searching ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Search className="w-3.5 h-3.5" />}
            <span>{t('rag.searchBtn')}</span>
          </button>
        </form>

        {/* 검색 결과 목록 */}
        {searchResults.length > 0 && (
          <div className="space-y-2 mt-3">
            <div className="text-label uppercase text-inkFaint">{t('rag.searchResultsHeader', { count: searchResults.length })}</div>
            {searchResults.map((item) => (
              <div key={item.id} className="p-3 rounded-lg bg-surfaceRaised border border-hairline/8 space-y-1">
                <div className="flex items-center justify-between text-caption">
                  <span className="text-primary font-semibold truncate flex items-center gap-1">
                    <FileText className="w-3 h-3 inline shrink-0" />
                    {item.source || item.id}
                  </span>
                  <span className="px-1.5 py-0.5 rounded bg-surface text-inkMuted text-[11px]">
                    {t('rag.similarity')} {(item.score * 100).toFixed(1)}%
                  </span>
                </div>
                <p className="text-body text-ink text-sm leading-relaxed">{item.text}</p>
              </div>
            ))}
          </div>
        )}

        {searchResults.length === 0 && searchQuery && !searching && (
          <div className="py-3 text-center text-inkFaint text-caption">
            {t('rag.noResults')}
          </div>
        )}
      </div>
    </div>
  );
};
