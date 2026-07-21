import React, { useState } from 'react';
import { Search, Loader2, Download, ArrowDownToLine, Heart, Tag } from 'lucide-react';
import type { HfModel } from '../../types/ipc';
import { MODEL_CATEGORIES, type ModelCategory } from '../../lib/modelCategories';

interface ModelSearchCardProps {
  results: HfModel[];
  popularModels: HfModel[];
  loadingPopular: boolean;
  searching: boolean;
  downloadingIds: Set<string>;
  onSearch: (query: string, limit: number, author?: string) => void;
  onDownload: (repoId: string) => void;
}

export const ModelSearchCard: React.FC<ModelSearchCardProps> = ({
  results,
  popularModels,
  loadingPopular,
  searching,
  downloadingIds,
  onSearch,
  onDownload,
}) => {
  const [query, setQuery] = useState('');
  const [activeCategoryId, setActiveCategoryId] = useState<string | null>(null);

  const activeCategory = MODEL_CATEGORIES.find((c) => c.id === activeCategoryId) ?? null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setActiveCategoryId(null);
    onSearch(query, 20);
  };

  const handleQueryChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setQuery(e.target.value);
    setActiveCategoryId(null);
  };

  const handleCategoryClick = (category: ModelCategory) => {
    setQuery(category.query);
    setActiveCategoryId(category.id);
    if (category.query.trim()) {
      onSearch(category.query, 8, category.author);
    }
  };

  const trimmedQuery = query.trim();
  const displayResults = trimmedQuery ? results : popularModels;
  const isLoadingDisplay = trimmedQuery ? searching : loadingPopular;
  const sectionLabel = trimmedQuery ? '검색 결과' : '인기 MLX 모델';

  return (
    <div className="rounded-xl bg-surface p-6 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">Hugging Face</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Search className="w-4 h-4 text-primary" />
          <span>모델 검색</span>
        </h2>
      </div>

      <form onSubmit={handleSubmit} className="flex gap-3 mb-4">
        <input
          type="text"
          value={query}
          onChange={handleQueryChange}
          placeholder="예: meta-llama/Llama-3.2-1B"
          className="flex-1 px-3.5 py-2.5 rounded-md bg-surfaceRaised text-ink text-body placeholder:text-inkFaint border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        />
        <button
          type="submit"
          disabled={searching || !query.trim()}
          className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        >
          {searching ? <Loader2 className="w-4 h-4 animate-spin" /> : <Search className="w-4 h-4" />}
          <span>검색</span>
        </button>
      </form>

      <div className="flex flex-wrap gap-2 mb-1.5">
        {MODEL_CATEGORIES.map((category) => {
          const isActive = activeCategoryId === category.id;
          return (
            <button
              key={category.id}
              type="button"
              onClick={() => handleCategoryClick(category)}
              className={`py-1.5 px-3 rounded-md text-caption transition-all border focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary ${
                isActive
                  ? 'bg-surfaceRaised text-primary border-primary/40'
                  : 'bg-surface text-inkMuted border-hairline/8 hover:brightness-95'
              }`}
            >
              {category.label}
            </button>
          );
        })}
      </div>
      <p className="text-caption text-inkFaint mb-4 min-h-[1em]">
        {activeCategory?.description ?? ''}
      </p>

      <div className="text-label uppercase text-inkFaint mb-2">{sectionLabel}</div>

      {displayResults.length === 0 ? (
        <div className="py-8 text-center text-inkMuted text-body">
          {isLoadingDisplay
            ? '모델을 불러오는 중...'
            : trimmedQuery
              ? '검색 결과가 없습니다.'
              : '인기 모델을 불러올 수 없습니다.'}
        </div>
      ) : (
        <div className="space-y-2">
          {displayResults.map((model) => {
            const isDownloading = downloadingIds.has(model.id);
            return (
              <div
                key={model.id}
                className="p-4 rounded-lg bg-surfaceRaised flex items-center justify-between gap-4"
              >
                <div className="min-w-0">
                  <div className="text-bodyStrong text-ink truncate">{model.id}</div>
                  <div className="flex items-center gap-3 mt-1 text-caption text-inkFaint">
                    <span className="flex items-center gap-1">
                      <ArrowDownToLine className="w-3.5 h-3.5" />
                      {model.downloads.toLocaleString()}
                    </span>
                    <span className="flex items-center gap-1">
                      <Heart className="w-3.5 h-3.5" />
                      {model.likes.toLocaleString()}
                    </span>
                    {model.pipeline_tag && (
                      <span className="flex items-center gap-1">
                        <Tag className="w-3.5 h-3.5" />
                        {model.pipeline_tag}
                      </span>
                    )}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => onDownload(model.id)}
                  disabled={isDownloading}
                  className="shrink-0 py-2 px-3.5 bg-surface hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-bodyStrong rounded-md transition-all flex items-center gap-1.5 border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                >
                  {isDownloading ? (
                    <Loader2 className="w-3.5 h-3.5 animate-spin text-primary" />
                  ) : (
                    <Download className="w-3.5 h-3.5 text-primary" />
                  )}
                  <span>다운로드</span>
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};
