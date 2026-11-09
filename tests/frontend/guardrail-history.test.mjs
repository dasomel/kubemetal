// Run: make test-frontend (or pnpm test:frontend).
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { registerHooks } from 'node:module';
import { test } from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

// tsc preserves extensionless bundler imports; resolve only those emitted local files.
const output = process.env.FRONTEND_TEST_OUTPUT;
assert.ok(output, 'Run via make test-frontend or pnpm test:frontend');
registerHooks({ resolve(specifier, context, next) {
  if (specifier.startsWith('.') && context.parentURL?.startsWith(output)) {
    const candidate = new URL(`${specifier}.js`, context.parentURL);
    if (existsSync(candidate)) return next(candidate.href, context);
  }
  return next(specifier, context);
} });
const { observeGuardrailHistory, GuardrailEventHistoryView } = await import(new URL('components/mlx/GuardrailEventHistory.js', output));
const { I18nContext } = await import(new URL('i18n/i18nContext.js', output));
const { translations } = await import(new URL('i18n/translations.js', output));
const tick = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};
const event = (id, patch = {}) => ({
  correlation_id: `gr-1000-${id}`, timestamp: '2026-11-09T00:00:00Z',
  action: 'pause', reason: 'manual', pid: 123, details: null, ...patch,
});
function fixture() {
  const subscription = deferred(), load = deferred();
  const states = [];
  let receive, loads = 0, stops = 0;
  const dispose = observeGuardrailHistory({
    subscribe: callback => { receive = callback; return subscription.promise; },
    load: () => { loads++; return load.promise; },
  }, state => states.push(state));
  return { subscription, load, states, dispose, receive: e => receive(e),
    connect: () => subscription.resolve(() => stops++),
    get state() { return states.at(-1); }, get loads() { return loads; }, get stops() { return stops; } };
}

test('subscribes before snapshot, preserves in-flight events, deduplicates and sorts same-second IDs', async () => {
  const f = fixture();
  assert.equal(f.loads, 0);
  f.receive(event(1)); // delivery can precede listen() promise resolution
  f.connect(); await tick();
  assert.equal(f.loads, 1);
  f.receive(event(11, { action: 'resume' }));
  f.receive(event(2));
  f.load.resolve([event(2), event(10)]); await tick();
  assert.deepEqual(f.state.events.map(e => e.correlation_id), ['gr-1000-1', 'gr-1000-2', 'gr-1000-10', 'gr-1000-11']);
  assert.equal(f.state.loading, false);
  f.dispose(); assert.equal(f.stops, 1);
});

test('retains only newest 50 for both snapshot and live events', async () => {
  const f = fixture(); f.connect(); await tick();
  f.load.resolve(Array.from({ length: 55 }, (_, i) => event(i))); await tick();
  assert.equal(f.state.events.length, 50);
  assert.equal(f.state.events[0].correlation_id, 'gr-1000-5');
  f.receive(event(55, { timestamp: '2026-11-09T00:00:01Z' }));
  assert.equal(f.state.events[0].correlation_id, 'gr-1000-6');
  assert.equal(f.state.events.at(-1).correlation_id, 'gr-1000-55');
  f.dispose();
});

test('late listener registration after disposal is immediately unregistered without fetching', async () => {
  const f = fixture(); f.dispose(); const count = f.states.length;
  f.connect(); await tick(); f.receive(event(1));
  assert.equal(f.stops, 1); assert.equal(f.loads, 0); assert.equal(f.states.length, count);
});

test('late snapshot success/failure and live callbacks cannot overwrite a refreshed session', async () => {
  for (const fails of [false, true]) {
    const old = fixture(); old.connect(); await tick(); old.dispose();
    const count = old.states.length;
    const fresh = fixture(); fresh.connect(); await tick();
    fresh.load.resolve([event(9)]); await tick();
    old.receive(event(1));
    if (fails) old.load.reject('old error'); else old.load.resolve([event(1)]);
    await tick();
    assert.equal(old.states.length, count);
    assert.deepEqual(fresh.state.events, [event(9)]);
    fresh.dispose();
  }
});

test('subscription failure surfaces while snapshot can still succeed; disposal fences late rejection', async () => {
  const f = fixture(); f.subscription.reject('listen denied'); await tick();
  f.load.resolve([event(1)]); await tick();
  assert.equal(f.state.subscriptionError, 'listen denied');
  assert.equal(f.state.loadError, null); assert.equal(f.state.events.length, 1);
  f.dispose();
  const late = fixture(); late.dispose(); const count = late.states.length;
  late.subscription.reject('late'); await tick();
  assert.equal(late.states.length, count); assert.equal(late.loads, 0);
});

test('snapshot failure keeps received events and exposes failure rather than empty success', async () => {
  const f = fixture(); f.connect(); await tick(); f.receive(event(1));
  f.load.reject(new Error('IPC denied')); await tick();
  assert.equal(f.state.loadError, 'Error: IPC denied');
  assert.equal(f.state.loading, false); assert.deepEqual(f.state.events, [event(1)]);
  f.dispose();
});

function render(state, language = 'en') {
  const t = (key, params = {}) => Object.entries(params).reduce(
    (value, [name, replacement]) => value.replace(`{${name}}`, String(replacement)), translations[language][key] ?? key);
  return renderToStaticMarkup(React.createElement(I18nContext.Provider, { value: { t, language } },
    React.createElement(GuardrailEventHistoryView, { state, onRefresh() {} })));
}
const empty = { events: [], loading: false, loadError: null, subscriptionError: null };

test('renders loading, empty, individual and combined errors honestly in both languages', () => {
  for (const lang of ['en', 'ko']) {
    const text = key => translations[lang][`mlx.guardrail.history.${key}`];
    assert.ok(render(empty, lang).includes(text('empty')));
    assert.ok(render({ ...empty, loading: true }, lang).includes(text('loading')));
    assert.ok(!render({ ...empty, loading: true }, lang).includes(text('empty')));
    for (const patch of [{ loadError: 'read denied' }, { subscriptionError: 'listen denied' },
      { loadError: 'read denied', subscriptionError: 'listen denied' }]) {
      const html = render({ ...empty, ...patch }, lang);
      assert.ok(html.includes('role="alert"')); assert.ok(!html.includes(text('empty')));
      for (const value of Object.values(patch)) assert.ok(html.includes(value));
    }
  }
});

test('renders newest first, all fields, optional details, absent PID and unknown code fallbacks', () => {
  const events = [event(1), event(2, { action: 'resume', reason: 'battery', details: '<diagnostic>' }),
    event(3, { action: 'reconcile', reason: 'wake', pid: null }),
    event(4, { action: 'future-action', reason: 'future-reason' })];
  for (const lang of ['en', 'ko']) {
    const html = render({ ...empty, events }, lang);
    for (const e of events) assert.ok(html.includes(e.correlation_id));
    for (const action of ['pause', 'resume', 'reconcile']) {
      assert.ok(html.includes(translations[lang][`mlx.guardrail.history.${action}`]));
    }
    assert.ok(html.indexOf('gr-1000-4') < html.indexOf('gr-1000-1'));
    for (const value of ['2026-11-09T00:00:00Z', '123', 'future-action', 'future-reason', '&lt;diagnostic&gt;']) assert.ok(html.includes(value));
    assert.ok(html.includes(translations[lang]['mlx.guardrail.history.noPid']));
  }
  for (const lang of ['en', 'ko']) {
    for (const [reason, key] of Object.entries({ memory_pressure: 'mlx.causeMemoryPressure',
      thermal: 'mlx.causeThermal', battery: 'mlx.causeBattery', manual: 'mlx.guardrail.history.manual',
      wake: 'mlx.guardrail.history.wake', wake_error: 'mlx.guardrail.history.wakeError' })) {
      assert.ok(render({ ...empty, events: [event(1, { reason })] }, lang).includes(translations[lang][key]));
    }
  }
});
