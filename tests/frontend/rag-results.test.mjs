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

const { RagSearchResults } = await import(new URL('components/pipeline/RagSearchResults.js', output));
const { I18nContext } = await import(new URL('i18n/i18nContext.js', output));
const { translations } = await import(new URL('i18n/translations.js', output));

function render(props, language = 'en') {
  const t = (key, params = {}) => Object.entries(params).reduce(
    (value, [name, replacement]) => value.replace(new RegExp(`\\{${name}\\}`, 'g'), String(replacement)),
    translations[language][key] ?? translations.ko[key] ?? key
  );
  return renderToStaticMarkup(
    React.createElement(
      I18nContext.Provider,
      { value: { language, setLanguage() {}, t } },
      React.createElement(RagSearchResults, props)
    )
  );
}

test('renders null, empty, whitespace-only and valid source/text with proper fallbacks in ko and en', () => {
  for (const lang of ['en', 'ko']) {
    const unknownSource = translations[lang]['rag.unknownSource'];
    const unknownText = translations[lang]['rag.unknownText'];

    // 1) Null source and null text
    const nullHtml = render({
      searchResults: [{ id: 'null-1', source: null, text: null, mode: 'exact', score: 1.0 }],
      routing: null,
      hasSearched: true,
      searching: false,
    }, lang);
    assert.ok(nullHtml.includes(unknownSource), `Should display unknown source in ${lang}`);
    assert.ok(nullHtml.includes(unknownText), `Should display unknown text in ${lang}`);

    // 2) Empty string source and text
    const emptyHtml = render({
      searchResults: [{ id: 'empty-1', source: '', text: '', mode: 'exact', score: 1.0 }],
      routing: null,
      hasSearched: true,
      searching: false,
    }, lang);
    assert.ok(emptyHtml.includes(unknownSource));
    assert.ok(emptyHtml.includes(unknownText));

    // 3) Whitespace-only string source and text
    const wsHtml = render({
      searchResults: [{ id: 'ws-1', source: '   ', text: '   \n\t  ', mode: 'exact', score: 1.0 }],
      routing: null,
      hasSearched: true,
      searching: false,
    }, lang);
    assert.ok(wsHtml.includes(unknownSource));
    assert.ok(wsHtml.includes(unknownText));

    // 4) Valid non-empty source and text
    const validHtml = render({
      searchResults: [{ id: 'valid-1', source: '/docs/spec.md', text: 'Valid chunk content text', mode: 'dense', score: 0.9 }],
      routing: null,
      hasSearched: true,
      searching: false,
    }, lang);
    assert.ok(validHtml.includes('/docs/spec.md'));
    assert.ok(validHtml.includes('Valid chunk content text'));
    assert.ok(!validHtml.includes(unknownSource));
    assert.ok(!validHtml.includes(unknownText));
  }
});

test('exact mode renders metadata match badge and NEVER includes similarity percentage', () => {
  for (const lang of ['en', 'ko']) {
    const exactBadge = translations[lang]['rag.exactMatch'];
    const similarityLabel = translations[lang]['rag.similarity'];

    const html = render({
      searchResults: [{ id: 'exact-1', source: 'docs/test.md', text: 'Exact match content', mode: 'exact', score: 1.0 }],
      routing: null,
      hasSearched: true,
      searching: false,
    }, lang);

    assert.ok(html.includes(exactBadge), `Expected ${exactBadge} in ${lang}`);
    assert.ok(!html.includes(similarityLabel), `Should not contain similarity label in exact mode (${lang})`);
    assert.ok(!html.includes('%'), `Should not contain '%' in exact mode (${lang})`);
  }
});

test('lexical, hybrid, and dense scores and provenance are correctly formatted', () => {
  for (const lang of ['en', 'ko']) {
    // Lexical: score formatted via toExponential(2)
    const lexScoreLabel = translations[lang]['rag.score'];
    const lexHtml = render({
      searchResults: [{ id: 'lex-1', source: 'doc.md', text: 'Lex hit', mode: 'lexical', score: 0.00123 }],
      routing: null,
      hasSearched: true,
      searching: false,
    }, lang);
    assert.ok(lexHtml.includes(lexScoreLabel));
    assert.ok(lexHtml.includes('1.23e-3'));
    assert.ok(!lexHtml.includes('%'));

    // Dense: score formatted as percentage with 1 decimal
    const denseSimilarityLabel = translations[lang]['rag.similarity'];
    const denseHtml = render({
      searchResults: [{ id: 'dense-1', source: 'doc.md', text: 'Dense hit', mode: 'dense', score: 0.856 }],
      routing: null,
      hasSearched: true,
      searching: false,
    }, lang);
    assert.ok(denseHtml.includes(denseSimilarityLabel));
    assert.ok(denseHtml.includes('85.6%'));

    // Hybrid: RRF score formatted to 4 decimals + provenance breakdown
    const rrfScoreLabel = translations[lang]['rag.rrfScore'];
    const notHitLabel = translations[lang]['rag.notHit'];
    const hybFullHtml = render({
      searchResults: [{
        id: 'hyb-1',
        source: 'doc.md',
        text: 'Hybrid hit',
        mode: 'hybrid',
        score: 0.03333,
        provenance: {
          retrievers: ['lexical', 'dense'],
          lexical: { rank: 1, score: 0.0025 },
          dense: { rank: 2, score: 0.8123 },
        },
      }],
      routing: null,
      hasSearched: true,
      searching: false,
    }, lang);
    assert.ok(hybFullHtml.includes(rrfScoreLabel));
    assert.ok(hybFullHtml.includes('0.0333'));
    assert.ok(hybFullHtml.includes('lexical, dense'));
    assert.ok(hybFullHtml.includes('#1 (2.50e-3)'));
    assert.ok(hybFullHtml.includes('#2 (0.8123)'));

    // Hybrid with lexical only hit
    const hybLexHtml = render({
      searchResults: [{
        id: 'hyb-2',
        source: 'doc.md',
        text: 'Hybrid lex hit',
        mode: 'hybrid',
        score: 0.0166,
        provenance: {
          retrievers: ['lexical'],
          lexical: { rank: 1, score: 0.005 },
        },
      }],
      routing: null,
      hasSearched: true,
      searching: false,
    }, lang);
    assert.ok(hybLexHtml.includes('#1 (5.00e-3)'));
    assert.ok(hybLexHtml.includes(notHitLabel), `Expected notHit label for dense in ${lang}`);

    // Hybrid with dense only hit
    const hybDenseHtml = render({
      searchResults: [{
        id: 'hyb-3',
        source: 'doc.md',
        text: 'Hybrid dense hit',
        mode: 'hybrid',
        score: 0.0166,
        provenance: {
          retrievers: ['dense'],
          dense: { rank: 1, score: 0.95 },
        },
      }],
      routing: null,
      hasSearched: true,
      searching: false,
    }, lang);
    assert.ok(hybDenseHtml.includes('#1 (0.9500)'));
    assert.ok(hybDenseHtml.includes(notHitLabel), `Expected notHit label for lexical in ${lang}`);
  }
});

test('shows routing info and empty notice when zero results return after search', () => {
  for (const lang of ['en', 'ko']) {
    const noResultsText = translations[lang]['rag.noResults'];
    const routing = { resolved_mode: 'lexical', rule: 'single_token' };

    const html = render({
      searchResults: [],
      routing,
      hasSearched: true,
      searching: false,
    }, lang);

    assert.ok(html.includes(noResultsText), `Expected empty notice in ${lang}`);
    assert.ok(html.includes('lexical'), `Expected resolved mode in ${lang}`);
    assert.ok(html.includes('single_token'), `Expected routing rule in ${lang}`);
  }
});

test('never displays empty results notice or routing before search is executed', () => {
  for (const lang of ['en', 'ko']) {
    const noResultsText = translations[lang]['rag.noResults'];

    // Initial state: hasSearched is false
    const initialHtml = render({
      searchResults: [],
      routing: null,
      hasSearched: false,
      searching: false,
    }, lang);
    assert.ok(!initialHtml.includes(noResultsText), `Should not show noResults before search (${lang})`);

    // In-flight searching state: searching is true
    const searchingHtml = render({
      searchResults: [],
      routing: null,
      hasSearched: true,
      searching: true,
    }, lang);
    assert.ok(!searchingHtml.includes(noResultsText), `Should not show noResults while searching (${lang})`);
  }
});

test('handles null id gracefully and renders header with result count', () => {
  for (const lang of ['en', 'ko']) {
    const headerPrefix = lang === 'ko' ? '유사도 검색 결과 (2건)' : 'Similarity Search Results (2)';

    const html = render({
      searchResults: [
        { id: null, source: 'doc1.md', text: 'First item', mode: 'exact', score: 1.0 },
        { id: null, source: 'doc2.md', text: 'Second item', mode: 'dense', score: 0.9 },
      ],
      routing: null,
      hasSearched: true,
      searching: false,
    }, lang);

    assert.ok(html.includes(headerPrefix), `Expected header count in ${lang}`);
    assert.ok(html.includes('doc1.md'));
    assert.ok(html.includes('doc2.md'));
  }
});
