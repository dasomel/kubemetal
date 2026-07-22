import React, { useEffect } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { ChevronDown, ChevronRight, Server, Database, Cpu, Archive, Rocket, FlaskConical, ArrowUpRight } from 'lucide-react';
import { useColima } from '../../hooks/useColima';
import { useModelHub } from '../../hooks/useModelHub';
import { useMlx } from '../../hooks/useMlx';
import { useRegisteredModels } from '../../hooks/useRegisteredModels';
import { usePrefect } from '../../hooks/usePrefect';
import { useTranslation } from '../../i18n/i18nContext';
import { OrchestrationCard } from './OrchestrationCard';

type DotColor = 'success' | 'warning' | 'danger' | 'inkFaint';

interface StageInfo {
  key: string;
  icon: React.ElementType;
  title: string;
  dot: DotColor;
  statusText: string;
  detail?: string;
  hint?: string;
  link?: { label: string; url: string };
}

const dotClass: Record<DotColor, string> = {
  success: 'bg-success',
  warning: 'bg-warning',
  danger: 'bg-danger',
  inkFaint: 'bg-inkFaint',
};

const openEndpoint = (url: string) => {
  openUrl(url).catch(() => window.open(url, '_blank'));
};

const StageCard: React.FC<{ stage: StageInfo }> = ({ stage }) => {
  const Icon = stage.icon;
  return (
    <div className="flex-1 rounded-xl bg-surface p-4 shadow-panel min-w-0">
      <div className="flex items-center gap-2 mb-3">
        <Icon className="w-4 h-4 text-primary shrink-0" />
        <h3 className="text-bodyStrong text-ink truncate">{stage.title}</h3>
      </div>

      <div className="flex items-center gap-1.5 text-caption text-inkMuted mb-1.5">
        <span className={`w-2 h-2 rounded-full shrink-0 ${dotClass[stage.dot]}`} />
        <span className="break-words leading-tight">{stage.statusText}</span>
      </div>

      {stage.detail && <div className="text-caption text-inkFaint mb-1.5 break-all leading-tight">{stage.detail}</div>}

      {stage.link && (
        <button
          type="button"
          onClick={() => openEndpoint(stage.link!.url)}
          className="mt-1 px-2.5 py-1 rounded-md bg-surfaceRaised hover:brightness-95 text-primary text-caption flex items-center gap-1 max-w-full text-left break-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          <span className="truncate">{stage.link.label}</span>
          <ArrowUpRight className="w-3 h-3 shrink-0" />
        </button>
      )}

      {stage.hint && <div className="text-caption text-inkFaint mt-2 pt-2 border-t border-hairline/8 break-words">{stage.hint}</div>}
    </div>
  );
};

const Connector: React.FC = () => (
  <div className="flex md:flex-col items-center justify-center text-inkFaint shrink-0 py-1 md:py-0 md:px-1">
    <ChevronDown className="w-5 h-5 md:hidden" />
    <ChevronRight className="w-5 h-5 hidden md:block" />
  </div>
);

const bytesToHuman = (bytes: number): string => {
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(0)}MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)}GB`;
};

export const PipelineView: React.FC = () => {
  const { status: cluster } = useColima();
  const { localModels, downloads } = useModelHub();
  const { mlxStatus } = useMlx();
  const registered = useRegisteredModels();
  const { evalResults } = usePrefect(false);
  const { t } = useTranslation();

  useEffect(() => {
    registered.refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ① 인프라
  const infraStage: StageInfo = (() => {
    if (!cluster?.is_running) {
      return {
        key: 'infra',
        icon: Server,
        title: t('pipeline.infraTitle'),
        dot: 'inkFaint',
        statusText: t('pipeline.infraWaiting'),
        hint: t('pipeline.infraHintDashboard'),
      };
    }
    if (!cluster.artifact_store_wired) {
      return {
        key: 'infra',
        icon: Server,
        title: t('pipeline.infraTitle'),
        dot: 'warning',
        statusText: t('pipeline.infraRunningPending'),
        detail: t('pipeline.infraDetailMlflowSeaweed', {
          mlflow: cluster.mlflow_ready ? 'Ready' : 'Not Ready',
          seaweed: cluster.seaweedfs_ready ? 'Ready' : 'Not Ready',
        }),
        hint: t('pipeline.infraHintProvision'),
      };
    }
    return {
      key: 'infra',
      icon: Server,
      title: t('pipeline.infraTitle'),
      dot: 'success',
      statusText: t('pipeline.infraReady'),
      detail: t('pipeline.infraReadyDetail'),
    };
  })();

  // ② 모델 준비
  const activeDownloads = downloads.filter((d) => d.state === 'downloading');
  const modelPrepStage: StageInfo = (() => {
    if (activeDownloads.length > 0) {
      return {
        key: 'model',
        icon: Database,
        title: t('pipeline.modelPrepTitle'),
        dot: 'warning',
        statusText: t('pipeline.modelPrepDownloading', { count: activeDownloads.length }),
        detail: activeDownloads.map((d) => `${d.repo_id} ${d.done_files}/${d.total_files || '?'}`).join(', '),
      };
    }
    if (localModels.length === 0) {
      return {
        key: 'model',
        icon: Database,
        title: t('pipeline.modelPrepTitle'),
        dot: 'inkFaint',
        statusText: t('pipeline.modelPrepWaiting'),
        hint: t('pipeline.modelPrepHintHub'),
      };
    }
    return {
      key: 'model',
      icon: Database,
      title: t('pipeline.modelPrepTitle'),
      dot: 'success',
      statusText: t('pipeline.modelPrepReady', { count: localModels.length }),
      detail: t('pipeline.modelPrepTotalSize', { size: bytesToHuman(localModels.reduce((sum, m) => sum + m.size_bytes, 0)) }),
    };
  })();

  // ③ 학습
  const training = mlxStatus?.training;
  const trainingStage: StageInfo = (() => {
    if (!training) {
      return {
        key: 'train',
        icon: Cpu,
        title: t('pipeline.trainTitle'),
        dot: 'inkFaint',
        statusText: t('pipeline.trainWaiting'),
        hint: t('pipeline.trainHintStudio'),
      };
    }
    if (training.status === 'error') {
      return {
        key: 'train',
        icon: Cpu,
        title: t('pipeline.trainTitle'),
        dot: 'danger',
        statusText: t('pipeline.trainFailed'),
        detail: training.error,
      };
    }
    if (training.status === 'done') {
      return {
        key: 'train',
        icon: Cpu,
        title: t('pipeline.trainTitle'),
        dot: 'success',
        statusText: t('pipeline.trainDone'),
        detail: training.adapter_path ? t('pipeline.trainAdapter', { path: training.adapter_path }) : undefined,
      };
    }
    return {
      key: 'train',
      icon: Cpu,
      title: t('pipeline.trainTitle'),
      dot: 'warning',
      statusText: t('pipeline.trainRunning', { pid: training.pid }),
      detail: `iter ${training.current_iter}/${training.total_iters}${
        training.last_loss != null ? ` · loss ${training.last_loss.toFixed(4)}` : ''
      }`,
    };
  })();

  // ④ 등록
  const registerStage: StageInfo = (() => {
    if (registered.error) {
      return {
        key: 'register',
        icon: Archive,
        title: t('pipeline.registerTitle'),
        dot: 'danger',
        statusText: t('pipeline.registerUnreachable'),
        hint: t('pipeline.registerHintPortForward'),
      };
    }
    if (registered.models.length === 0) {
      return {
        key: 'register',
        icon: Archive,
        title: t('pipeline.registerTitle'),
        dot: 'inkFaint',
        statusText: registered.loading && !registered.loaded ? t('pipeline.registerChecking') : t('pipeline.registerWaiting'),
        hint: t('pipeline.registerHintHub'),
      };
    }
    const latest = registered.models[registered.models.length - 1];
    return {
      key: 'register',
      icon: Archive,
      title: t('pipeline.registerTitle'),
      dot: 'success',
      statusText: t('pipeline.registerReady', { count: registered.models.length }),
      detail: t('pipeline.registerLatest', { name: `${latest.name}${latest.latest_version ? ` v${latest.latest_version}` : ''}` }),
    };
  })();

  // ⑤ 서빙
  const serving = mlxStatus?.serving;
  const servingStage: StageInfo = serving
    ? {
        key: 'serve',
        icon: Rocket,
        title: t('pipeline.servingTitle'),
        dot: 'success',
        statusText: t('pipeline.servingRunning', { pid: serving.pid, port: serving.port }),
        detail: serving.model_path,
        link: { label: `http://127.0.0.1:${serving.port}/v1`, url: `http://127.0.0.1:${serving.port}/v1/models` },
        hint: t('pipeline.servingBaseUrlHint'),
      }
    : {
        key: 'serve',
        icon: Rocket,
        title: t('pipeline.servingTitle'),
        dot: 'inkFaint',
        statusText: t('pipeline.servingStopped'),
        hint: t('pipeline.servingHintStudio'),
      };

  // ⑥ 평가
  const evalStage: StageInfo =
    evalResults.length > 0
      ? {
          key: 'eval',
          icon: FlaskConical,
          title: t('pipeline.evalTitle'),
          dot: 'success',
          statusText: t('pipeline.evalRecent', { count: Math.min(evalResults.length, 3) }),
          detail: evalResults
            .slice(0, 3)
            .map((m) => `${m.task} · ${m.metric} ${m.value.toFixed(3)}`)
            .join(' / '),
          link: { label: 'MLflow UI', url: 'http://localhost:5001' },
        }
      : {
          key: 'eval',
          icon: FlaskConical,
          title: t('pipeline.evalTitle'),
          dot: 'inkFaint',
          statusText: t('pipeline.evalNoHistory'),
          hint: t('pipeline.evalHintOrch'),
        };

  const stages = [infraStage, modelPrepStage, trainingStage, registerStage, servingStage, evalStage];

  return (
    <div className="space-y-4">
      <div className="rounded-xl bg-surface p-4 shadow-panel">
        <h2 className="text-heading text-ink mb-0.5">{t('pipeline.title')}</h2>
        <p className="text-caption text-inkMuted">
          {t('pipeline.subtitle')}
        </p>
      </div>

      <OrchestrationCard />

      <div className="flex flex-col md:flex-row items-stretch gap-1">
        {stages.map((stage, i) => (
          <React.Fragment key={stage.key}>
            <StageCard stage={stage} />
            {i < stages.length - 1 && <Connector />}
          </React.Fragment>
        ))}
      </div>

      <div className="flex justify-end">
        <button
          type="button"
          onClick={() => registered.refresh()}
          disabled={registered.loading}
          className="px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 disabled:opacity-50 text-caption text-inkMuted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          {registered.loading ? t('pipeline.refreshingModels') : t('pipeline.refreshModels')}
        </button>
      </div>
    </div>
  );
};
