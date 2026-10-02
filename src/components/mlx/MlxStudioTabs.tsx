import React, { useRef } from 'react';
import { Activity, Bot, Cpu, Sliders } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';

export type MlxSubTab = 'env' | 'runtime' | 'training' | 'serving';

const STORAGE_KEY = 'kubemetal_mlx_subtab';
let inMemorySubTab: MlxSubTab = 'env';

export function getStoredMlxSubTab(): MlxSubTab {
  try {
    const val = localStorage.getItem(STORAGE_KEY) as MlxSubTab;
    if (val === 'env' || val === 'runtime' || val === 'training' || val === 'serving') {
      inMemorySubTab = val;
      return val;
    }
  } catch {
    // localStorage 접근 실패 시 인메모리 fallback
  }
  return inMemorySubTab;
}

export function setStoredMlxSubTab(tab: MlxSubTab): void {
  inMemorySubTab = tab;
  try {
    localStorage.setItem(STORAGE_KEY, tab);
  } catch {
    // quota/security exception 무시
  }
}

interface TabDef {
  id: MlxSubTab;
  labelKey: string;
  icon: React.ElementType;
}

export const MLX_SUB_TABS: TabDef[] = [
  { id: 'env', labelKey: 'mlx.subtabEnv', icon: Activity },
  { id: 'runtime', labelKey: 'mlx.subtabRuntime', icon: Cpu },
  { id: 'training', labelKey: 'mlx.subtabTraining', icon: Sliders },
  { id: 'serving', labelKey: 'mlx.subtabServing', icon: Bot },
];

interface MlxStudioTabsProps {
  activeTab: MlxSubTab;
  onTabChange: (tab: MlxSubTab) => void;
}

export const MlxStudioTabs: React.FC<MlxStudioTabsProps> = ({ activeTab, onTabChange }) => {
  const { t } = useTranslation();
  const tabButtonRefs = useRef<(HTMLButtonElement | null)[]>([]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLButtonElement>, currentIndex: number) => {
    let targetIndex = -1;
    const tabCount = MLX_SUB_TABS.length;

    switch (e.key) {
      case 'ArrowRight':
      case 'ArrowDown':
        targetIndex = (currentIndex + 1) % tabCount;
        break;
      case 'ArrowLeft':
      case 'ArrowUp':
        targetIndex = (currentIndex - 1 + tabCount) % tabCount;
        break;
      case 'Home':
        targetIndex = 0;
        break;
      case 'End':
        targetIndex = tabCount - 1;
        break;
      default:
        return;
    }

    e.preventDefault();
    const nextTab = MLX_SUB_TABS[targetIndex];
    if (nextTab) {
      onTabChange(nextTab.id);
      tabButtonRefs.current[targetIndex]?.focus();
    }
  };

  return (
    <div
      role="tablist"
      aria-label={t('mlx.subtabsAriaLabel')}
      aria-orientation="horizontal"
      className="border-b border-hairline/8 flex items-center gap-2 overflow-x-auto pb-0.5"
    >
      {MLX_SUB_TABS.map(({ id, labelKey, icon: Icon }, index) => {
        const isSelected = activeTab === id;
        return (
          <button
            key={id}
            ref={(el) => {
              tabButtonRefs.current[index] = el;
            }}
            id={`mlx-tab-${id}`}
            role="tab"
            type="button"
            aria-selected={isSelected}
            aria-controls={`mlx-panel-${id}`}
            tabIndex={isSelected ? 0 : -1}
            onClick={() => onTabChange(id)}
            onKeyDown={(e) => handleKeyDown(e, index)}
            className={`px-4 py-2.5 rounded-t-xl text-caption font-bold flex items-center gap-2 border-b-2 transition-all whitespace-nowrap focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary ${
              isSelected
                ? 'border-primary text-primary bg-primary/5'
                : 'border-transparent text-inkMuted hover:text-ink hover:bg-surfaceRaised/50'
            }`}
          >
            <Icon className="w-4 h-4" />
            <span>{t(labelKey)}</span>
          </button>
        );
      })}
    </div>
  );
};
