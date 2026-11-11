import { invoke } from '@tauri-apps/api/core';
import type { SystemHealthSummary } from '../../types/ipc';

export interface HealthSummaryState {
  summary: SystemHealthSummary | null;
  error: string | null;
  fetchedAt: string | null;
  loading: boolean;
  stale: boolean;
}

export interface HealthSummarySource {
  fetchSummary: () => Promise<SystemHealthSummary>;
  now?: () => Date;
}

export const initialHealthSummaryState: HealthSummaryState = {
  summary: null,
  error: null,
  fetchedAt: null,
  loading: false,
  stale: false,
};

export const defaultHealthSummarySource: HealthSummarySource = {
  fetchSummary: () => invoke<SystemHealthSummary>('get_system_health_summary'),
};

// D22 (#17): retain previous summary snapshot and fetchedAt timestamp on refresh failure.
// Only a successful response restores current verdicts and success colors, never a retry itself.
// Requests are serialized/fenced by requestId, and disposal fences late responses or unmounts.
export function createHealthSummaryController(
  source: HealthSummarySource,
  publish: (state: HealthSummaryState) => void,
  initialState: Partial<HealthSummaryState> = {},
) {
  let requestId = 0;
  let disposed = false;
  let state: HealthSummaryState = {
    ...initialHealthSummaryState,
    ...initialState,
  };

  const update = (patch: Partial<HealthSummaryState>) => {
    if (disposed) return;
    const nextError = patch.error !== undefined ? patch.error : state.error;
    const nextSummary = patch.summary !== undefined ? patch.summary : state.summary;
    state = {
      ...state,
      ...patch,
      stale: nextError !== null && nextSummary !== null,
    };
    publish(state);
  };

  const fetch = async () => {
    const currentId = ++requestId;
    update({ loading: true });
    try {
      const response = await source.fetchSummary();
      if (disposed || currentId !== requestId) return;
      const time = (source.now ? source.now() : new Date()).toLocaleTimeString();
      update({
        summary: response,
        error: null,
        fetchedAt: time,
        loading: false,
      });
    } catch (err) {
      if (disposed || currentId !== requestId) return;
      update({
        error: String(err),
        loading: false,
      });
    }
  };

  const dispose = () => {
    disposed = true;
    requestId += 1;
  };

  return {
    fetch,
    dispose,
    getState: () => state,
  };
}
