import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Download, FileCheck, HardDrive, Package, RefreshCw, Shield, Sparkles } from 'lucide-react';
import { useTranslation } from '../../i18n/i18nContext';
import type { AirgapLatestVersionReport, AirgapStatusReport } from '../../types/ipc';
import { AirgapAssetTable, formatStorageSize } from './AirgapAssetTable';
import { AirgapVersionPanel } from './AirgapVersionPanel';

export const AirgapManagerView: React.FC = () => {
  const { t } = useTranslation();
  const [report, setReport] = useState<AirgapStatusReport | null>(null);
  const [loading, setLoading] = useState<boolean>(false);
  const [downloading, setDownloading] = useState<boolean>(false);
  const [installing, setInstalling] = useState<boolean>(false);
  const [checkingVersions, setCheckingVersions] = useState<boolean>(false);
  const [latestVersions, setLatestVersions] = useState<AirgapLatestVersionReport[] | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [msgIsError, setMsgIsError] = useState<boolean>(false);

  const report_ = (text: string, isError = false) => {
    setMsg(text);
    setMsgIsError(isError);
  };

  const fetchAirgapStatus = async () => {
    setLoading(true);
    try {
      setReport(await invoke<AirgapStatusReport>('get_airgap_status'));
    } catch (e) {
      setReport(null);
      report_(t('airgap.statusFailed', { error: String(e) }), true);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchAirgapStatus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 스크립트가 실제로 끝난 뒤 그 출력만 표시한다 — 완료 문구를 미리 띄우지 않는다.
  const handleDownloadBundle = async () => {
    setDownloading(true);
    report_(t('airgap.downloadRunning'));
    try {
      report_(await invoke<string>('trigger_airgap_download'));
      await fetchAirgapStatus();
    } catch (e) {
      report_(t('airgap.downloadFailed', { error: String(e) }), true);
    } finally {
      setDownloading(false);
    }
  };

  const handleInstallAirgap = async () => {
    setInstalling(true);
    report_(t('airgap.installRunning'));
    try {
      report_(await invoke<string>('trigger_airgap_install'));
    } catch (e) {
      report_(t('airgap.installFailed', { error: String(e) }), true);
    } finally {
      setInstalling(false);
    }
  };

  const handleCheckLatestVersions = async () => {
    setCheckingVersions(true);
    report_(t('airgap.versionChecking'));
    try {
      setLatestVersions(await invoke<AirgapLatestVersionReport[]>('check_latest_airgap_versions'));
      report_(t('airgap.versionChecked'));
    } catch (e) {
      report_(t('airgap.versionCheckFailed', { error: String(e) }), true);
    } finally {
      setCheckingVersions(false);
    }
  };

  const downloadedRatio =
    report && report.total_assets_count > 0
      ? Math.round((report.downloaded_count / report.total_assets_count) * 100)
      : 0;

  return (
    <div className="space-y-5">
      <div className="rounded-xl bg-surface p-5 shadow-panel flex flex-col md:flex-row md:items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <div className="p-2.5 rounded-xl bg-primary/10 text-primary shrink-0">
            <Package className="w-6 h-6" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h2 className="text-heading text-ink">{t('airgap.title')}</h2>
              <span className="px-2.5 py-0.5 rounded-full bg-primary/10 text-primary text-label font-bold uppercase">
                Offline Bundle
              </span>
            </div>
            <p className="text-caption text-inkMuted mt-0.5">{t('airgap.subtitle')}</p>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2.5 shrink-0">
          <button
            type="button"
            onClick={fetchAirgapStatus}
            disabled={loading}
            className="px-3 py-2 rounded-xl bg-surfaceRaised hover:brightness-95 text-primary text-caption font-bold flex items-center gap-1.5 transition-all"
          >
            <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
            <span>{t('airgap.refresh')}</span>
          </button>

          <button
            type="button"
            onClick={handleCheckLatestVersions}
            disabled={checkingVersions}
            className="px-3.5 py-2 rounded-xl bg-surfaceRaised hover:brightness-95 border border-hairline/10 text-ink text-caption font-bold flex items-center gap-1.5 transition-all"
          >
            <Sparkles className={`w-4 h-4 text-warning ${checkingVersions ? 'animate-spin' : ''}`} />
            <span>{checkingVersions ? t('airgap.checking') : t('airgap.checkLatest')}</span>
          </button>

          <button
            type="button"
            onClick={handleDownloadBundle}
            disabled={downloading}
            className="px-4 py-2 rounded-xl bg-surfaceRaised hover:brightness-95 border border-hairline/10 text-ink text-caption font-bold flex items-center gap-2 transition-all"
          >
            <Download className={`w-4 h-4 ${downloading ? 'animate-bounce' : ''}`} />
            <span>{downloading ? t('airgap.downloading') : t('airgap.downloadBtn')}</span>
          </button>

          <button
            type="button"
            onClick={handleInstallAirgap}
            disabled={installing}
            className="px-4 py-2 rounded-xl bg-primaryStrong hover:brightness-110 text-inverse text-caption font-bold flex items-center gap-2 transition-all shadow-sm"
          >
            <HardDrive className={`w-4 h-4 ${installing ? 'animate-spin' : ''}`} />
            <span>{installing ? t('airgap.installing') : t('airgap.installBtn')}</span>
          </button>
        </div>
      </div>

      {msg && (
        <div
          className={`px-4 py-2.5 rounded-xl text-caption break-words shadow-sm ${
            msgIsError
              ? 'bg-danger/10 border border-danger/20 text-danger font-mono'
              : 'bg-primary/10 border border-primary/20 text-primary'
          }`}
        >
          {msg}
        </div>
      )}

      {latestVersions && (
        <AirgapVersionPanel
          versions={latestVersions}
          downloading={downloading}
          onDownload={handleDownloadBundle}
        />
      )}

      {report && (
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <div className="rounded-xl bg-surface p-4 shadow-panel flex items-center justify-between border-l-4 border-l-primary">
            <div>
              <div className="text-caption text-inkFaint">{t('airgap.metricHeld')}</div>
              <div className="text-heading text-ink font-bold mt-1">
                {report.downloaded_count} / {report.total_assets_count} ({downloadedRatio}%)
              </div>
            </div>
            <div className="p-3 rounded-full bg-primary/10 text-primary">
              <FileCheck className="w-5 h-5" />
            </div>
          </div>

          <div className="rounded-xl bg-surface p-4 shadow-panel flex items-center justify-between border-l-4 border-l-success">
            <div>
              <div className="text-caption text-inkFaint">{t('airgap.metricSize')}</div>
              <div className="text-heading text-ink font-bold mt-1">
                {formatStorageSize(report.total_size_mb)}
              </div>
            </div>
            <div className="p-3 rounded-full bg-success/10 text-success">
              <HardDrive className="w-5 h-5" />
            </div>
          </div>

          <div className="rounded-xl bg-surface p-4 shadow-panel flex items-center justify-between border-l-4 border-l-warning">
            <div className="min-w-0">
              <div className="text-caption text-inkFaint">{t('airgap.metricPath')}</div>
              <div
                className="text-caption text-ink font-mono font-bold mt-1 truncate max-w-[200px]"
                title={report.airgap_dir}
              >
                {report.airgap_dir}
              </div>
            </div>
            <div className="p-3 rounded-full bg-warning/10 text-warning shrink-0">
              <Shield className="w-5 h-5" />
            </div>
          </div>
        </div>
      )}

      <AirgapAssetTable assets={report?.assets ?? []} />
    </div>
  );
};
