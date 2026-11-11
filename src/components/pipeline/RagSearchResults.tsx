import React from 'react';
import { FileText } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';
import type { RagSearchResult, RetrievalMode } from '../../types/ipc';

export interface RagSearchResultsProps {
  searchResults: RagSearchResult[];
  routing?: { resolved_mode: RetrievalMode; rule: string } | null;
  hasSearched: boolean;
  searching: boolean;
}

export const RagSearchResults: React.FC<RagSearchResultsProps> = ({
  searchResults,
  routing,
  hasSearched,
  searching,
}) => {
  const { t } = useTranslation();

  return (
    <>
      {hasSearched && routing && (
        <div className="text-caption text-inkMuted">
          {t('rag.resolved', {
            mode: routing.resolved_mode,
            rule: routing.rule,
          })}
        </div>
      )}

      {/* 검색 결과 목록 */}
      {searchResults.length > 0 && (
        <div className="space-y-2 mt-3">
          <div className="text-label uppercase text-inkFaint">
            {t('rag.searchResultsHeader', { count: searchResults.length })}
          </div>
          {searchResults.map((item, index) => (
            <div
              key={item.id ?? `rag-result-${index}`}
              className="p-3 rounded-lg bg-surfaceRaised border border-hairline/8 space-y-1"
            >
              <div className="flex items-center justify-between text-caption">
                <span className="text-primary font-semibold truncate flex items-center gap-1">
                  <FileText className="w-3 h-3 inline shrink-0" />
                  {item.source?.trim() || t('rag.unknownSource')}
                </span>
                <span className="px-1.5 py-0.5 rounded bg-surface text-inkMuted text-[11px]">
                  {item.mode === 'exact'
                    ? t('rag.exactMatch')
                    : item.mode === 'lexical'
                    ? `${t('rag.score')} ${item.score.toExponential(2)}`
                    : item.mode === 'hybrid'
                    ? `${t('rag.rrfScore')} ${item.score.toFixed(4)}`
                    : `${t('rag.similarity')} ${(item.score * 100).toFixed(1)}%`}
                </span>
              </div>
              <p className="text-body text-ink text-sm leading-relaxed">
                {item.text?.trim() ? item.text : t('rag.unknownText')}
              </p>
              {item.mode === 'hybrid' && item.provenance && (
                <div className="text-caption text-inkMuted">
                  {t('rag.provenance', {
                    retrievers: item.provenance.retrievers?.join(', ') || '',
                    lexical: item.provenance.lexical
                      ? `#${item.provenance.lexical.rank} (${item.provenance.lexical.score.toExponential(2)})`
                      : t('rag.notHit'),
                    dense: item.provenance.dense
                      ? `#${item.provenance.dense.rank} (${item.provenance.dense.score.toFixed(4)})`
                      : t('rag.notHit'),
                  })}
                </div>
              )}
            </div>
          ))}
        </div>
      )}

      {searchResults.length === 0 && hasSearched && !searching && (
        <div className="py-3 text-center text-inkFaint text-caption">
          {t('rag.noResults')}
        </div>
      )}
    </>
  );
};
