import React, { useState } from 'react';
import { Database, Search, FileText, Loader2, Play } from 'lucide-react';
import { useRAG } from '../../hooks/useRAG';

const inputClass =
  'w-full px-3.5 py-2 rounded-md bg-surfaceRaised text-ink text-body placeholder:text-inkFaint border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary';
const labelClass = 'text-label uppercase text-inkFaint mb-1 block';

export const RagCard: React.FC = () => {
  const { status, indexing, searching, searchResults, triggerIndex, search } = useRAG(true);
  const [docPath, setDocPath] = useState('./data/docs');
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
            <span>로컬 RAG & 지식 베이스</span>
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
              ? '인덱싱 중...'
              : isReady
              ? `준비됨 (${status?.document_count || 0} 문서, ${status?.total_chunks || 0} 청크)`
              : '대기'}
          </span>
        </div>
      </div>

      <p className="text-caption text-inkMuted mb-4">
        로컬 지식 문서를 LanceDB 벡터 데이터베이스에 인덱싱하고 시맨틱 코사인 유사도 검색을 테스트합니다.
      </p>

      {/* ① 문서 인덱싱 폼 및 요약 */}
      <div className="p-3 rounded-lg bg-surfaceRaised mb-4 space-y-3">
        <h3 className="text-bodyStrong text-ink flex items-center gap-1.5">
          <FileText className="w-4 h-4 text-primary" />
          <span>문서 인덱싱</span>
        </h3>

        <form onSubmit={handleIndexSubmit} className="flex gap-2 items-end">
          <div className="flex-1">
            <label className={labelClass}>문서 디렉토리 경로</label>
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
            <span>{indexing ? '인덱싱 중...' : '인덱싱 시작'}</span>
          </button>
        </form>

        {status && (
          <div className="grid grid-cols-2 sm:grid-cols-3 gap-2 pt-2 border-t border-hairline/8 text-caption">
            <div>
              <span className="text-inkFaint">인덱싱 문서: </span>
              <span className="text-ink font-semibold">{status.document_count}개</span>
            </div>
            <div>
              <span className="text-inkFaint">벡터 청크: </span>
              <span className="text-ink font-semibold">{status.total_chunks}개</span>
            </div>
            {status.last_indexed_at && (
              <div className="col-span-2 sm:col-span-1">
                <span className="text-inkFaint">최종 업데이트: </span>
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
          <span>시맨틱 검색 테스트</span>
        </h3>

        <form onSubmit={handleSearchSubmit} className="flex gap-2">
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="검색할 질의 입력 (예: Apple Silicon Metal 메모리 최적화...)"
            className={inputClass}
          />
          <button
            type="submit"
            disabled={searching || !searchQuery.trim()}
            className="py-2 px-3.5 bg-surfaceRaised hover:brightness-95 disabled:opacity-50 text-ink text-bodyStrong rounded-md transition-all flex items-center gap-1.5 shrink-0 border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            {searching ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Search className="w-3.5 h-3.5" />}
            <span>검색</span>
          </button>
        </form>

        {/* 검색 결과 목록 */}
        {searchResults.length > 0 && (
          <div className="space-y-2 mt-3">
            <div className="text-label uppercase text-inkFaint">유사도 검색 결과 ({searchResults.length}건)</div>
            {searchResults.map((item) => (
              <div key={item.id} className="p-3 rounded-lg bg-surfaceRaised border border-hairline/8 space-y-1">
                <div className="flex items-center justify-between text-caption">
                  <span className="text-primary font-semibold truncate flex items-center gap-1">
                    <FileText className="w-3 h-3 inline shrink-0" />
                    {item.source || item.id}
                  </span>
                  <span className="px-1.5 py-0.5 rounded bg-surface text-inkMuted text-[11px]">
                    유사도: {(item.score * 100).toFixed(1)}%
                  </span>
                </div>
                <p className="text-body text-ink text-sm leading-relaxed">{item.text}</p>
              </div>
            ))}
          </div>
        )}

        {searchResults.length === 0 && searchQuery && !searching && (
          <div className="py-3 text-center text-inkFaint text-caption">
            검색 결과가 없습니다.
          </div>
        )}
      </div>
    </div>
  );
};
