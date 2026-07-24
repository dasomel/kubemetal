import React, { useState } from 'react';
import {
  Globe,
  FileText,
  Database,
  Scissors,
  Layers,
  CloudUpload,
  Play,
  RotateCcw,
  CheckCircle2,
  AlertTriangle,
  Loader2,
  Terminal,
  Settings2,
  ArrowRight,
  Sparkles,
  Check,
  ChevronDown,
  ChevronUp,
  HardDrive,
} from 'lucide-react';
import { useDataIngest } from '../../hooks/useDataIngest';
import { useTranslation } from '../../i18n/i18nContext';
import type { DataIngestSourceType, DagNodeId, DagNodeStatus } from '../../types/ipc';

const sourceOptions: { id: DataIngestSourceType; labelKey: string; icon: React.ElementType; example: string }[] = [
  { id: 'web', labelKey: 'dataIngest.sourceWeb', icon: Globe, example: 'https://docs.kubemetal.io' },
  { id: 'file', labelKey: 'dataIngest.sourceFile', icon: FileText, example: 'docs/' },
  { id: 'huggingface', labelKey: 'dataIngest.sourceHuggingface', icon: Database, example: 'wikitext/wikitext-2-raw-v1' },
];

const dagNodeDefinitions: {
  id: DagNodeId;
  titleKey: string;
  subtitle: string;
  icon: React.ElementType;
}[] = [
  {
    id: 'ingest',
    titleKey: 'dataIngest.nodeIngestTitle',
    subtitle: 'Web / File / HuggingFace',
    icon: Globe,
  },
  {
    id: 'clean_chunk',
    titleKey: 'dataIngest.nodeCleanChunkTitle',
    subtitle: 'Recursive Character Splitter',
    icon: Scissors,
  },
  {
    id: 'lancedb_store',
    titleKey: 'dataIngest.nodeLancedbTitle',
    subtitle: 'all-MiniLM-L6-v2 Embeddings',
    icon: Layers,
  },
  {
    id: 'dvc_backup',
    titleKey: 'dataIngest.nodeDvcBackupTitle',
    subtitle: 'S3 Remote Versioning',
    icon: CloudUpload,
  },
];

const getNodeIcon = (nodeId: DagNodeId, sourceType: DataIngestSourceType): React.ElementType => {
  if (nodeId === 'ingest') {
    if (sourceType === 'file') return FileText;
    if (sourceType === 'huggingface') return Database;
    return Globe;
  }
  const found = dagNodeDefinitions.find((n) => n.id === nodeId);
  return found ? found.icon : Database;
};

const SvgDagConnector: React.FC<{ fromStatus: DagNodeStatus; toStatus: DagNodeStatus; active: boolean }> = ({
  fromStatus,
  toStatus,
  active,
}) => {
  let strokeColor = 'var(--color-inkFaint)'; // default
  let isAnimated = false;

  if (fromStatus === 'success' && (toStatus === 'running' || active)) {
    strokeColor = 'var(--color-primary)';
    isAnimated = true;
  } else if (fromStatus === 'success' && toStatus === 'success') {
    strokeColor = 'var(--color-success)';
  } else if (toStatus === 'error' || fromStatus === 'error') {
    strokeColor = 'var(--color-danger)';
  }

  return (
    <div className="hidden md:flex items-center justify-center w-8 shrink-0 relative self-center">
      <svg className="w-full h-8 overflow-visible" viewBox="0 0 32 32" fill="none">
        <defs>
          <linearGradient id={`grad-${fromStatus}-${toStatus}`} x1="0%" y1="0%" x2="100%" y2="0%">
            <stop offset="0%" stopColor={fromStatus === 'success' ? 'var(--color-success)' : strokeColor} />
            <stop offset="100%" stopColor={strokeColor} />
          </linearGradient>
        </defs>
        <line
          x1="0"
          y1="16"
          x2="32"
          y2="16"
          stroke={`url(#grad-${fromStatus}-${toStatus})`}
          strokeWidth="2.5"
          strokeDasharray={isAnimated ? '6 4' : 'none'}
          className={isAnimated ? 'animate-[dash_1s_linear_infinite]' : ''}
        />
        <polygon
          points="26,11 32,16 26,21"
          fill={strokeColor}
        />
      </svg>
    </div>
  );
};

export const DataIngestionDagCard: React.FC = () => {
  const {
    config,
    setConfig,
    pipelineRun,
    activeNodeId,
    setActiveNodeId,
    isPipelineRunning,
    triggerPipelineRun,
    resetPipeline,
    datasets,
    loadingDatasets,
  } = useDataIngest();
  const { t } = useTranslation();

  const [showConfig, setShowConfig] = useState(true);
  const [copiedLog, setCopiedLog] = useState(false);

  const activeMetric = activeNodeId && pipelineRun?.nodes[activeNodeId] ? pipelineRun.nodes[activeNodeId] : null;

  const getStatusBadge = (status: DagNodeStatus, isCurrent: boolean) => {
    if (status === 'running' || isCurrent) {
      return (
        <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-caption font-medium bg-primary/10 text-primary border border-primary/20 animate-pulse">
          <Loader2 className="w-3 h-3 animate-spin shrink-0" />
          {t('dataIngest.statusRunning')}
        </span>
      );
    }
    if (status === 'success') {
      return (
        <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-caption font-medium bg-success/10 text-success border border-success/20">
          <CheckCircle2 className="w-3 h-3 shrink-0" />
          {t('dataIngest.statusSuccess')}
        </span>
      );
    }
    if (status === 'error') {
      return (
        <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-caption font-medium bg-danger/10 text-danger border border-danger/20">
          <AlertTriangle className="w-3 h-3 shrink-0" />
          {t('dataIngest.statusError')}
        </span>
      );
    }
    return (
      <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-caption font-medium bg-surfaceRaised text-inkFaint border border-hairline/20">
        {t('dataIngest.statusIdle')}
      </span>
    );
  };

  const handleSourceTypeChange = (type: DataIngestSourceType) => {
    const matched = sourceOptions.find((s) => s.id === type);
    setConfig({
      ...config,
      source_type: type,
      source_target: matched ? matched.example : config.source_target,
    });
  };

  const handleCopyLogs = () => {
    if (!activeMetric) return;
    const text = activeMetric.logs.join('\n');
    navigator.clipboard.writeText(text);
    setCopiedLog(true);
    setTimeout(() => setCopiedLog(false), 2000);
  };

  return (
    <div className="rounded-xl bg-surface p-5 shadow-panel border border-hairline/20 space-y-5">
      {/* Header & Main Control */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 pb-3 border-b border-hairline/10">
        <div>
          <div className="flex items-center gap-2">
            <Sparkles className="w-5 h-5 text-primary" />
            <h2 className="text-heading font-bold text-ink">{t('dataIngest.title')}</h2>
            {pipelineRun?.overall_status && (
              <span className="ml-2">{getStatusBadge(pipelineRun.overall_status, isPipelineRunning)}</span>
            )}
          </div>
          <p className="text-caption text-inkMuted mt-0.5">
            {t('dataIngest.subtitle')}
          </p>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          <button
            type="button"
            onClick={() => setShowConfig(!showConfig)}
            className="px-3 py-1.5 rounded-lg bg-surfaceRaised hover:brightness-95 text-caption text-ink font-medium flex items-center gap-1.5 border border-hairline/20"
          >
            <Settings2 className="w-4 h-4 text-inkMuted" />
            <span>{t('dataIngest.optionsBtn')}</span>
            {showConfig ? <ChevronUp className="w-3.5 h-3.5" /> : <ChevronDown className="w-3.5 h-3.5" />}
          </button>

          {pipelineRun && !isPipelineRunning && (
            <button
              type="button"
              onClick={resetPipeline}
              className="p-1.5 rounded-lg bg-surfaceRaised hover:brightness-95 text-inkMuted hover:text-ink border border-hairline/20"
              title={t('dataIngest.resetBtnTitle')}
            >
              <RotateCcw className="w-4 h-4" />
            </button>
          )}

          <button
            type="button"
            onClick={() => triggerPipelineRun()}
            disabled={isPipelineRunning || !config.source_target.trim()}
            className="px-4 py-1.5 rounded-lg bg-primary hover:bg-primary/90 disabled:opacity-50 text-white text-caption font-semibold flex items-center gap-2 shadow-sm transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            {isPipelineRunning ? (
              <>
                <Loader2 className="w-4 h-4 animate-spin" />
                <span>{t('dataIngest.runningBtn')}</span>
              </>
            ) : (
              <>
                <Play className="w-4 h-4 fill-current" />
                <span>{t('dataIngest.runBtn')}</span>
              </>
            )}
          </button>
        </div>
      </div>

      {/* Ingestion Config Panel */}
      {showConfig && (
        <div className="rounded-lg bg-surfaceRaised/50 p-4 border border-hairline/15 space-y-4">
          <div className="text-bodyStrong text-ink font-semibold flex items-center gap-1.5">
            <Settings2 className="w-4 h-4 text-primary" />
            <span>{t('dataIngest.configTitle')}</span>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
            {/* Source Type Selection Tabs */}
            <div className="md:col-span-3">
              <label className="block text-caption text-inkMuted mb-1.5 font-medium">{t('dataIngest.sourceTypeLabel')}</label>
              <div className="grid grid-cols-3 gap-2">
                {sourceOptions.map((opt) => {
                  const Icon = opt.icon;
                  const selected = config.source_type === opt.id;
                  return (
                    <button
                      key={opt.id}
                      type="button"
                      onClick={() => handleSourceTypeChange(opt.id)}
                      className={`flex items-center justify-center gap-2 px-3 py-2 rounded-lg text-caption font-medium border transition-all ${
                        selected
                          ? 'bg-primary/10 border-primary text-primary shadow-xs font-semibold'
                          : 'bg-surface border-hairline/20 text-inkMuted hover:text-ink hover:border-hairline/40'
                      }`}
                    >
                      <Icon className="w-4 h-4 shrink-0" />
                      <span>{t(opt.labelKey)}</span>
                    </button>
                  );
                })}
              </div>
            </div>

            {/* Target Input */}
            <div className="md:col-span-3">
              <label className="block text-caption text-inkMuted mb-1 font-medium">
                {config.source_type === 'web'
                  ? t('dataIngest.targetWebUrl')
                  : config.source_type === 'huggingface'
                  ? t('dataIngest.targetHfRepo')
                  : t('dataIngest.targetLocalDir')}
              </label>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={config.source_target}
                  onChange={(e) => setConfig({ ...config, source_target: e.target.value })}
                  placeholder={sourceOptions.find((s) => s.id === config.source_type)?.example}
                  className="flex-1 px-3 py-1.5 rounded-lg bg-surface border border-hairline/30 text-body text-ink focus:outline-none focus:border-primary font-mono text-caption"
                />
                <button
                  type="button"
                  onClick={() =>
                    setConfig({
                      ...config,
                      source_target: sourceOptions.find((s) => s.id === config.source_type)?.example || '',
                    })
                  }
                  className="px-2.5 py-1.5 rounded-lg bg-surface border border-hairline/20 text-caption text-inkMuted hover:text-ink hover:bg-surfaceRaised"
                >
                  {t('dataIngest.applyDefaultBtn')}
                </button>
              </div>
            </div>

            {/* Advanced Settings */}
            <div>
              <label className="block text-caption text-inkMuted mb-1 font-medium">{t('dataIngest.chunkSize')}</label>
              <input
                type="number"
                min={64}
                max={4096}
                step={64}
                value={config.chunk_size}
                onChange={(e) => setConfig({ ...config, chunk_size: parseInt(e.target.value) || 512 })}
                className="w-full px-3 py-1.5 rounded-lg bg-surface border border-hairline/30 text-body text-ink font-mono text-caption focus:outline-none focus:border-primary"
              />
            </div>

            <div>
              <label className="block text-caption text-inkMuted mb-1 font-medium">{t('dataIngest.chunkOverlap')}</label>
              <input
                type="number"
                min={0}
                max={512}
                step={16}
                value={config.chunk_overlap}
                onChange={(e) => setConfig({ ...config, chunk_overlap: parseInt(e.target.value) || 64 })}
                className="w-full px-3 py-1.5 rounded-lg bg-surface border border-hairline/30 text-body text-ink font-mono text-caption focus:outline-none focus:border-primary"
              />
            </div>

            <div>
              <label className="block text-caption text-inkMuted mb-1 font-medium">{t('dataIngest.collectionName')}</label>
              <input
                type="text"
                value={config.collection_name}
                onChange={(e) => setConfig({ ...config, collection_name: e.target.value })}
                className="w-full px-3 py-1.5 rounded-lg bg-surface border border-hairline/30 text-body text-ink font-mono text-caption focus:outline-none focus:border-primary"
              />
            </div>

            {/* Auto DVC Backup Checkbox */}
            <div className="md:col-span-3 flex items-center gap-2 pt-1">
              <input
                type="checkbox"
                id="autoDvcBackup"
                checked={config.auto_dvc_backup}
                onChange={(e) => setConfig({ ...config, auto_dvc_backup: e.target.checked })}
                className="w-4 h-4 rounded border-hairline text-primary focus:ring-primary accent-primary"
              />
              <label htmlFor="autoDvcBackup" className="text-caption text-ink font-medium cursor-pointer">
                {t('dataIngest.autoDvcBackup')}
              </label>
            </div>
          </div>
        </div>
      )}

      {/* DAG Node Flow Diagram */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <span className="text-bodyStrong text-ink font-semibold">{t('dataIngest.dagGraphTitle')}</span>
          <span className="text-caption text-inkFaint">{t('dataIngest.dagGraphHint')}</span>
        </div>

        <div className="flex flex-col md:flex-row items-stretch justify-between gap-2 md:gap-0 p-3 rounded-xl bg-surfaceRaised/40 border border-hairline/15 overflow-x-auto">
          {dagNodeDefinitions.map((node, idx) => {
            const nodeMetric = pipelineRun?.nodes[node.id];
            const nodeStatus = nodeMetric?.status || 'idle';
            const isSelected = activeNodeId === node.id;
            const isCurrentRunning = pipelineRun?.current_node === node.id && isPipelineRunning;

            const NodeIcon = getNodeIcon(node.id, config.source_type);

            const prevNodeStatus = idx > 0 ? pipelineRun?.nodes[dagNodeDefinitions[idx - 1].id]?.status || 'idle' : 'idle';

            return (
              <React.Fragment key={node.id}>
                {/* SVG Connector between nodes */}
                {idx > 0 && (
                  <SvgDagConnector
                    fromStatus={prevNodeStatus}
                    toStatus={nodeStatus}
                    active={isCurrentRunning}
                  />
                )}

                {/* Node Card */}
                <div
                  onClick={() => setActiveNodeId(node.id)}
                  className={`flex-1 min-w-[170px] p-3.5 rounded-xl border transition-all cursor-pointer select-none flex flex-col justify-between ${
                    isSelected
                      ? 'bg-surface border-primary shadow-md ring-2 ring-primary/20'
                      : nodeStatus === 'running' || isCurrentRunning
                      ? 'bg-surface border-primary/50 shadow-sm animate-pulse'
                      : nodeStatus === 'success'
                      ? 'bg-surface border-success/30 hover:border-success/60'
                      : nodeStatus === 'error'
                      ? 'bg-surface border-danger/40 hover:border-danger/60'
                      : 'bg-surface border-hairline/20 hover:border-hairline/50'
                  }`}
                >
                  <div className="flex items-start justify-between gap-1 mb-2">
                    <div
                      className={`p-2 rounded-lg shrink-0 ${
                        nodeStatus === 'success'
                          ? 'bg-success/10 text-success'
                          : nodeStatus === 'running' || isCurrentRunning
                          ? 'bg-primary/10 text-primary'
                          : nodeStatus === 'error'
                          ? 'bg-danger/10 text-danger'
                          : 'bg-surfaceRaised text-inkMuted'
                      }`}
                    >
                      <NodeIcon className="w-4 h-4" />
                    </div>
                    {getStatusBadge(nodeStatus, isCurrentRunning)}
                  </div>

                  <div>
                    <h4 className="text-bodyStrong text-ink font-semibold leading-snug truncate">{t(node.titleKey)}</h4>
                    <p className="text-caption text-inkFaint truncate mt-0.5">{node.subtitle}</p>
                  </div>

                  <div className="mt-3 pt-2 border-t border-hairline/10 flex items-center justify-between text-caption text-inkMuted">
                    <span>{t('dataIngest.processedItems')}</span>
                    <span className="font-mono font-semibold text-ink">
                      {t('dataIngest.itemsCount', { count: nodeMetric ? nodeMetric.items_processed : 0 })}
                    </span>
                  </div>
                </div>
              </React.Fragment>
            );
          })}
        </div>
      </div>

      {/* Selected Node Logs & Metrics Inspector */}
      {activeNodeId && (
        <div className="rounded-xl bg-surfaceRaised/60 p-4 border border-hairline/20 space-y-3">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 pb-2 border-b border-hairline/10">
            <div className="flex items-center gap-2">
              <Terminal className="w-4 h-4 text-primary" />
              <h3 className="text-bodyStrong text-ink font-semibold">
                {t('dataIngest.nodeLogsTitle')}{' '}
                <span className="text-primary font-bold">
                  {(() => {
                    const activeTitleKey = dagNodeDefinitions.find((n) => n.id === activeNodeId)?.titleKey;
                    return activeTitleKey ? t(activeTitleKey) : '';
                  })()}
                </span>
              </h3>
            </div>

            <div className="flex items-center gap-2">
              {activeMetric?.duration_ms ? (
                <span className="text-caption text-inkMuted bg-surface px-2.5 py-1 rounded-md border border-hairline/20 font-mono">
                  {t('dataIngest.durationMs')} <strong className="text-ink">{activeMetric.duration_ms}ms</strong>
                </span>
              ) : null}

              <button
                type="button"
                onClick={handleCopyLogs}
                disabled={!activeMetric || activeMetric.logs.length === 0}
                className="px-2.5 py-1 rounded-md bg-surface hover:brightness-95 text-caption text-inkMuted hover:text-ink border border-hairline/20 flex items-center gap-1"
              >
                {copiedLog ? <Check className="w-3.5 h-3.5 text-success" /> : <ArrowRight className="w-3.5 h-3.5" />}
                <span>{copiedLog ? t('dataIngest.copiedLogsBtn') : t('dataIngest.copyLogsBtn')}</span>
              </button>
            </div>
          </div>

          {/* Node Specific Details Grid */}
          {activeMetric?.details && (
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 text-caption bg-surface p-2.5 rounded-lg border border-hairline/15">
              {Object.entries(activeMetric.details).map(([key, val]) => (
                <div key={key} className="space-y-0.5">
                  <span className="text-inkFaint block text-[11px] uppercase tracking-wider">{key}</span>
                  <span className="font-mono text-ink font-medium break-all">{String(val)}</span>
                </div>
              ))}
            </div>
          )}

          {/* Log Console Window */}
          <div className="rounded-lg bg-ink p-3.5 text-inverse/80 font-mono text-caption min-h-[140px] max-h-[220px] overflow-y-auto space-y-1 shadow-inner border border-hairline/20">
            {activeMetric && activeMetric.logs.length > 0 ? (
              activeMetric.logs.map((log, i) => (
                <div key={i} className="flex gap-2 leading-relaxed break-words">
                  <span className="text-inverse/35 select-none shrink-0">{i + 1}</span>
                  <span
                    className={
                      log.includes('[오류]') || log.includes('[Error]')
                        ? 'text-danger font-semibold'
                        : log.includes('완료') || log.includes('성공') || log.includes('completed') || log.includes('Success')
                        ? 'text-success font-medium'
                        : log.includes('실행 중') || log.includes('시작') || log.includes('running')
                        ? 'text-primary'
                        : 'text-inverse/80'
                    }
                  >
                    {log}
                  </span>
                </div>
              ))
            ) : (
              <div className="text-inverse/40 italic py-6 text-center">{t('dataIngest.noLogs')}</div>
            )}
          </div>
        </div>
      )}

      {/* Ingested Datasets History */}
      <div className="rounded-xl bg-surfaceRaised/60 p-4 border border-hairline/20 space-y-2.5">
        <div className="flex items-center gap-2 pb-2 border-b border-hairline/10">
          <HardDrive className="w-4 h-4 text-primary" />
          <h3 className="text-bodyStrong text-ink font-semibold">{t('dataIngest.historyTitle')}</h3>
        </div>

        {loadingDatasets ? (
          <div className="flex items-center gap-2 text-caption text-inkMuted py-2">
            <Loader2 className="w-3.5 h-3.5 animate-spin" />
            <span>{t('dataIngest.historyLoading')}</span>
          </div>
        ) : datasets.length === 0 ? (
          <div className="text-caption text-inkFaint py-2">{t('dataIngest.historyEmpty')}</div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
            {datasets.map((ds) => (
              <div
                key={ds.collection_name}
                className="flex items-center justify-between gap-2 p-2.5 rounded-lg bg-surface border border-hairline/15 text-caption"
              >
                <div className="flex items-center gap-2 min-w-0">
                  <Database className="w-3.5 h-3.5 text-primary shrink-0" />
                  <span className="text-ink font-medium truncate">{ds.collection_name}</span>
                </div>
                <span className="text-inkFaint shrink-0">
                  {ds.is_lance_table ? 'LanceDB' : 'fallback JSON'}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};
