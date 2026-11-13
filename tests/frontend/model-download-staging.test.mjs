import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { registerHooks } from 'node:module';
import { test } from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
const output = process.env.FRONTEND_TEST_OUTPUT;
assert.ok(output);
registerHooks({ resolve(specifier, context, next) {
  if (specifier.startsWith('.') && context.parentURL?.startsWith(output)) {
    const candidate = new URL(`${specifier}.js`, context.parentURL);
    if (existsSync(candidate)) return next(candidate.href, context);
  }
  return next(specifier, context);
} });
const { ModelDownloadStagingCard } = await import(new URL('components/modelhub/ModelDownloadStagingCard.js', output));
const { I18nContext } = await import(new URL('i18n/i18nContext.js', output));
const { translations } = await import(new URL('i18n/translations.js', output));
const empty = { state: 'observed', observed_at_unix_ms: 1000, entries: [], errors: [], truncated: false };
function render(inventory, error = null, language = 'en') {
  const t = (key, params = {}) => Object.entries(params).reduce((text, [k, v]) => text.replace(`{${k}}`, String(v)), translations[language][key]);
  return renderToStaticMarkup(React.createElement(I18nContext.Provider, { value: { language, t, setLanguage() {} } }, React.createElement(ModelDownloadStagingCard, { inventory, error, onRefresh() {} })));
}
for (const language of ['en', 'ko']) {
  test(`staging distinguishes loading, failure, empty and partial scans (${language})`, () => {
    const dict = translations[language];
    assert.ok(render(null, null, language).includes(dict['modelhub.staging.loading']));
    const failed = render(null, 'permission denied', language);
    assert.ok(failed.includes(dict['modelhub.staging.failed']) && failed.includes('permission denied'));
    assert.ok(!failed.includes(dict['modelhub.staging.empty']));
    assert.ok(render(empty, null, language).includes(dict['modelhub.staging.empty']));
    const partial = render({ ...empty, errors: ['root unreadable'], truncated: true }, null, language);
    assert.ok(partial.includes('root unreadable') && partial.includes(dict['modelhub.staging.truncated']));
    assert.ok(!partial.includes(dict['modelhub.staging.empty']));
  });
  test(`staging preserves unknown liveness, null size, errors and real zero (${language})`, () => {
    const html = render({ ...empty, entries: [
      { id: 'pid-reused', state: 'unknown', reason: 'no_durable_liveness_evidence', size_bytes: null, errors: ['symlink refused'] },
      { id: 'registered', state: 'active', reason: 'registered_attempt', size_bytes: 0, errors: [] },
    ] }, null, language);
    for (const key of ['unknown', 'active', 'sizeUnknown', 'partial', 'registered_attempt', 'no_durable_liveness_evidence']) {
      assert.ok(html.includes(translations[language][`modelhub.staging.${key}`]));
    }
    assert.ok(html.includes('symlink refused') && html.includes('1970-01-01T00:00:01.000Z'));
    assert.ok(!/delete|삭제하기|orphan/i.test(html));
  });
}

const { createStagingReader } = await import(new URL('lib/modelDownloadStaging.js', output));
test('reader clears previous active data on failure and can recover', async () => {
  const snapshots = [];
  let fail = false;
  const reader = createStagingReader(async () => {
    if (fail) throw new Error('denied');
    return empty;
  }, snapshot => snapshots.push(snapshot));
  await reader.refresh();
  assert.equal(snapshots.at(-1).inventory, empty);
  fail = true;
  await reader.refresh();
  assert.deepEqual(snapshots.at(-1), { inventory: null, error: 'Error: denied' });
  fail = false;
  await reader.refresh();
  assert.deepEqual(snapshots.at(-1), { inventory: empty, error: null });
});
test('reader discards stale successes, stale failures and unmounted results', async () => {
  const pending = [];
  const snapshots = [];
  const reader = createStagingReader(() => new Promise((resolve, reject) => pending.push({ resolve, reject })), value => snapshots.push(value));
  const old = reader.refresh();
  const current = reader.refresh();
  pending[1].resolve(empty);
  await current;
  pending[0].resolve({ ...empty, entries: [{ state: 'active' }] });
  await old;
  assert.deepEqual(snapshots, [{ inventory: empty, error: null }]);
  const staleFailure = reader.refresh();
  const newest = reader.refresh();
  pending[3].resolve(empty);
  await newest;
  pending[2].reject('old failure');
  await staleFailure;
  assert.equal(snapshots.length, 2);
  const unmounted = reader.refresh();
  reader.dispose();
  pending[4].resolve(empty);
  await unmounted;
  const unmountedFailure = reader.refresh();
  pending[5].reject('unmounted failure');
  await unmountedFailure;
  assert.equal(snapshots.length, 2);
});

for (const language of ['ko', 'en']) {
  test(`root states have distinct honest labels (${language})`, () => {
    const labels = language === 'ko'
      ? ['staging 디렉터리가 아직 생성되지 않았습니다.', 'staging 상태 알 수 없음', '관측된 staging 항목 없음']
      : ['Staging directory has not been created.', 'Staging state unknown', 'No staging entries observed'];
    for (const [index, state] of ['not_created', 'unknown', 'observed'].entries()) {
      const html = render({ ...empty, state }, null, language);
      assert.ok(html.includes(labels[index]), `${state}: ${labels[index]}`);
      for (const other of labels.filter((_, n) => n !== index)) assert.ok(!html.includes(other));
    }
    const html = render({ ...empty, truncated: true }, null, language);
    assert.ok(html.includes(language === 'ko' ? '목록이 불완전할 수 있습니다' : 'list may be incomplete'));
  });
}
