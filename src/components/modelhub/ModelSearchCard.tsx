import React from 'react';
import { Search, Loader2, Download, ArrowDownToLine, Heart, Tag, HardDrive } from 'lucide-react';
import type { HfModel } from '../../types/ipc';
import { MODEL_CATEGORIES, type ModelCategory } from '../../lib/modelCategories';
import { useTranslation } from '../../i18n/i18nContext';

interface ModelSearchCardProps {
  results: HfModel[];
  popularModels: HfModel[];
  loadingPopular: boolean;
  searching: boolean;
  downloadingIds: Set<string>;
  /** 검색창에 표시되는 현재 검색어 — 카테고리 칩/메모리 가이드 클릭으로도 갱신되므로
   * 부모(ModelHub)가 소유한다(controlled input). */
  query: string;
  /** 현재 활성화된 프리셋 id(카테고리 칩 또는 메모리 가이드 행) — 둘 중 어느 쪽에서 선택됐든
   * id 네임스페이스가 겹치지 않아(카테고리는 평문 id, 가이드는 `ram-` 접두사) 하나로 관리한다. */
  activeSelectionId: string | null;
  onQueryChange: (value: string) => void;
  onSubmitSearch: () => void;
  onSelectCategory: (category: ModelCategory) => void;
  onDownload: (repoId: string) => void;
}

const formatSize = (bytes: number): string => {
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb.toFixed(1)}GB`;
  const mb = bytes / 1024 ** 2;
  return `${mb.toFixed(0)}MB`;
};

export const ModelSearchCard: React.FC<ModelSearchCardProps> = ({
  results,
  popularModels,
  loadingPopular,
  searching,
  downloadingIds,
  query,
  activeSelectionId,
  onQueryChange,
  onSubmitSearch,
  onSelectCategory,
  onDownload,
}) => {
  const { t } = useTranslation();
  const activeCategory = MODEL_CATEGORIES.find((c) => c.id === activeSelectionId) ?? null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    onSubmitSearch();
  };

  const trimmedQuery = query.trim();
  const displayResults = trimmedQuery ? results : popularModels;
  const isLoadingDisplay = trimmedQuery ? searching : loadingPopular;
  const sectionLabel = trimmedQuery ? t('modelhub.searchResultsTitle') : t('modelhub.popularModelsTitle');

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">Hugging Face</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <Search className="w-4 h-4 text-primary" />
          <span>{t('modelhub.searchTitle')}</span>
        </h2>
      </div>

      <form onSubmit={handleSubmit} className="flex gap-3 mb-4">
        <input
          type="text"
          value={query}
          onChange={(e) => onQueryChange(e.target.value)}
          placeholder={t('modelhub.searchPlaceholder')}
          className="flex-1 px-3.5 py-2.5 rounded-md bg-surfaceRaised text-ink text-body placeholder:text-inkFaint border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        />
        <button
          type="submit"
          disabled={searching || !query.trim()}
          className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        >
          {searching ? <Loader2 className="w-4 h-4 animate-spin" /> : <Search className="w-4 h-4" />}
          <span>{t('modelhub.searchBtn')}</span>
        </button>
      </form>

      <div className="flex flex-wrap gap-2 mb-1.5">
        {MODEL_CATEGORIES.map((category) => {
          const isActive = activeSelectionId === category.id;
          return (
            <button
              key={category.id}
              type="button"
              onClick={() => onSelectCategory(category)}
              className={`py-1.5 px-3 rounded-md text-caption transition-all border focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary ${
                isActive
                  ? 'bg-surfaceRaised text-primary border-primary/40'
                  : 'bg-surface text-inkMuted border-hairline/8 hover:brightness-95'
              }`}
            >
              {t(category.labelKey)}
            </button>
          );
        })}
      </div>
      <p className="text-caption text-inkFaint mb-4 min-h-[1em]">
        {activeCategory ? t(activeCategory.descKey) : ''}
      </p>

      <div className="text-label uppercase text-inkFaint mb-2">{sectionLabel}</div>

      {displayResults.length === 0 ? (
        <div className="py-8 text-center text-inkMuted text-body">
          {isLoadingDisplay
            ? t('modelhub.loadingModels')
            : trimmedQuery
              ? t('modelhub.noSearchResults')
              : t('modelhub.noPopularModels')}
        </div>
      ) : (
        <div className="space-y-2">
          {displayResults.map((model) => {
            const isDownloading = downloadingIds.has(model.id);
            return (
              <div
                key={model.id}
                className="p-3 rounded-lg bg-surfaceRaised flex items-center justify-between gap-4"
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
                    {model.size_bytes !== undefined && (
                      <span className="flex items-center gap-1">
                        <HardDrive className="w-3.5 h-3.5" />
                        {formatSize(model.size_bytes)}
                      </span>
                    )}
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
                  <span>{isDownloading ? t('modelhub.downloadingBtn') : t('modelhub.downloadBtn')}</span>
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};
