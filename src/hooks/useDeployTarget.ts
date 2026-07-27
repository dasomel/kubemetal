import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { BridgeState, DeployTarget, PreflightReport } from '../types/ipc';

export const COLIMA_CONTEXT = 'colima';

/**
 * 배포 대상(D26) 선택·사전점검·브리지 탐지.
 *
 * 이 훅은 실패를 성공으로 바꾸지 않는다 — 조회가 실패하면 에러 문자열을 그대로 노출하고,
 * 사전점검 결과나 브리지 상태를 낙관적 기본값으로 채우지 않는다(D22–D25).
 */
export function useDeployTarget() {
  const [target, setTarget] = useState<DeployTarget | null>(null);
  const [contexts, setContexts] = useState<string[]>([]);
  const [contextsError, setContextsError] = useState<string | null>(null);
  const [preflight, setPreflight] = useState<PreflightReport | null>(null);
  const [preflightError, setPreflightError] = useState<string | null>(null);
  const [busy, setBusy] = useState<null | 'preflight' | 'bridge' | 'save'>(null);

  const loadTarget = useCallback(async () => {
    try {
      setTarget(await invoke<DeployTarget>('get_deploy_target'));
    } catch (err) {
      console.error('배포 대상 로드 오류:', err);
    }
  }, []);

  const loadContexts = useCallback(async () => {
    try {
      setContexts(await invoke<string[]>('list_kubeconfig_contexts'));
      setContextsError(null);
    } catch (err) {
      // 컨텍스트 목록을 지어내지 않는다 — 조회 실패는 실패로 보여준다(D22).
      setContexts([]);
      setContextsError(String(err));
    }
  }, []);

  useEffect(() => {
    void loadTarget();
    void loadContexts();
  }, [loadTarget, loadContexts]);

  /** 컨텍스트를 바꾸면 이전 대상의 사전점검·브리지 검증 결과는 무효다. */
  const selectContext = useCallback((context: string) => {
    setPreflight(null);
    setPreflightError(null);
    setTarget({
      context,
      namespace: context === COLIMA_CONTEXT ? 'default' : 'kubemetal',
      storage_class: null,
      image_registry: null,
      bridge:
        context === COLIMA_CONTEXT
          ? { kind: 'keep_base' }
          : { kind: 'unverified', candidates: [], reason: '아직 탐지·검증하지 않았습니다.' },
    });
  }, []);

  const patchTarget = useCallback((patch: Partial<DeployTarget>) => {
    setTarget((prev) => (prev ? { ...prev, ...patch } : prev));
  }, []);

  const runPreflight = useCallback(async () => {
    if (!target) return;
    setBusy('preflight');
    setPreflightError(null);
    try {
      setPreflight(
        await invoke<PreflightReport>('preflight_deploy_target', {
          context: target.context,
          namespace: target.namespace,
        }),
      );
    } catch (err) {
      setPreflight(null);
      setPreflightError(String(err));
    } finally {
      setBusy(null);
    }
  }, [target]);

  const detectBridge = useCallback(async () => {
    if (!target) return;
    setBusy('bridge');
    try {
      const bridge = await invoke<BridgeState>('detect_host_bridge', {
        context: target.context,
        namespace: target.namespace,
      });
      patchTarget({ bridge });
    } catch (err) {
      // 탐지 자체가 실패한 경우도 미검증으로 남긴다 — 임의 주소를 채우지 않는다.
      patchTarget({ bridge: { kind: 'unverified', candidates: [], reason: String(err) } });
    } finally {
      setBusy(null);
    }
  }, [target, patchTarget]);

  const save = useCallback(async () => {
    if (!target) return;
    setBusy('save');
    try {
      setTarget(await invoke<DeployTarget>('save_deploy_target', { target }));
      await message(`배포 대상을 [${target.context}] / ${target.namespace} 로 저장했습니다.`, {
        title: 'KubeMetal',
        kind: 'info',
      });
    } catch (err) {
      await message(`배포 대상 저장 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setBusy(null);
    }
  }, [target]);

  /** 배포 가능 여부와 그 사유. 버튼 비활성화와 안내 문구가 같은 판단을 쓰도록 한곳에서 계산한다. */
  const blockers: string[] = [];
  if (target?.bridge.kind === 'unverified') {
    blockers.push(`호스트 브리지가 검증되지 않았습니다 — ${target.bridge.reason}`);
  }
  if (preflight) blockers.push(...preflight.blockers);

  return {
    target,
    contexts,
    contextsError,
    preflight,
    preflightError,
    busy,
    blockers,
    isColima: target?.context === COLIMA_CONTEXT,
    selectContext,
    patchTarget,
    runPreflight,
    detectBridge,
    save,
    reload: loadTarget,
  };
}
