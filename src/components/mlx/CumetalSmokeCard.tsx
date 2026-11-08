import React, { useCallback, useEffect, useRef, useState } from 'react';
import { AlertTriangle, CheckCircle2, Loader2, Play, XCircle } from 'lucide-react';
import { runCumetalSmoke } from '../../hooks/useCumetal';
import { useTranslation } from '../../i18n/i18nContext';
import type { CumetalSmokeReport, CumetalStepResult, CumetalWorkloadResult } from '../../types/ipc';

// D51: clang CUDA registration/JIT smoke. Shows only what the backend measured (D22); the GPU claim
// comes from CuMetal's self-reported provenance, so an absent line is shown as unverified, never as success.
const stepTone = (status: CumetalStepResult['status']) =>
  status === 'ok' ? 'text-success' : status === 'skipped' ? 'text-inkMuted' : 'text-danger';

const StepLine: React.FC<{ label: string; step: CumetalStepResult }> = ({ label, step }) => {
  const { t } = useTranslation();
  return (
    <div className="min-w-0">
      <p className={`text-caption ${stepTone(step.status)}`}>
        {label}: {t(`mlx.cumetalSmoke.step.${step.status}`)}
        {step.exit_code !== null && ` · exit ${step.exit_code}`}
      </p>
      {step.status !== 'ok' && step.excerpt && (
        <pre className="whitespace-pre-wrap break-all text-caption text-inkMuted">{step.excerpt}</pre>
      )}
    </div>
  );
};

const WorkloadRow: React.FC<{ w: CumetalWorkloadResult }> = ({ w }) => {
  const { t } = useTranslation();
  const passed = w.verdict === 'passed';
  const parityTone = w.parity.status === 'pass' ? 'text-success' : w.parity.status === 'fail' ? 'text-danger' : 'text-warning';
  const provTone =
    w.provenance.status === 'verified' ? 'text-success' : w.provenance.status === 'failed' ? 'text-danger' : 'text-warning';
  return (
    <li className="rounded-lg bg-surfaceRaised p-3 space-y-1 min-w-0">
      <div className="flex items-center justify-between gap-2">
        <code className="text-caption text-ink">{w.name}</code>
        <span className={`text-bodyStrong flex items-center gap-1.5 ${passed ? 'text-success' : 'text-danger'}`}>
          {passed ? <CheckCircle2 className="w-4 h-4 shrink-0" /> : <XCircle className="w-4 h-4 shrink-0" />}
          {t(`mlx.cumetalSmoke.verdict.${w.verdict}`)}
        </span>
      </div>
      {w.verdict_reason !== null && (
        <p className="break-all text-caption text-inkMuted">
          {t('mlx.cumetal.reason')}: {w.verdict_reason}
        </p>
      )}
      <StepLine label="compile" step={w.compile} />
      <StepLine label="link" step={w.link} />
      <StepLine label="run" step={w.run} />
      <div className="min-w-0">
        <p className={`text-caption ${parityTone}`}>parity: {t(`mlx.cumetalSmoke.parity.${w.parity.status}`)}</p>
        {w.parity.metric_line !== null && (
          <code className="block break-all text-caption text-ink">{w.parity.metric_line}</code>
        )}
      </div>
      <div className="min-w-0">
        <p className={`text-caption ${provTone}`}>
          provenance: {t(`mlx.cumetalSmoke.provenance.${w.provenance.status}`)}
        </p>
        {w.provenance.launches.map((l, i) => (
          <code key={i} className="block break-all text-caption text-inkMuted">
            {`device=${l.device ?? '?'} device_name=${l.device_name ?? '?'} launch_success=${String(l.launch_success)} semantic_quality=${l.semantic_quality ?? '?'} source=${l.source ?? '?'}${l.unsupported_reason ? ` unsupported_reason=${l.unsupported_reason}` : ''}`}
          </code>
        ))}
      </div>
      <code className="block break-all text-caption text-inkFaint">sha256 {w.source_sha256}</code>
    </li>
  );
};

export const CumetalSmokeCard: React.FC = () => {
  const { t } = useTranslation();
  const [report, setReport] = useState<CumetalSmokeReport | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const run = useCallback(async () => {
    setRunning(true);
    setError(null);
    try {
      const next = await runCumetalSmoke();
      if (mountedRef.current) setReport(next);
    } catch (err) {
      if (mountedRef.current) setError(String(err));
    } finally {
      if (mountedRef.current) setRunning(false);
    }
  }, []);

  return (
    <section className="rounded-xl bg-surface p-4 shadow-panel" aria-live="polite">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div>
          <div className="text-label uppercase text-inkFaint mb-1">MLX</div>
          <h2 className="text-heading text-ink">{t('mlx.cumetalSmoke.title')}</h2>
        </div>
        <button
          type="button"
          onClick={() => void run()}
          disabled={running}
          className="px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 disabled:opacity-50 disabled:cursor-not-allowed text-ink text-caption flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          {running ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Play className="w-3.5 h-3.5" />}
          <span>{running ? t('mlx.cumetalSmoke.running') : t('mlx.cumetalSmoke.run')}</span>
        </button>
      </div>

      <p className="mb-3 text-caption text-inkMuted">{t('mlx.cumetalSmoke.description')}</p>

      {error && (
        <div role="alert" className="mb-3 flex items-start gap-1.5 text-caption text-danger">
          <AlertTriangle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
          <span className="break-all">{error}</span>
        </div>
      )}

      {report && report.state === 'unavailable' && (
        <div className="rounded-lg bg-surfaceRaised p-3 space-y-1 min-w-0">
          <span className="text-bodyStrong flex items-center gap-1.5 text-warning">
            <AlertTriangle className="w-4 h-4 shrink-0" />
            {t('mlx.cumetal.state.unavailable')}
          </span>
          <p className="break-all text-caption text-inkMuted">
            {t('mlx.cumetal.reason')}: {report.reason}
          </p>
        </div>
      )}

      {report && report.state === 'ran' && (
        <div className="space-y-2">
          <div className="rounded-lg bg-surfaceRaised p-3 min-w-0">
            <span className={`text-bodyStrong flex items-center gap-1.5 ${report.all_passed ? 'text-success' : 'text-danger'}`}>
              {report.all_passed ? <CheckCircle2 className="w-4 h-4 shrink-0" /> : <XCircle className="w-4 h-4 shrink-0" />}
              {report.all_passed ? t('mlx.cumetalSmoke.allPassed') : t('mlx.cumetalSmoke.notAllPassed')}
            </span>
          </div>
          <ul className="space-y-2">
            {report.workloads.map((w) => (
              <WorkloadRow key={w.name} w={w} />
            ))}
          </ul>
          {report.manifest && (
            <details className="rounded-lg bg-surfaceRaised p-3 min-w-0">
              <summary className="cursor-pointer text-caption text-inkMuted">{t('mlx.cumetalSmoke.manifest')}</summary>
              <dl className="mt-2 space-y-1 text-caption text-ink break-all">
                <div>cumetal: {report.manifest.cumetal_version ?? t('mlx.cumetal.notRun')}</div>
                <div>clang++: {report.manifest.clang_version ?? t('mlx.cumetal.notRun')}</div>
                <div>macOS: {report.manifest.macos_version ?? t('mlx.cumetal.notRun')}</div>
                <div>arch: {report.manifest.arch}</div>
                <div>
                  {t('mlx.cumetalSmoke.metalToolchain')}:{' '}
                  {report.manifest.metal_toolchain_present ? t('mlx.cumetalSmoke.present') : t('mlx.cumetalSmoke.absent')}
                </div>
                <div>prefix: {report.manifest.prefix}</div>
                <div>timestamp: {new Date(report.manifest.timestamp_unix * 1000).toISOString()}</div>
                {report.manifest.errors.map((e, i) => (
                  <div key={i} className="text-danger">
                    {e}
                  </div>
                ))}
              </dl>
            </details>
          )}
        </div>
      )}
    </section>
  );
};
