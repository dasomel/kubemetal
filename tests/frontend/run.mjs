import { spawnSync } from 'node:child_process';
import { mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
// Keep emitted ESM under node_modules for dependency resolution without copying packages.
// A unique directory costs one compile per run; finally removes it even on failure.
const output = mkdtempSync(join(root, 'node_modules/.frontend-tests-'));
function run(command, args, env = process.env) {
  const result = spawnSync(command, args, { cwd: root, env, stdio: 'inherit' });
  if (result.error) throw result.error;
  return result.status ?? 1;
}

try {
  writeFileSync(join(output, 'package.json'), '{"type":"module"}\n');
  process.exitCode = run('pnpm', ['exec', 'tsc', '--noEmit', 'false',
    '--allowImportingTsExtensions', 'false', '--rootDir', 'src', '--outDir', output,
    '--noEmitOnError']);
  if (process.exitCode === 0) {
    const tests = readdirSync(join(root, 'tests/frontend'))
      .filter(name => name.endsWith('.test.mjs')).sort()
      .map(name => `tests/frontend/${name}`);
    process.exitCode = run(process.execPath, ['--test', ...tests], {
      ...process.env, FRONTEND_TEST_OUTPUT: pathToFileURL(`${output}/`).href,
    });
  }
} finally {
  rmSync(output, { recursive: true, force: true });
}
