import type { ModelDownloadStaging } from '../types/ipc';

export interface StagingSnapshot {
  inventory: ModelDownloadStaging | null;
  error: string | null;
}

// D53: never retain a stale active snapshot after a failed or superseded read.
// Cost: overlapping reads discard older results; a later refresh can retry.
export function createStagingReader(
  read: () => Promise<ModelDownloadStaging>,
  publish: (snapshot: StagingSnapshot) => void,
) {
  let generation = 0;
  let disposed = false;
  return {
    async refresh() {
      const request = ++generation;
      try {
        const inventory = await read();
        if (!disposed && request === generation) publish({ inventory, error: null });
      } catch (error) {
        if (!disposed && request === generation) publish({ inventory: null, error: String(error) });
      }
    },
    dispose() { disposed = true; ++generation; },
  };
}
