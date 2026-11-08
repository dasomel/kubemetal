import { invoke } from '@tauri-apps/api/core';
import type { CumetalSmokeReport, CumetalStatus } from '../types/ipc';

// D50: read-only CuMetal detection. Never installs, compiles or runs a CUDA workload.
export function getCumetalStatus(): Promise<CumetalStatus> {
  return invoke<CumetalStatus>('get_cumetal_status');
}

// D51: compiles and runs the bundled CUDA smoke workloads (clang registration/JIT path).
export function runCumetalSmoke(): Promise<CumetalSmokeReport> {
  return invoke<CumetalSmokeReport>('run_cumetal_smoke');
}
