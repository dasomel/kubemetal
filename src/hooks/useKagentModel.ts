import { useCallback, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import type { KagentModelStatus } from '../types/ipc';
import { useTranslation } from '../i18n/i18nContext';

/**
 * 저장된 DeployTarget(D26) 기준 kagent 모델 연계(D32). KagentOpsView의 로컬 kubeconfig
 * 선택기와는 무관한 별도 대상이라 이 훅은 그 상태를 구독하지 않는다.
 */
export function useKagentModel() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<KagentModelStatus | null>(null);
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<boolean>(false);

  // 조회 실패는 그대로 노출한다 — "정상"으로 폴백하면 장애를 정상으로 위장하게 된다(D22).
  const fetchStatus = useCallback(async () => {
    setLoading(true);
    try {
      const res = await invoke<KagentModelStatus>('get_kagent_model_status');
      setStatus(res);
      setError(null);
    } catch (e) {
      setStatus(null);
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  // 백엔드가 반환한 helm 출력을 그대로 인용한다 — "연결됨"을 프론트에서 단정하지 않는다(D22).
  const connect = useCallback(async () => {
    setBusy(true);
    try {
      const res = await invoke<string>('configure_kagent_model');
      await message(res, { title: 'KubeMetal', kind: 'info' });
    } catch (e) {
      await message(t('kagent.modelStatus.connectFailed', { error: String(e) }), {
        title: 'KubeMetal',
        kind: 'error',
      });
    } finally {
      setBusy(false);
      await fetchStatus();
    }
  }, [fetchStatus, t]);

  return { status, loading, error, busy, fetchStatus, connect };
}
