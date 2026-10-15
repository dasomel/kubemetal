import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AdapterResumeSource, TrainingCheckpointEvent } from '../types/ipc';

export function useTrainingCheckpoints(trainingStatus?: string) {
  const [sources, setSources] = useState<AdapterResumeSource[]>([]);
  const [sourceError, setSourceError] = useState('');
  const [lastCheckpoint, setLastCheckpoint] = useState<TrainingCheckpointEvent | null>(null);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen<TrainingCheckpointEvent>('mlx-training-checkpoint', ({ payload }) => {
      if (!disposed) setLastCheckpoint(payload);
    }).then((stop) => { if (disposed) stop(); else unlisten = stop; })
      .catch((error: unknown) => { if (!disposed) setSourceError(String(error)); });
    return () => { disposed = true; unlisten?.(); };
  }, []);
  useEffect(() => {
    let active = true;
    const fetchSources = () => {
      invoke<AdapterResumeSource[]>('list_adapter_resume_sources').then((rows) => {
        if (active) { setSources(rows); setSourceError(''); }
      }).catch((error: unknown) => { if (active) setSourceError(String(error)); });
    };
    fetchSources();
    // D49: stop intent reaches the UI before the child exits and its record settles.
    // Cost: read-only terminal polling; escape hatch: refresh by reopening this tab.
    const timer = trainingStatus === 'killed' || trainingStatus === 'error'
      ? window.setInterval(fetchSources, 3000)
      : undefined;
    return () => { active = false; window.clearInterval(timer); };
  }, [trainingStatus]);
  return { sources, sourceError, lastCheckpoint };
}
