import React, { useState } from 'react';
import { Tag, GitBranch, Loader2, Plus, Server } from 'lucide-react';
import { useDVC } from '../../hooks/useDVC';
import { useTranslation } from '../../i18n/i18nContext';

const inputClass =
  'w-full px-3.5 py-2 rounded-md bg-surfaceRaised text-ink text-body placeholder:text-inkFaint border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary';
const labelClass = 'text-label uppercase text-inkFaint mb-1 block';

export const DvcCard: React.FC = () => {
  const { status, initializing, creatingTag, initDvc, createTag } = useDVC(true);
  const { t } = useTranslation();
  const [tagName, setTagName] = useState('');
  const [tagMessage, setTagMessage] = useState('');
  const [datasetPath, setDatasetPath] = useState('./data/dataset');

  const handleCreateTag = (e: React.FormEvent) => {
    e.preventDefault();
    if (!tagName.trim()) return;
    createTag(tagName, tagMessage || `Dataset version ${tagName}`, datasetPath);
    setTagName('');
    setTagMessage('');
  };

  const isInitialized = status?.initialized ?? false;

  return (
    <div className="animate-card-in rounded-xl bg-surface p-4 shadow-panel">
      <div className="flex items-center justify-between mb-4">
        <div>
          <div className="text-label uppercase text-inkFaint mb-1">SeaweedFS S3 Remote</div>
          <h2 className="text-heading text-ink flex items-center gap-2">
            <GitBranch className="w-4 h-4 text-primary" />
            <span>{t('dvc.title')}</span>
          </h2>
        </div>
        <div className="flex items-center gap-1.5 text-caption text-inkMuted">
          <span
            className={`w-2 h-2 rounded-full ${
              isInitialized ? 'bg-success' : 'bg-inkFaint'
            }`}
          />
          <span>{isInitialized ? t('dvc.connected') : t('dvc.uninitialized')}</span>
        </div>
      </div>

      <p className="text-caption text-inkMuted mb-4">
        {t('dvc.desc')}
      </p>

      {/* DVC 미초기화 시 초기화 버튼 */}
      {!isInitialized ? (
        <div className="p-3 rounded-lg bg-surfaceRaised flex items-center justify-between gap-3 mb-4">
          <div>
            <div className="text-bodyStrong text-ink">{t('dvc.initRepoTitle')}</div>
            <div className="text-caption text-inkFaint mt-0.5">
              {t('dvc.initRepoDesc')}
            </div>
          </div>
          <button
            type="button"
            onClick={() => initDvc()}
            disabled={initializing}
            className="py-2 px-3.5 bg-primaryStrong hover:brightness-110 disabled:opacity-50 text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            {initializing ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Server className="w-3.5 h-3.5" />}
            <span>{initializing ? t('dvc.initializingBtn') : t('dvc.initBtn')}</span>
          </button>
        </div>
      ) : (
        <div className="p-3 rounded-lg bg-surfaceRaised mb-4 flex items-center justify-between text-caption">
          <div className="flex items-center gap-2">
            <Server className="w-4 h-4 text-primary" />
            <span className="text-inkFaint">{t('dvc.remoteS3')}</span>
            <span className="text-ink font-mono">{status?.remote_url || 's3://kubemetal-dvc'}</span>
          </div>
          {status?.current_tag && (
            <div className="flex items-center gap-1">
              <span className="text-inkFaint">{t('dvc.currentTag')}</span>
              <span className="px-2 py-0.5 rounded bg-primary/10 text-primary font-medium">{status.current_tag}</span>
            </div>
          )}
        </div>
      )}

      {/* 새 데이터셋 버전 태그 생성 폼 */}
      <div className="p-3 rounded-lg bg-surfaceRaised mb-4 space-y-3 border border-hairline/8">
        <h3 className="text-bodyStrong text-ink flex items-center gap-1.5">
          <Plus className="w-4 h-4 text-primary" />
          <span>{t('dvc.createTagTitle')}</span>
        </h3>

        <form onSubmit={handleCreateTag} className="space-y-3">
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
            <div>
              <label className={labelClass}>{t('dvc.tagNameLabel')}</label>
              <input
                type="text"
                value={tagName}
                onChange={(e) => setTagName(e.target.value)}
                placeholder="v1.0-instruction-dataset"
                className={inputClass}
              />
            </div>
            <div>
              <label className={labelClass}>{t('dvc.datasetPathLabel')}</label>
              <input
                type="text"
                value={datasetPath}
                onChange={(e) => setDatasetPath(e.target.value)}
                placeholder="./data/dataset"
                className={inputClass}
              />
            </div>
          </div>
          <div>
            <label className={labelClass}>{t('dvc.tagDescLabel')}</label>
            <input
              type="text"
              value={tagMessage}
              onChange={(e) => setTagMessage(e.target.value)}
              placeholder={t('dvc.tagDescPlaceholder')}
              className={inputClass}
            />
          </div>
          <div className="flex justify-end">
            <button
              type="submit"
              disabled={creatingTag || !tagName.trim()}
              className="py-2 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-50 text-inverse text-bodyStrong rounded-md transition-all flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
            >
              {creatingTag ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Tag className="w-3.5 h-3.5" />}
              <span>{creatingTag ? t('dvc.creatingTagBtn') : t('dvc.createTagBtn')}</span>
            </button>
          </div>
        </form>
      </div>

      {/* 데이터셋 버전 태그 목록 */}
      <div className="pt-3 border-t border-hairline/8 space-y-2">
        <h3 className="text-label uppercase text-inkFaint">{t('dvc.registeredTagsHeader', { count: status?.tags.length || 0 })}</h3>
        {status?.tags && status.tags.length > 0 ? (
          <div className="space-y-2">
            {status.tags.map((tag) => (
              <div key={tag.tag} className="p-3 rounded-lg bg-surfaceRaised flex items-start justify-between gap-3 border border-hairline/8">
                <div>
                  <div className="flex items-center gap-2">
                    <span className="px-2 py-0.5 rounded bg-surface text-primary font-mono text-bodyStrong border border-hairline/8">
                      {tag.tag}
                    </span>
                    <span className="text-caption text-inkFaint font-mono">{tag.commit_hash.slice(0, 7)}</span>
                  </div>
                  <div className="text-body text-ink mt-1.5">{tag.message}</div>
                  {tag.dataset_path && (
                    <div className="text-caption text-inkMuted mt-1">{t('dvc.pathLabel')} {tag.dataset_path}</div>
                  )}
                </div>
                {tag.created_at && (
                  <span className="text-caption text-inkFaint shrink-0">{tag.created_at}</span>
                )}
              </div>
            ))}
          </div>
        ) : (
          <div className="py-4 text-center text-inkFaint text-caption">
            {t('dvc.noTags')}
          </div>
        )}
      </div>
    </div>
  );
};
