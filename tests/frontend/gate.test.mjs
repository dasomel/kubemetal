import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';

for (const target of ['test-frontend', 'test', 'verify']) {
  test(`make ${target} includes frontend behavioral tests`, () => {
    const result = spawnSync('make', ['--dry-run', target], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /^pnpm test:frontend$/m);
  });
}
