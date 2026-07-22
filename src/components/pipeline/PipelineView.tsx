import React, { useEffect } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { ChevronDown, ChevronRight, Server, Database, Cpu, Archive, Rocket, FlaskConical, ArrowUpRight } from 'lucide-react';
import { useColima } from '../../hooks/useColima';
import { useModelHub } from '../../hooks/useModelHub';
import { useMlx } from '../../hooks/useMlx';
import { useRegisteredModels } from '../../hooks/useRegisteredModels';
import { usePrefect } from '../../hooks/usePrefect';
import { OrchestrationCard } from './OrchestrationCard';
import { RagCard } from './RagCard';
import { DvcCard } from './DvcCard';

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
        <span>{stage.statusText}</span>
      </div>

      {stage.detail && <div className="text-caption text-inkFaint mb-1.5 break-words">{stage.detail}</div>}

      {stage.link && (
        <button
          type="button"
          onClick={() => openEndpoint(stage.link!.url)}
          className="mt-1 px-2.5 py-1 rounded-md bg-surfaceRaised hover:brightness-95 text-primary text-caption flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          {stage.link.label}
          <ArrowUpRight className="w-3 h-3" />
        </button>
      )}

      {stage.hint && <div className="text-caption text-inkFaint mt-2 pt-2 border-t border-hairline/8">{stage.hint}</div>}
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
        title: '인프라',
        dot: 'inkFaint',
        statusText: '대기',
        hint: '대시보드에서 클러스터 시작',
      };
    }
    if (!cluster.artifact_store_wired) {
      return {
        key: 'infra',
        icon: Server,
        title: '인프라',
        dot: 'warning',
        statusText: '기동됨 · 스택 연동 대기',
        detail: `MLflow ${cluster.mlflow_ready ? '준비' : '대기'} · SeaweedFS ${cluster.seaweedfs_ready ? '준비' : '대기'}`,
        hint: '대시보드에서 MLOps 스택 프로비저닝 및 포트포워딩 실행',
      };
    }
    return {
      key: 'infra',
      icon: Server,
      title: '인프라',
      dot: 'success',
      statusText: '준비됨',
      detail: 'K8s + MLflow + SeaweedFS 연동 완료',
    };
  })();

  // ② 모델 준비
  const activeDownloads = downloads.filter((d) => d.state === 'downloading');
  const modelPrepStage: StageInfo = (() => {
    if (activeDownloads.length > 0) {
      return {
        key: 'model',
        icon: Database,
        title: '모델 준비',
        dot: 'warning',
        statusText: `다운로드 진행 중 (${activeDownloads.length}개)`,
        detail: activeDownloads.map((d) => `${d.repo_id} ${d.done_files}/${d.total_files || '?'}`).join(', '),
      };
    }
    if (localModels.length === 0) {
      return {
        key: 'model',
        icon: Database,
        title: '모델 준비',
        dot: 'inkFaint',
        statusText: '대기',
        hint: '모델 허브에서 모델 다운로드',
      };
    }
    return {
      key: 'model',
      icon: Database,
      title: '모델 준비',
      dot: 'success',
      statusText: `로컬 모델 ${localModels.length}개`,
      detail: `총 ${bytesToHuman(localModels.reduce((sum, m) => sum + m.size_bytes, 0))}`,
    };
  })();

  // ③ 학습
  const training = mlxStatus?.training;
  const trainingStage: StageInfo = (() => {
    if (!training) {
      return {
        key: 'train',
        icon: Cpu,
        title: '학습',
        dot: 'inkFaint',
        statusText: '대기',
        hint: 'MLX 스튜디오에서 파인튜닝 시작',
      };
    }
    if (training.status === 'error') {
      return {
        key: 'train',
        icon: Cpu,
        title: '학습',
        dot: 'danger',
        statusText: '실패',
        detail: training.error,
      };
    }
    if (training.status === 'done') {
      return {
        key: 'train',
        icon: Cpu,
        title: '학습',
        dot: 'success',
        statusText: '완료',
        detail: training.adapter_path ? `어댑터: ${training.adapter_path}` : undefined,
      };
    }
    return {
      key: 'train',
      icon: Cpu,
      title: '학습',
      dot: 'warning',
      statusText: `진행 중 (PID ${training.pid})`,
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
        title: '등록',
        dot: 'danger',
        statusText: 'MLflow 접근 불가',
        hint: '대시보드에서 포트포워딩(5001)이 활성화되어 있는지 확인',
      };
    }
    if (registered.models.length === 0) {
      return {
        key: 'register',
        icon: Archive,
        title: '등록',
        dot: 'inkFaint',
        statusText: registered.loading && !registered.loaded ? '확인 중' : '대기',
        hint: '모델 허브에서 MLflow 등록',
      };
    }
    const latest = registered.models[registered.models.length - 1];
    return {
      key: 'register',
      icon: Archive,
      title: '등록',
      dot: 'success',
      statusText: `등록 모델 ${registered.models.length}개`,
      detail: `최신: ${latest.name}${latest.latest_version ? ` v${latest.latest_version}` : ''}`,
    };
  })();

  // ⑤ 서빙
  const serving = mlxStatus?.serving;
  const servingStage: StageInfo = serving
    ? {
        key: 'serve',
        icon: Rocket,
        title: '서빙',
        dot: 'success',
        statusText: `기동됨 (PID ${serving.pid}, 포트 ${serving.port})`,
        detail: serving.model_path,
        link: { label: `http://127.0.0.1:${serving.port}/v1`, url: `http://127.0.0.1:${serving.port}/v1/models` },
        hint: 'OpenAI 호환 base URL — 클릭 시 모델 목록으로 동작 확인',
      }
    : {
        key: 'serve',
        icon: Rocket,
        title: '서빙',
        dot: 'inkFaint',
        statusText: '중지',
        hint: 'MLX 스튜디오에서 서빙 시작',
      };

  // ⑥ 평가
  const evalStage: StageInfo =
    evalResults.length > 0
      ? {
          key: 'eval',
          icon: FlaskConical,
          title: '평가',
          dot: 'success',
          statusText: `최근 결과 ${Math.min(evalResults.length, 3)}건`,
          detail: evalResults
            .slice(0, 3)
            .map((m) => `${m.task} · ${m.metric} ${m.value.toFixed(3)}`)
            .join(' / '),
          link: { label: 'MLflow UI', url: 'http://localhost:5001' },
        }
      : {
          key: 'eval',
          icon: FlaskConical,
          title: '평가',
          dot: 'inkFaint',
          statusText: '평가 이력 없음',
          hint: '오케스트레이션에서 평가 실행',
        };

  const stages = [infraStage, modelPrepStage, trainingStage, registerStage, servingStage, evalStage];

  return (
    <div className="space-y-4">
      <div className="rounded-xl bg-surface p-4 shadow-panel">
        <h2 className="text-heading text-ink mb-0.5">파이프라인 가시화</h2>
        <p className="text-caption text-inkMuted">
          인프라 → 모델 준비 → 학습 → 등록 → 서빙까지 앱 내 오케스트레이션 상태를 한눈에 확인합니다.
        </p>
      </div>

      <OrchestrationCard />

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <RagCard />
        <DvcCard />
      </div>

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
          {registered.loading ? '등록 모델 새로고침 중...' : '등록 모델 새로고침'}
        </button>
      </div>
    </div>
  );
};
