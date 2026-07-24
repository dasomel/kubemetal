import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { LayoutDashboard, Bot, Rocket, Database, Cpu, HardDrive, Key, ChevronLeft, ChevronRight, Package } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';
import type { HardwareSpec } from '../../types/ipc';

export type MainTab = 'dashboard' | 'kagent' | 'pipeline' | 'modelhub' | 'mlx' | 'data' | 'access' | 'airgap';

/** 색만으로 상태를 전달하지 않도록 `title`(툴팁 텍스트)을 항상 함께 받는다. */
export interface SidebarBadge {
  kind: 'dot' | 'count';
  tone: 'success' | 'warning' | 'danger';
  title: string;
  value?: number;
  pulse?: boolean;
}

export type SidebarBadges = Partial<Record<MainTab, SidebarBadge>>;

interface SidebarProps {
  activeTab: MainTab;
  setActiveTab: (tab: MainTab) => void;
  collapsed: boolean;
  setCollapsed: (collapsed: boolean | ((prev: boolean) => boolean)) => void;
  badges?: SidebarBadges;
}

const TONE_BG: Record<SidebarBadge['tone'], string> = {
  success: 'bg-success',
  warning: 'bg-warning',
  danger: 'bg-danger',
};

export const Sidebar: React.FC<SidebarProps> = ({
  activeTab,
  setActiveTab,
  collapsed,
  setCollapsed,
  badges = {},
}) => {
  const { t } = useTranslation();
  const [hwSpec, setHwSpec] = useState<HardwareSpec | null>(null);
  const [hwError, setHwError] = useState<string | null>(null);

  // 최초 앱 마운트 시 단 1회만 하드웨어 정적 스펙 실측 수집.
  // 실패해도 특정 기종 스펙을 가정해 채우지 않는다 — 조회 실패를 그대로 표시한다.
  useEffect(() => {
    invoke<HardwareSpec>('get_hardware_spec')
      .then((res) => setHwSpec(res))
      .catch((e) => setHwError(String(e)));
  }, []);

  const menuItems: { id: MainTab; label: string; icon: React.ElementType }[] = [
    { id: 'dashboard', label: t('tabs.dashboard'), icon: LayoutDashboard },
    { id: 'kagent', label: t('tabs.kagent'), icon: Bot },
    { id: 'pipeline', label: t('tabs.pipeline'), icon: Rocket },
    { id: 'modelhub', label: t('tabs.modelhub'), icon: Database },
    { id: 'mlx', label: t('tabs.mlx'), icon: Cpu },
    { id: 'data', label: t('tabs.data'), icon: HardDrive },
    { id: 'access', label: t('tabs.access'), icon: Key },
    { id: 'airgap', label: t('tabs.airgap'), icon: Package },
  ];

  return (
    <aside
      className={`h-screen sticky top-0 bg-surface border-r border-hairline/8 flex flex-col justify-between transition-all duration-200 z-30 ${
        collapsed ? 'w-16' : 'w-60'
      }`}
    >
      {/* 로고 & 상단 헤더 */}
      <div>
        <div className="h-16 px-4 flex items-center justify-between border-b border-hairline/8">
          {!collapsed ? (
            <div className="flex items-center gap-2.5">
              <div className="w-8 h-8 rounded-lg bg-primaryStrong text-inverse flex items-center justify-center font-bold text-bodyStrong shadow-sm">
                KM
              </div>
              <div>
                <h1 className="text-bodyStrong text-ink font-bold leading-tight">KubeMetal</h1>
                <span className="text-label text-inkFaint">Hybrid MLOps</span>
              </div>
            </div>
          ) : (
            <div className="w-8 h-8 rounded-lg bg-primaryStrong text-inverse flex items-center justify-center font-bold text-bodyStrong shadow-sm mx-auto">
              KM
            </div>
          )}

          <button
            type="button"
            onClick={() => setCollapsed((prev) => !prev)}
            className="p-1.5 rounded-md hover:bg-surfaceRaised text-inkMuted focus-visible:outline-none"
            aria-label={t('sidebar.toggle')}
          >
            {collapsed ? <ChevronRight className="w-4 h-4" /> : <ChevronLeft className="w-4 h-4" />}
          </button>
        </div>

        {/* 내비게이션 메뉴 목록 */}
        <nav className="p-2 space-y-1">
          {menuItems.map((item) => {
            const Icon = item.icon;
            const isActive = activeTab === item.id;
            const badge = badges[item.id];
            return (
              <button
                key={item.id}
                type="button"
                onClick={() => setActiveTab(item.id)}
                className={`relative w-full flex items-center gap-3 px-3 py-2.5 rounded-xl text-caption font-medium transition-all ${
                  isActive
                    ? 'bg-primary/10 text-primary font-bold shadow-xs'
                    : 'text-inkMuted hover:bg-surfaceRaised hover:text-ink'
                } ${collapsed ? 'justify-center px-0' : ''}`}
                title={collapsed ? `${item.label}${badge ? ` — ${badge.title}` : ''}` : undefined}
              >
                <Icon className={`w-5 h-5 shrink-0 ${isActive ? 'text-primary' : 'text-inkMuted'}`} />
                {!collapsed && (
                  <div className="flex items-center justify-between flex-1 min-w-0 gap-2">
                    <span className="truncate">{item.label}</span>
                    {badge?.kind === 'dot' && (
                      <span
                        title={badge.title}
                        className={`w-1.5 h-1.5 rounded-full shrink-0 ${TONE_BG[badge.tone]} ${
                          badge.pulse ? 'animate-pulse' : ''
                        }`}
                      />
                    )}
                    {badge?.kind === 'count' && (
                      <span
                        title={badge.title}
                        className={`min-w-[16px] h-4 px-1 rounded-full ${TONE_BG[badge.tone]} text-inverse text-[10px] leading-4 font-semibold flex items-center justify-center shrink-0`}
                      >
                        {badge.value}
                      </span>
                    )}
                  </div>
                )}
                {collapsed && badge && (
                  <span
                    className={`absolute right-2 top-2 w-1.5 h-1.5 rounded-full ${TONE_BG[badge.tone]}`}
                  />
                )}
              </button>
            );
          })}
        </nav>
      </div>

      {/* 하단 시스템 하드웨어 스펙 요약 바 */}
      {!collapsed && (
        <div className="p-3 m-2 rounded-xl bg-surfaceRaised border border-hairline/6 space-y-1 text-caption">
          <div className="flex items-center gap-1.5 text-ink font-semibold">
            <Cpu className="w-3.5 h-3.5 text-primary shrink-0" />
            <span className="truncate">
              {hwSpec ? hwSpec.brand_name : hwError ? t('sidebar.hwUnavailable') : t('sidebar.hwLoading')}
            </span>
          </div>
          {hwSpec && (
            <div className="text-inkFaint text-label">
              {hwSpec.cpu_cores}c CPU
              {hwSpec.gpu_cores !== null && ` / ${hwSpec.gpu_cores}c GPU`}
              {' / '}
              {hwSpec.total_memory_gb}GB RAM
            </div>
          )}
          {hwError && (
            <div className="text-danger text-label truncate" title={hwError}>
              {hwError}
            </div>
          )}
        </div>
      )}
    </aside>
  );
};
