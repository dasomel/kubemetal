import React, { useMemo, useState } from 'react';
import { Copy, Gauge, PlayCircle } from 'lucide-react';

type RuntimeKind = 'omlx' | 'mlx-lm';
type CacheState = 'cold' | 'warm' | 'ssd-restore' | 'unknown';

const inputClass =
  'w-full px-3 py-2 rounded-md bg-surfaceRaised text-ink text-caption border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary';
const buttonClass =
  'inline-flex items-center justify-center gap-1.5 px-3 py-2 rounded-md text-caption font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50 disabled:cursor-not-allowed';

const shellQuote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;

export const LocalInferenceBenchmarkCard: React.FC = () => {
  const [runtime, setRuntime] = useState<RuntimeKind>('omlx');
  const [endpoint, setEndpoint] = useState('http://127.0.0.1:8000');
  const [model, setModel] = useState('');
  const [modelRepo, setModelRepo] = useState('');
  const [modelRevision, setModelRevision] = useState('');
  const [modelDigest, setModelDigest] = useState('');
  const [quantization, setQuantization] = useState('');
  const [runtimePid, setRuntimePid] = useState('');
  const [cacheState, setCacheState] = useState<CacheState>('unknown');
  const [cacheDir, setCacheDir] = useState('~/.omlx/cache');
  const [requests, setRequests] = useState(8);
  const [concurrency, setConcurrency] = useState(1);
  const [stream, setStream] = useState(true);
  const [copied, setCopied] = useState(false);

  const command = useMemo(() => {
    const parts = [
      'python3 scripts/mlx/benchmark_local_inference.py',
      `--runtime ${runtime}`,
      `--endpoint ${shellQuote(endpoint)}`,
      model ? `--model ${shellQuote(model)}` : '--model <MODEL_ID>',
      `--requests ${Math.max(1, requests)}`,
      `--concurrency ${Math.max(1, concurrency)}`,
      `--cache-state ${cacheState}`,
      `--output ${shellQuote(`evidence/local-inference/${runtime}-${cacheState}-c${Math.max(1, concurrency)}.json`)}`,
    ];
    if (stream) parts.push('--stream');
    if (modelRepo) parts.push(`--model-repo ${shellQuote(modelRepo)}`);
    if (modelRevision) parts.push(`--model-revision ${shellQuote(modelRevision)}`);
    if (modelDigest) parts.push(`--model-digest ${shellQuote(modelDigest)}`);
    if (quantization) parts.push(`--quantization ${shellQuote(quantization)}`);
    if (runtimePid) parts.push(`--runtime-pid ${Number(runtimePid) || '<PID>'}`);
    if (cacheDir) parts.push(`--cache-dir ${shellQuote(cacheDir)}`);
    return parts.join(' \\\n  ');
  }, [runtime, endpoint, model, requests, concurrency, cacheState, stream, modelRepo, modelRevision, modelDigest, quantization, runtimePid, cacheDir]);

  const copy = async () => {
    await navigator.clipboard.writeText(command);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="rounded-xl bg-surface p-4 shadow-panel space-y-4">
      <div>
        <div className="text-label uppercase text-inkFaint mb-1">Benchmark</div>
        <h3 className="text-body font-medium text-ink flex items-center gap-2">
          <Gauge className="w-4 h-4 text-primary" /> Reproducible inference evidence
        </h3>
        <p className="text-caption text-inkMuted mt-1">
          Generates the exact offline-friendly benchmark command. Run the same model/prompt across mlx-lm and oMLX cold/warm/SSD-restore states; unsupported metrics stay null.
        </p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-4 gap-2">
        <label className="text-caption text-inkMuted">
          Runtime
          <select className={`${inputClass} mt-1`} value={runtime} onChange={(event) => setRuntime(event.target.value as RuntimeKind)}>
            <option value="omlx">oMLX</option>
            <option value="mlx-lm">mlx-lm</option>
          </select>
        </label>
        <label className="text-caption text-inkMuted md:col-span-2">
          Endpoint
          <input className={`${inputClass} mt-1`} value={endpoint} onChange={(event) => setEndpoint(event.target.value)} />
        </label>
        <label className="text-caption text-inkMuted">
          Runtime PID
          <input className={`${inputClass} mt-1`} inputMode="numeric" value={runtimePid} onChange={(event) => setRuntimePid(event.target.value)} placeholder="optional" />
        </label>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-2">
        <label className="text-caption text-inkMuted">
          Model ID
          <input className={`${inputClass} mt-1`} value={model} onChange={(event) => setModel(event.target.value)} placeholder="required at execution" />
        </label>
        <label className="text-caption text-inkMuted">
          Model repo
          <input className={`${inputClass} mt-1`} value={modelRepo} onChange={(event) => setModelRepo(event.target.value)} placeholder="owner/repo" />
        </label>
        <label className="text-caption text-inkMuted">
          Revision
          <input className={`${inputClass} mt-1`} value={modelRevision} onChange={(event) => setModelRevision(event.target.value)} />
        </label>
        <label className="text-caption text-inkMuted">
          Digest
          <input className={`${inputClass} mt-1`} value={modelDigest} onChange={(event) => setModelDigest(event.target.value)} />
        </label>
        <label className="text-caption text-inkMuted">
          Quantization
          <input className={`${inputClass} mt-1`} value={quantization} onChange={(event) => setQuantization(event.target.value)} placeholder="4bit / oQ4e / ..." />
        </label>
        <label className="text-caption text-inkMuted">
          Cache directory
          <input className={`${inputClass} mt-1`} value={cacheDir} onChange={(event) => setCacheDir(event.target.value)} />
        </label>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-4 gap-2">
        <label className="text-caption text-inkMuted">
          Requests
          <input className={`${inputClass} mt-1`} type="number" min={1} value={requests} onChange={(event) => setRequests(Number(event.target.value))} />
        </label>
        <label className="text-caption text-inkMuted">
          Concurrency
          <input className={`${inputClass} mt-1`} type="number" min={1} value={concurrency} onChange={(event) => setConcurrency(Number(event.target.value))} />
        </label>
        <label className="text-caption text-inkMuted">
          Cache state
          <select className={`${inputClass} mt-1`} value={cacheState} onChange={(event) => setCacheState(event.target.value as CacheState)}>
            <option value="unknown">unknown</option>
            <option value="cold">cold</option>
            <option value="warm">warm</option>
            <option value="ssd-restore">ssd-restore</option>
          </select>
        </label>
        <label className="inline-flex items-end gap-2 pb-2 text-caption text-inkMuted">
          <input type="checkbox" checked={stream} onChange={(event) => setStream(event.target.checked)} />
          Streaming / TTFT
        </label>
      </div>

      <pre className="rounded-md bg-surfaceRaised p-3 text-[11px] text-inkMuted overflow-x-auto whitespace-pre-wrap">{command}</pre>
      <div className="flex flex-wrap gap-2">
        <button className={`${buttonClass} bg-primaryStrong text-inverse`} onClick={copy}>
          <Copy className="w-3.5 h-3.5" /> {copied ? 'Copied' : 'Copy benchmark command'}
        </button>
        <span className="inline-flex items-center gap-1.5 text-caption text-inkFaint">
          <PlayCircle className="w-3.5 h-3.5" /> Suggested matrix: runtime × cold/warm/SSD × concurrency 1/2/4/8
        </span>
      </div>
    </div>
  );
};
