import React from 'react';
import { Cpu, Layers, HardDrive, Globe } from 'lucide-react';
import { useMetrics } from '../../hooks/useMetrics';
import { useTranslation } from '../../i18n/i18nContext';

export const Header: React.FC = () => {
  const metrics = useMetrics(3000);
  const { language, setLanguage, t } = useTranslation();

  return (
    <header className="border-b border-hairline/8 bg-surface px-6 py-4 flex items-center justify-between">
      <div className="flex items-center gap-3">
        <div className="w-10 h-10 rounded-lg bg-primaryStrong flex items-center justify-center">
          <Layers className="w-5 h-5 text-inverse" />
        </div>
        <div>
          <h1 className="text-display text-ink flex items-center gap-2">
            {t('header.title')}
            <span className="text-caption font-normal text-inkFaint">v0.1.0</span>
          </h1>
          <p className="text-caption text-inkMuted">
            {t('header.subtitle')}
          </p>
        </div>
      </div>

      <div className="flex items-center gap-3 text-caption text-inkMuted">
        <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-surfaceRaised border border-hairline/8">
          <Cpu className="w-3.5 h-3.5 text-primary" />
          <span>
            {t('header.hostMemory')}
            {metrics ? ` (${metrics.used_memory_gb.toFixed(1)}GB / ${metrics.total_memory_gb.toFixed(0)}GB)` : ''}
          </span>
        </div>
        <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-surfaceRaised border border-hairline/8">
          <HardDrive className="w-3.5 h-3.5 text-primary" />
          <span>{t('header.vmVirt')}</span>
        </div>

        {/* Language Switcher Toggle Button */}
        <div className="flex items-center gap-1 bg-surfaceRaised border border-hairline/8 rounded-md p-0.5">
          <Globe className="w-3.5 h-3.5 text-primary ml-1.5 mr-0.5 shrink-0" />
          <button
            type="button"
            onClick={() => setLanguage('ko')}
            className={`px-2 py-0.5 rounded text-caption font-semibold transition-colors ${
              language === 'ko'
                ? 'bg-primary text-white shadow-xs'
                : 'text-inkMuted hover:text-ink'
            }`}
          >
            KR
          </button>
          <button
            type="button"
            onClick={() => setLanguage('en')}
            className={`px-2 py-0.5 rounded text-caption font-semibold transition-colors ${
              language === 'en'
                ? 'bg-primary text-white shadow-xs'
                : 'text-inkMuted hover:text-ink'
            }`}
          >
            EN
          </button>
        </div>
      </div>
    </header>
  );
};
