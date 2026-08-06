import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

/**
 * 백엔드가 실제로 배정한 호스트 포트. D1 표의 숫자는 우선 시도값이지 보장이 아니라서
 * (다른 로컬 프로세스가 같은 포트를 점유하면 대체 포트로 밀린다), 링크 URL을 컴포넌트에
 * 박아두면 그때 링크만 조용히 엉뚱한 곳을 가리킨다.
 *
 * 조회 전에는 빈 맵이고 `urlFor`는 빈 문자열을 돌려준다 — 모르는 값을 지어내지 않는다(D22).
 */
export function useHostPorts() {
  const [ports, setPorts] = useState<Record<string, number>>({});

  const refresh = useCallback(async () => {
    try {
      setPorts(await invoke<Record<string, number>>('get_host_ports'));
    } catch {
      // 조회 실패는 빈 맵을 유지한다 — 호출부가 링크를 비활성으로 렌더한다.
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  /** `http://127.0.0.1:{실제포트}` — 배정을 모르면 빈 문자열. */
  const urlFor = useCallback(
    (key: string) => (ports[key] ? `http://127.0.0.1:${ports[key]}` : ''),
    [ports],
  );

  return { ports, urlFor, refresh };
}
