import React from 'react';
import { HardDrive, Loader2, UploadCloud, ClipboardCheck } from 'lucide-react';
import type { LocalModel } from '../../types/ipc';

interface LocalModelsCardProps {
  models: LocalModel[];
  uploadingIds: Set<string>;
  uploadedIds: Set<string>;
  registeringIds: Set<string>;
  registeredIds: Set<string>;
  onUpload: (repoId: string) => void;
  onRegister: (repoId: string) => void;
}

const formatGb = (bytes: number) => (bytes / 1024 ** 3).toFixed(2);

export const LocalModelsCard: React.FC<LocalModelsCardProps> = ({
  models,
  uploadingIds,
  uploadedIds,
  registeringIds,
  registeredIds,
  onUpload,
  onRegister,
}) => {
  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">Local Cache</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <HardDrive className="w-4 h-4 text-primary" />
          <span>로컬 모델</span>
        </h2>
      </div>

      {models.length === 0 ? (
        <div className="py-8 text-center text-inkMuted text-body">
          다운로드된 로컬 모델이 없습니다. 모델 검색 후 다운로드를 시작하세요.
        </div>
      ) : (
        <div className="space-y-2">
          {models.map((model) => {
            const isUploading = uploadingIds.has(model.repo_id);
            const isUploaded = uploadedIds.has(model.repo_id);
            const isRegistering = registeringIds.has(model.repo_id);
            const isRegistered = registeredIds.has(model.repo_id);

            return (
              <div
                key={model.repo_id}
                className="p-3 rounded-lg bg-surfaceRaised flex items-center justify-between gap-4"
              >
                <div className="min-w-0">
                  <div className="text-bodyStrong text-ink truncate">{model.repo_id}</div>
                  <div className="flex items-center gap-3 mt-1 text-caption text-inkFaint">
                    <span className="tabular-nums">{formatGb(model.size_bytes)} GB</span>
                    <span className="truncate">{model.path}</span>
                  </div>
                </div>

                <div className="shrink-0 flex items-center gap-2">
                  <button
                    type="button"
                    onClick={() => onUpload(model.repo_id)}
                    disabled={isUploading || isUploaded}
                    className="py-2 px-3.5 bg-surface hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-bodyStrong rounded-md transition-all flex items-center gap-1.5 border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                  >
                    {isUploading ? (
                      <Loader2 className="w-3.5 h-3.5 animate-spin text-primary" />
                    ) : isUploaded ? (
                      <span className="w-2 h-2 rounded-full bg-success" />
                    ) : (
                      <UploadCloud className="w-3.5 h-3.5 text-primary" />
                    )}
                    <span>{isUploaded ? 'SeaweedFS 업로드됨' : 'SeaweedFS 업로드'}</span>
                  </button>

                  <button
                    type="button"
                    onClick={() => onRegister(model.repo_id)}
                    disabled={isRegistering || isRegistered}
                    className="py-2 px-3.5 bg-surface hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-bodyStrong rounded-md transition-all flex items-center gap-1.5 border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                  >
                    {isRegistering ? (
                      <Loader2 className="w-3.5 h-3.5 animate-spin text-primary" />
                    ) : isRegistered ? (
                      <span className="w-2 h-2 rounded-full bg-success" />
                    ) : (
                      <ClipboardCheck className="w-3.5 h-3.5 text-primary" />
                    )}
                    <span>{isRegistered ? 'MLflow 등록됨' : 'MLflow 등록'}</span>
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};
