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
registerHooks({
  resolve(specifier, context, next) {
    if (specifier.startsWith('.') && context.parentURL?.startsWith(output)) {
      const candidate = new URL(`${specifier}.js`, context.parentURL);
      if (existsSync(candidate)) return next(candidate.href, context);
    }
    return next(specifier, context);
  },
});

const {
  createHealthSummaryController,
  initialHealthSummaryState,
} = await import(new URL('components/dashboard/healthSummaryState.js', output));
const { HealthSummaryView } = await import(new URL('components/dashboard/HealthSummaryPanel.js', output));
const { I18nContext } = await import(new URL('i18n/i18nContext.js', output));
const { translations } = await import(new URL('i18n/translations.js', output));

const tick = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
};

const makeSummary = (patch = {}) => ({
  colima: {
    is_running: true,
    kubernetes_active: true,
    mlflow_ready: true,
    seaweedfs_ready: true,
    artifact_store_wired: true,
  },
  colima_error: null,
  guardrails: {
    memory_pressure_level: 'normal',
    on_battery: false,
    battery_pause_enabled: true,
    training_paused: false,
    caffeinate_active: false,
    thermal_state: 'nominal',
    thermal_pause_enabled: true,
    resume_overrides: [],
    last_event: null,
  },
  guardrails_error: null,
  kagent: {
    target_context: 'colima',
    kagent_ready: true,
    kagent_installed: true,
    pod_issues_count: 0,
    recent_diagnosis: 'all ok',
    recommended_action: 'none',
    active_agents: ['k8s-agent'],
    available_agents: ['k8s-agent'],
  },
  kagent_error: null,
  overall: 'healthy',
  ...patch,
});

function render(state, language = 'en') {
  const t = (key, params = {}) =>
    Object.entries(params).reduce(
      (value, [name, replacement]) => value.replace(`{${name}}`, String(replacement)),
      translations[language][key] ?? key,
    );
  return renderToStaticMarkup(
    React.createElement(
      I18nContext.Provider,
      { value: { t, language } },
      React.createElement(HealthSummaryView, { state, onRefresh() {} }),
    ),
  );
}

function countOccurrences(haystack, needle) {
  let count = 0;
  let pos = 0;
  while ((pos = haystack.indexOf(needle, pos)) !== -1) {
    count++;
    pos += needle.length;
  }
  return count;
}

test('initial fetch failure surfaces error, leaves summary empty, and avoids false healthy/stale notices', async () => {
  const states = [];
  const source = {
    fetchSummary: () => Promise.reject(new Error('connection refused')),
  };
  const controller = createHealthSummaryController(source, s => states.push(s));
  assert.equal(states.length, 0);

  const fetchPromise = controller.fetch();
  assert.equal(controller.getState().loading, true);
  await fetchPromise;
  await tick();

  const state = controller.getState();
  assert.equal(state.loading, false);
  assert.equal(state.summary, null);
  assert.equal(state.error, 'Error: connection refused');
  assert.equal(state.fetchedAt, null);
  assert.equal(state.stale, false);

  for (const lang of ['ko', 'en']) {
    const html = render(state, lang);
    assert.ok(html.includes('role="alert"'));
    assert.ok(html.includes('Error: connection refused'));
    assert.ok(!html.includes(translations[lang]['health.overall.healthy']));
    assert.ok(!html.includes(translations[lang]['health.previousSnapshot']));
    assert.ok(!html.includes('bg-success'));
  }

  controller.dispose();
});

test('lifecycle: success -> failure -> retrying -> resuccess with timestamp retention and honest states', async () => {
  const states = [];
  let d = deferred();
  let currentTime = new Date('2026-11-12T10:00:00');
  const source = {
    fetchSummary: () => d.promise,
    now: () => currentTime,
  };
  const controller = createHealthSummaryController(source, s => states.push(s));

  // 1. Initial success
  void controller.fetch();
  assert.equal(controller.getState().loading, true);
  d.resolve(makeSummary());
  await tick();

  let state = controller.getState();
  assert.equal(state.loading, false);
  assert.equal(state.error, null);
  assert.equal(state.stale, false);
  assert.ok(state.summary !== null);
  const firstFetchedAt = state.fetchedAt;
  assert.ok(firstFetchedAt !== null);

  for (const lang of ['ko', 'en']) {
    const html = render(state, lang);
    assert.ok(html.includes(translations[lang]['health.overall.healthy']));
    assert.ok(html.includes(translations[lang]['health.probeSucceeded']));
    assert.ok(html.includes('bg-success'));
    assert.ok(html.includes('text-success'));
    assert.ok(!html.includes(translations[lang]['health.previousSnapshot']));
    assert.ok(!html.includes(translations[lang]['health.currentUnavailable']));
  }

  // 2. Refresh failure -> stale state
  d = deferred();
  currentTime = new Date('2026-11-12T10:01:00');
  void controller.fetch();
  assert.equal(controller.getState().loading, true);
  d.reject(new Error('apiserver timeout'));
  await tick();

  state = controller.getState();
  assert.equal(state.loading, false);
  assert.equal(state.error, 'Error: apiserver timeout');
  assert.equal(state.stale, true);
  assert.ok(state.summary !== null);
  // Preserves last successful fetchedAt time
  assert.equal(state.fetchedAt, firstFetchedAt);

  for (const lang of ['ko', 'en']) {
    const html = render(state, lang);
    assert.ok(html.includes('role="alert"'));
    assert.ok(html.includes('Error: apiserver timeout'));
    assert.ok(html.includes(translations[lang]['health.currentUnavailable']));
    // Preserved fetchedAt is rendered
    assert.ok(html.includes(firstFetchedAt));
    // D22: stale notice rendered exactly ONCE in both languages
    const snapshotText = translations[lang]['health.previousSnapshot'];
    assert.equal(countOccurrences(html, snapshotText), 1);
    // Absolute absence of success/healthy colors and verdicts
    assert.ok(!html.includes('bg-success'));
    assert.ok(!html.includes('text-success'));
    assert.ok(html.includes(`<span class="text-bodyStrong text-ink">${translations[lang]['health.currentUnavailable']}</span>`));
    assert.ok(!html.includes(`<span class="text-bodyStrong text-ink">${translations[lang]['health.overall.healthy']}</span>`));
    assert.ok(!html.includes(translations[lang]['health.probeSucceeded']));
  }

  // 3. Retry in progress while stale (must not flash green or clear error prematurely)
  d = deferred();
  currentTime = new Date('2026-11-12T10:02:00');
  void controller.fetch();
  state = controller.getState();
  assert.equal(state.loading, true);
  assert.equal(state.stale, true);
  assert.equal(state.error, 'Error: apiserver timeout');
  assert.equal(state.fetchedAt, firstFetchedAt);

  for (const lang of ['ko', 'en']) {
    const html = render(state, lang);
    assert.ok(!html.includes('bg-success'));
    assert.ok(!html.includes('text-success'));
    assert.ok(html.includes(`<span class="text-bodyStrong text-ink">${translations[lang]['health.currentUnavailable']}</span>`));
    assert.ok(!html.includes(`<span class="text-bodyStrong text-ink">${translations[lang]['health.overall.healthy']}</span>`));
    assert.equal(countOccurrences(html, translations[lang]['health.previousSnapshot']), 1);
  }

  // 4. Resuccess restores healthy verdicts and updates timestamp
  currentTime = new Date('2026-11-12T10:02:30');
  d.resolve(makeSummary({ overall: 'healthy' }));
  await tick();

  state = controller.getState();
  assert.equal(state.loading, false);
  assert.equal(state.error, null);
  assert.equal(state.stale, false);
  const secondFetchedAt = state.fetchedAt;
  assert.ok(secondFetchedAt !== null);

  for (const lang of ['ko', 'en']) {
    const html = render(state, lang);
    assert.ok(!html.includes('role="alert"'));
    assert.ok(html.includes(translations[lang]['health.overall.healthy']));
    assert.ok(html.includes(translations[lang]['health.probeSucceeded']));
    assert.ok(html.includes('bg-success'));
    assert.ok(html.includes('text-success'));
    assert.ok(!html.includes(translations[lang]['health.previousSnapshot']));
    assert.ok(!html.includes(translations[lang]['health.currentUnavailable']));
  }

  controller.dispose();
});

test('last successful fetchedAt is retained across multiple consecutive failures', async () => {
  let d = deferred();
  const time = new Date('2026-11-12T09:15:00');
  const source = { fetchSummary: () => d.promise, now: () => time };
  const controller = createHealthSummaryController(source, () => {});

  void controller.fetch();
  d.resolve(makeSummary());
  await tick();
  const baselineFetchedAt = controller.getState().fetchedAt;
  assert.ok(baselineFetchedAt !== null);

  // Failure 1
  d = deferred();
  void controller.fetch();
  d.reject(new Error('failure 1'));
  await tick();
  assert.equal(controller.getState().fetchedAt, baselineFetchedAt);

  // Failure 2
  d = deferred();
  void controller.fetch();
  d.reject(new Error('failure 2'));
  await tick();
  assert.equal(controller.getState().fetchedAt, baselineFetchedAt);

  controller.dispose();
});

test('superseded slow response cannot overwrite a newer refresh result', async () => {
  const req1 = deferred();
  const req2 = deferred();
  let calls = 0;
  const source = {
    fetchSummary: () => {
      calls++;
      return calls === 1 ? req1.promise : req2.promise;
    },
  };
  const controller = createHealthSummaryController(source, () => {});

  // Start request 1
  void controller.fetch();
  // Start request 2
  void controller.fetch();

  // Request 2 completes first with success
  req2.resolve(makeSummary({ overall: 'healthy' }));
  await tick();
  assert.equal(controller.getState().loading, false);
  assert.equal(controller.getState().error, null);

  // Request 1 completes later with failure
  req1.reject(new Error('slow failure'));
  await tick();
  // State must NOT be overwritten by request 1
  assert.equal(controller.getState().error, null);
  assert.equal(controller.getState().summary?.overall, 'healthy');

  controller.dispose();
});

test('disposal fences late resolution or rejection after unmount', async () => {
  const req = deferred();
  const updates = [];
  const source = { fetchSummary: () => req.promise };
  const controller = createHealthSummaryController(source, s => updates.push(s));

  void controller.fetch();
  const updatesBefore = updates.length;
  controller.dispose();

  req.resolve(makeSummary());
  await tick();
  assert.equal(updates.length, updatesBefore);

  const reqFail = deferred();
  const controller2 = createHealthSummaryController({ fetchSummary: () => reqFail.promise }, s => updates.push(s));
  void controller2.fetch();
  const updatesBefore2 = updates.length;
  controller2.dispose();

  reqFail.reject(new Error('unmount rejection'));
  await tick();
  assert.equal(updates.length, updatesBefore2);
});

test('component-level error in summary renders probeFailed correctly', () => {
  const summaryWithComponentErrors = makeSummary({
    colima: null,
    colima_error: 'colima daemon not running',
    kagent: null,
    kagent_error: 'kagent pods crashing',
    overall: 'degraded',
  });

  const state = {
    ...initialHealthSummaryState,
    summary: summaryWithComponentErrors,
  };

  for (const lang of ['ko', 'en']) {
    const html = render(state, lang);
    assert.ok(html.includes(translations[lang]['health.probeFailed']));
    assert.ok(html.includes('colima daemon not running'));
    assert.ok(html.includes('kagent pods crashing'));
    assert.ok(html.includes(translations[lang]['health.overall.degraded']));
  }
});
