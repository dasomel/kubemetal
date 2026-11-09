import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { GuardrailEvent } from '../../types/ipc';
import { useTranslation } from '../../i18n/i18nContext';

interface HistoryState {
  events: GuardrailEvent[];
  loading: boolean;
  loadError: string | null;
  subscriptionError: string | null;
}

interface HistorySource {
  load: () => Promise<GuardrailEvent[]>;
  subscribe: (receive: (event: GuardrailEvent) => void) => Promise<() => void>;
}

const initialState: HistoryState = {
  events: [], loading: true, loadError: null, subscriptionError: null,
};

function mergeEvents(events: GuardrailEvent[]): GuardrailEvent[] {
  return [...new Map(events.map(event => [event.correlation_id, event])).values()]
    .sort((a, b) => a.timestamp.localeCompare(b.timestamp)
      || a.correlation_id.localeCompare(b.correlation_id, undefined, { numeric: true }))
    .slice(-50);
}

// D22: failed reads remain errors. Subscribe before reading to close the snapshot gap;
// buffering costs at most 50 events, and disposal fences late refresh/unmount responses.
export function observeGuardrailHistory(source: HistorySource, publish: (state: HistoryState) => void): () => void {
  let disposed = false;
  let unlisten: (() => void) | undefined;
  let state = { ...initialState };
  const update = (patch: Partial<HistoryState>) => {
    if (disposed) return;
    state = { ...state, ...patch };
    publish(state);
  };
  update({});
  void (async () => {
    try {
      const stop = await source.subscribe(event => {
        update({ events: mergeEvents([...state.events, event]) });
      });
      if (disposed) {
        stop();
        return;
      }
      unlisten = stop;
    } catch (error) {
      update({ subscriptionError: String(error) });
    }
    if (disposed) return;
    try {
      const snapshot = await source.load();
      update({ events: mergeEvents([...snapshot, ...state.events]), loading: false });
    } catch (error) {
      update({ loadError: String(error), loading: false });
    }
  })();
  return () => {
    disposed = true;
    unlisten?.();
  };
}

const source: HistorySource = {
  load: () => invoke<GuardrailEvent[]>('get_guardrail_events'),
  subscribe: receive => listen<GuardrailEvent>('guardrail-event', event => receive(event.payload)),
};

export const GuardrailEventHistory: React.FC = () => {
  const [state, setState] = useState<HistoryState>(initialState);
  const [revision, setRevision] = useState(0);
  useEffect(() => observeGuardrailHistory(source, setState), [revision]);
  return <GuardrailEventHistoryView state={state} onRefresh={() => setRevision(value => value + 1)} />;
};

export const GuardrailEventHistoryView: React.FC<{ state: HistoryState; onRefresh: () => void }> = ({ state, onRefresh }) => {
  const { t } = useTranslation();
  const actions: Record<string, string> = {
    pause: t('mlx.guardrail.history.pause'),
    resume: t('mlx.guardrail.history.resume'),
    reconcile: t('mlx.guardrail.history.reconcile'),
  };
  const reasons: Record<string, string> = {
    manual: t('mlx.guardrail.history.manual'),
    memory_pressure: t('mlx.causeMemoryPressure'),
    battery: t('mlx.causeBattery'),
    thermal: t('mlx.causeThermal'),
    wake: t('mlx.guardrail.history.wake'),
    wake_error: t('mlx.guardrail.history.wakeError'),
  };

  return (
    <section className="mt-4 pt-3 border-t border-hairline/8" aria-label={t('mlx.guardrail.history.title')}>
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-bodyStrong text-ink">{t('mlx.guardrail.history.title')}</h3>
        <button type="button" onClick={onRefresh}
          className="p-2 rounded-md bg-surfaceRaised text-caption text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary">
          {t('mlx.guardrail.history.refresh')}
        </button>
      </div>
      <p className="mt-2 text-caption text-inkMuted">{t('mlx.guardrail.history.limit')}</p>
      {state.subscriptionError !== null && (
        <p role="alert" className="mt-2 text-caption text-danger break-words">
          {t('mlx.guardrail.history.subscriptionError', { error: state.subscriptionError })}
        </p>
      )}
      {state.loadError !== null && (
        <p role="alert" className="mt-2 text-caption text-danger break-words">
          {t('mlx.guardrail.history.loadError', { error: state.loadError })}
        </p>
      )}
      {state.loading && <p role="status" className="mt-2 text-caption text-inkMuted">{t('mlx.guardrail.history.loading')}</p>}
      {!state.loading && state.loadError === null && state.subscriptionError === null && state.events.length === 0 && (
        <p className="mt-2 text-caption text-inkMuted">{t('mlx.guardrail.history.empty')}</p>
      )}
      <ol className="mt-2 max-h-64 overflow-y-auto space-y-2" aria-label={t('mlx.guardrail.history.title')}>
        {[...state.events].reverse().map(event => (
          <li key={event.correlation_id} className="p-3 rounded-lg bg-surfaceRaised text-caption text-ink break-words">
            <time dateTime={event.timestamp} className="text-inkMuted">{event.timestamp}</time>
            <div>{t('mlx.guardrail.history.action')}: {actions[event.action] ?? event.action}</div>
            <div>{t('mlx.guardrail.history.reason')}: {reasons[event.reason] ?? event.reason}</div>
            <div>{t('mlx.guardrail.history.pid')}: {event.pid ?? t('mlx.guardrail.history.noPid')}</div>
            <div className="font-mono text-inkMuted">{t('mlx.guardrail.history.ref')}: {event.correlation_id}</div>
            {event.details && <p className="mt-2 text-inkMuted">{event.details}</p>}
          </li>
        ))}
      </ol>
    </section>
  );
};
