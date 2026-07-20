import React from 'react';
import { useModelHub } from '../../hooks/useModelHub';
import { ModelSearchCard } from './ModelSearchCard';
import { DownloadStatusCard } from './DownloadStatusCard';
import { LocalModelsCard } from './LocalModelsCard';

export const ModelHub: React.FC = () => {
  const {
    searchResults,
    searching,
    search,
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

  return (
    <div className="space-y-6">
      <ModelSearchCard
        results={searchResults}
        searching={searching}
        downloadingIds={downloadingIds}
        onSearch={search}
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
