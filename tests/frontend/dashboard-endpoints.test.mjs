// Run: make test-frontend (or pnpm test:frontend).
import assert from 'node:assert/strict';
import { test } from 'node:test';

const output = process.env.FRONTEND_TEST_OUTPUT;
assert.ok(output, 'Run via make test-frontend or pnpm test:frontend');
const { dashboardEndpoints } = await import(new URL('lib/dashboardEndpoints.js', output));

const svc = (service, url) => ({ service, url, health: 'ok', credential_hint: null, credentials: [] });

test('dashboard shortcuts drop the browser-blank S3 API and keep the rest in order', () => {
  const services = [
    svc('MLflow', 'http://127.0.0.1:5001'),
    svc('SeaweedFS S3 API', 'http://127.0.0.1:8333'),
    svc('SeaweedFS Filer UI', 'http://127.0.0.1:8888'),
    svc('Model Serving', ''),
  ];
  assert.deepEqual(dashboardEndpoints(services).map((s) => s.service),
    ['MLflow', 'SeaweedFS Filer UI', 'Model Serving']);
  // Input is not mutated: the Access console and forwarding health still see S3.
  assert.equal(services.length, 4);
});

test('S3-only list is empty so the panel shows its unknown-endpoints placeholder', () => {
  assert.deepEqual(dashboardEndpoints([svc('SeaweedFS S3 API', 'http://127.0.0.1:8333')]), []);
});
