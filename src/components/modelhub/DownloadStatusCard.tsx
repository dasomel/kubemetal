import React from 'react';
import { Loader2, DownloadCloud } from 'lucide-react';
import type { DownloadStatus } from '../../types/ipc';

interface DownloadStatusCardProps {
  downloads: DownloadStatus[];
}

export const DownloadStatusCard: React.FC<DownloadStatusCardProps> = ({ downloads }) => {
  if (downloads.length === 0) return null;

  return (
    <div className="rounded-xl bg-surface p-6 shadow-panel">
      <div className="mb-4">
        <div className="text-label uppercase text-inkFaint mb-1">Progress</div>
        <h2 className="text-heading text-ink flex items-center gap-2">
          <DownloadCloud className="w-4 h-4 text-primary" />
          <span>다운로드 진행 상황</span>
        </h2>
      </div>

      <div className="space-y-3">
        {downloads.map((d) => {
          const percent = d.total_files > 0 ? Math.min((d.done_files / d.total_files) * 100, 100) : 0;
          return (
            <div key={d.repo_id} className="p-4 rounded-lg bg-surfaceRaised">
              <div className="flex items-center justify-between mb-2">
                <span className="text-bodyStrong text-ink truncate">{d.repo_id}</span>
                <span className="text-caption text-inkFaint tabular-nums">
                  {d.done_files} / {d.total_files} 파일
                </span>
              </div>
              <div
                className="w-full h-1.5 bg-base rounded-full overflow-hidden mb-2"
                role="progressbar"
                aria-valuenow={percent}
                aria-valuemin={0}
                aria-valuemax={100}
              >
                <div
                  className="h-full bg-primary transition-all duration-500 rounded-full"
                  style={{ width: `${percent}%` }}
                />
              </div>
              {d.state === 'downloading' && (
                <div className="flex items-center gap-1.5 text-caption text-inkMuted">
                  <Loader2 className="w-3.5 h-3.5 animate-spin text-primary" />
                  <span>다운로드 중</span>
                </div>
              )}
              {d.state === 'done' && (
                <div className="flex items-center gap-1.5 text-caption text-inkMuted">
                  <span className="w-2 h-2 rounded-full bg-success" />
                  <span>완료</span>
                </div>
              )}
              {d.state === 'error' && (
                <div className="flex items-center gap-1.5 text-caption text-danger">
                  <span className="w-2 h-2 rounded-full bg-danger" />
                  <span>오류: {d.error ?? '알 수 없는 오류'}</span>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
};
