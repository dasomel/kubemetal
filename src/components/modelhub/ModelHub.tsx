import React, { useState } from 'react';
import { useModelHub } from '../../hooks/useModelHub';
import { ModelSearchCard } from './ModelSearchCard';
import { ModelHubGuideCard } from './ModelHubGuideCard';
import { DownloadStatusCard } from './DownloadStatusCard';
import { LocalModelsCard } from './LocalModelsCard';
import type { ModelCategory, RamSizeProfile } from '../../lib/modelCategories';

export const ModelHub: React.FC = () => {
  const {
    searchResults,
    searching,
    search,
    popularModels,
    loadingPopular,
    downloads,
    downloadingIds,
    startDownload,
    localModels,
    uploadToStorage,
    uploadingIds,
    uploadedIds,
    registerModel,
    registeringIds,
    registeredIds,
  } = useModelHub();

  // 검색창의 검색어와 "어떤 프리셋이 활성화되어 있는지"는 ModelSearchCard의 카테고리 칩과
  // ModelHubGuideCard의 메모리 프로필 행이 함께 갱신해야 하므로(둘 다 같은 검색 결과 영역을
  // 제어) 여기서 소유하고 양쪽에 controlled 값으로 내려준다.
  const [query, setQuery] = useState('');
  const [activeSelectionId, setActiveSelectionId] = useState<string | null>(null);

  const handleQueryChange = (value: string) => {
    setQuery(value);
    setActiveSelectionId(null);
  };

  const handleSubmitSearch = () => {
    setActiveSelectionId(null);
    search(query, 20);
  };

  const handleSelectCategory = (category: ModelCategory) => {
    setQuery(category.query);
    setActiveSelectionId(category.id);
    if (category.query.trim()) {
      search(category.query, 8, category.author);
    }
  };

  const handleSelectRamProfile = (profile: RamSizeProfile) => {
    setQuery(profile.query);
    setActiveSelectionId(profile.id);
    search(profile.query, 8, profile.author);
  };

  return (
    <div className="space-y-4">
      <ModelHubGuideCard activeSelectionId={activeSelectionId} onSelectProfile={handleSelectRamProfile} />

      <ModelSearchCard
        results={searchResults}
        popularModels={popularModels}
        loadingPopular={loadingPopular}
        searching={searching}
        downloadingIds={downloadingIds}
        query={query}
        activeSelectionId={activeSelectionId}
        onQueryChange={handleQueryChange}
        onSubmitSearch={handleSubmitSearch}
        onSelectCategory={handleSelectCategory}
        onDownload={startDownload}
      />

      <DownloadStatusCard downloads={downloads} />

      <LocalModelsCard
        models={localModels}
        uploadingIds={uploadingIds}
        uploadedIds={uploadedIds}
        registeringIds={registeringIds}
        registeredIds={registeredIds}
        onUpload={uploadToStorage}
        onRegister={registerModel}
      />
    </div>
  );
};
