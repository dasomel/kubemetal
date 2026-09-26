import { invoke } from '@tauri-apps/api/core';
import { ask, message } from '@tauri-apps/plugin-dialog';
import type { OperationSummary } from '../types/ipc';

type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

/**
 * 파괴적 배포 액션(provision_mlops_stack/stop_cluster/install_kagent) 실행 직전 공용
 * 확인 플로우. `describe_deploy_operation`이 돌려주는 요약을 `ask()`로 보여주고 사용자
 * 확인을 받는다. describe 자체가 실패하면 원인을 `message()`로 보여주고 항상 false를
 * 반환한다 — 확인 없이 진행하지 않는다(D22 원칙: 실패를 성공처럼 넘기지 않는다).
 */
export async function confirmDeployOperation(
  t: TranslateFn,
  action: string,
  context?: string,
): Promise<boolean> {
  try {
    const summary = await invoke<OperationSummary>('describe_deploy_operation', {
      action,
      context,
    });
    return await ask(
      t('deployOp.confirmationMessage', {
        targetDescription: summary.target_description,
        context: summary.context,
        namespace: summary.namespace,
        riskClass: summary.risk_class,
      }),
      { title: t('deployOp.confirmationTitle'), kind: 'warning' },
    );
  } catch (error) {
    await message(t('deployOp.confirmationFailed', { error: String(error) }), {
      title: t('deployOp.confirmationTitle'),
      kind: 'error',
    });
    return false;
  }
}
