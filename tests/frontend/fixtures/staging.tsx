// Local observation only. Generate .omc/logs/a1-staging.json from the Rust
// local_transfer_abort_then_restart test's A1_* JSON lines, then run pnpm dev.
import React from 'react';
import { createRoot } from 'react-dom/client';
import { ModelDownloadStagingCard } from '../../../src/components/modelhub/ModelDownloadStagingCard';
import { I18nProvider } from '../../../src/i18n/i18nContext';
import type { ModelDownloadStaging } from '../../../src/types/ipc';
import '../../../src/index.css';

const root = createRoot(document.getElementById('root')!);
try {
  const response = await fetch('/.omc/logs/a1-staging.json');
  if (!response.ok) throw new Error(`Fixture snapshot read: HTTP ${response.status}`);
  const snapshots: Record<string, ModelDownloadStaging> = await response.json();
  root.render(<I18nProvider><main className="bg-base text-ink p-4 space-y-4">
    <h1 className="text-heading">A1 — observed local-transfer snapshots (Rust filesystem fixture)</h1>
    {Object.entries(snapshots).map(([phase, inventory]) => <div key={phase}>
      <h2 className="text-heading">{phase}</h2>
      <ModelDownloadStagingCard inventory={inventory} error={null} onRefresh={() => window.location.reload()} />
    </div>)}
  </main></I18nProvider>);
} catch (error) {
  root.render(<p role="alert">{String(error)}</p>);
}
