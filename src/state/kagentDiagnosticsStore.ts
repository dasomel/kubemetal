import { useSyncExternalStore } from 'react';
import type { KagentDiagnosticReport } from '../types/ipc';

/**
 * kagent 진단 결과의 최신 스냅샷을 모듈 레벨에서 브로드캐스트한다.
 * useDeployTarget.ts의 saveListeners 패턴(13-14, 55-61)을 일반화한 것 —
 * KagentOpsView가 이미 갖고 있는 fetchDiagnostics 성공 경로에 publish 호출을 얹을 뿐,
 * 이 스토어 자신은 폴링도 IPC 호출도 하지 않는다.
 */
export interface KagentDiagnosticsSnapshot {
  report: KagentDiagnosticReport;
  /** ISO 8601 — publish 호출 시점(이벤트 시점) 기록. */
  fetchedAtIso: string;
}

let snapshot: KagentDiagnosticsSnapshot | null = null;
const listeners = new Set<() => void>();

export function publishKagentDiagnostics(report: KagentDiagnosticReport, fetchedAtIso: string): void {
  snapshot = { report, fetchedAtIso };
  listeners.forEach((fn) => fn());
}

export function subscribeKagentDiagnostics(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

function getSnapshot(): KagentDiagnosticsSnapshot | null {
  return snapshot;
}

/** 탭을 방문한 적 없으면(=publish된 적 없으면) null — 실측 전 값을 지어내지 않는다(D22). */
export function useKagentDiagnosticsSnapshot(): KagentDiagnosticsSnapshot | null {
  return useSyncExternalStore(subscribeKagentDiagnostics, getSnapshot);
}
