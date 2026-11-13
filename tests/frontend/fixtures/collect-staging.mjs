// cargo test --locked --manifest-path src-tauri/Cargo.toml --lib local_transfer_abort_then_restart -- --skip live_smoke --skip live_matrix --nocapture > .omc/logs/a1-fixture.log 2>&1
// node tests/frontend/fixtures/collect-staging.mjs .omc/logs/a1-fixture.log
// pnpm dev --host 127.0.0.1; open /tests/frontend/fixtures/staging.html
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
const snapshots = Object.fromEntries(readFileSync(process.argv[2], 'utf8').split('\n')
  .filter(line => /^A1_(ACTIVE|UNKNOWN|PARTIAL|ERROR)=/.test(line))
  .map(line => {
    const separator = line.indexOf('=');
    return [line.slice(0, separator), JSON.parse(line.slice(separator + 1))];
  }));
assert.deepEqual(Object.keys(snapshots).sort(), ['A1_ACTIVE', 'A1_ERROR', 'A1_PARTIAL', 'A1_UNKNOWN']);
const directory = new URL('../../../.omc/logs/', import.meta.url);
mkdirSync(directory, { recursive: true });
writeFileSync(new URL('a1-staging.json', directory), JSON.stringify(snapshots, null, 2));
