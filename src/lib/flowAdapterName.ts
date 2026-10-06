/** 클릭마다 고유한 어댑터 이름. D45는 기존 최종 이름을 거부하므로 고정 이름은 두 번째부터 실패한다.
 * 문자셋은 백엔드 validate_adapter_name(영숫자 - _ .)을 만족한다. */
export const flowAdapterName = (now: Date = new Date()): string => {
  const p = (n: number) => String(n).padStart(2, '0');
  const date = `${now.getFullYear()}${p(now.getMonth() + 1)}${p(now.getDate())}`;
  const time = `${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}`;
  return `flow-adapter-${date}-${time}`;
};
