import { invoke } from '@tauri-apps/api/core';
import { ask, message } from '@tauri-apps/plugin-dialog';
import type { OperationSummary } from '../types/ipc';

type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

/**
 * 파괴적 배포 액션(provision_mlops_stack/stop_cluster/install_kagent) 실행 직전 공용
 * 확인 플로우. `describe_deploy_operation`이 돌려주는 요약을 `ask()`로 보여주고 사용자
 * 확인을 받는다. 승인하면 실행 대상 비교에 쓸 요약을 돌려준다. describe 자체가 실패하면
 * 원인을 `message()`로 보여주고 null을 반환한다 — 확인 없이 진행하지 않는다(D22 원칙).
 */
export async function confirmDeployOperation(
  t: TranslateFn,
  action: string,
  context?: string,
): Promise<OperationSummary | null> {
  try {
    const summary = await invoke<OperationSummary>('describe_deploy_operation', {
      action,
      context,
    });
    const confirmed = await ask(
      t('deployOp.confirmationMessage', {
        targetDescription: summary.target_description,
        context: summary.context,
        namespace: summary.namespace,
        riskClass: t(`deployOp.risk.${summary.risk_class}`),
        question: t(`deployOp.question.${action}`),
      }),
      { title: t('deployOp.confirmationTitle'), kind: 'warning' },
    );
    return confirmed ? summary : null;
  } catch (error) {
    // 오류 다이얼로그마저 실패해도 호출부의 busy 상태가 풀리도록 이 함수는 절대 throw하지 않는다.
    await message(t('deployOp.confirmationFailed', { error: String(error) }), {
      title: t('deployOp.confirmationTitle'),
      kind: 'error',
    }).catch(() => undefined);
    return null;
  }
}
