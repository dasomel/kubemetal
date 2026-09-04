export interface ModelCategory {
  id: string;
  /** translations.ts 키 — 표시 텍스트는 t()로 조회한다 */
  labelKey: string;
  query: string;
  author?: string;
  /** translations.ts 키 — 표시 텍스트는 t()로 조회한다 */
  descKey: string;
}

/**
 * 검색어는 Hugging Face `/api/models` 실측(2026-07-21, curl)으로 의미 있는 결과가
 * 나오는 값만 채택했다. 각 항목의 실측 결과 개수는 useModelHub 훅 도입 커밋 로그 참고.
 */
export const MODEL_CATEGORIES: ModelCategory[] = [
  {
    id: 'popular',
    labelKey: 'modelhub.cat.popular.label',
    query: '',
    author: 'mlx-community',
    descKey: 'modelhub.cat.popular.desc',
  },
  {
    id: 'korean',
    labelKey: 'modelhub.cat.korean.label',
    query: 'EXAONE',
    descKey: 'modelhub.cat.korean.desc',
  },
  {
    id: 'coding',
    labelKey: 'modelhub.cat.coding.label',
    query: 'coder mlx',
    descKey: 'modelhub.cat.coding.desc',
  },
  {
    id: 'lightweight',
    labelKey: 'modelhub.cat.lightweight.label',
    query: '3B instruct 4bit',
    author: 'mlx-community',
    descKey: 'modelhub.cat.lightweight.desc',
  },
  {
    id: 'general',
    labelKey: 'modelhub.cat.general.label',
    query: 'instruct 4bit',
    author: 'mlx-community',
    descKey: 'modelhub.cat.general.desc',
  },
];

export interface RamSizeProfile {
  id: string;
  /** 가이드 표에 보이는 메모리 구간 표시 문구 (숫자·기호뿐이라 로케일 중립 — 그대로 표시) */
  range: string;
  /** 가이드 표에 보이는 권장 모델 규모 표시 문구 — translations.ts 키 */
  sizeLabelKey: string;
  query: string;
  author?: string;
  /** 이 프로필이 적용되는 총 메모리(GB) 하한 — matchRamProfile에서 현재 Mac과 매칭할 때 쓴다. */
  minGb: number;
}

/**
 * ModelHubGuideCard의 메모리별 권장 규모 표를 그대로 검색 프리셋으로도 쓴다.
 * 검색어는 Hugging Face `/api/models` 실측(2026-07-21, curl, author=mlx-community)으로
 * 해당 파라미터 규모대의 결과가 실제로 나오는 값만 채택했다.
 */
export const RAM_SIZE_PROFILES: RamSizeProfile[] = [
  {
    id: 'ram-16',
    range: '16GB',
    sizeLabelKey: 'modelhub.ram.16.sizeLabel',
    query: '3B instruct 4bit',
    author: 'mlx-community',
    minGb: 0,
  },
  {
    id: 'ram-32',
    range: '32~48GB',
    sizeLabelKey: 'modelhub.ram.32.sizeLabel',
    query: '7B instruct 4bit',
    author: 'mlx-community',
    minGb: 32,
  },
  {
    id: 'ram-64',
    range: '64GB+',
    sizeLabelKey: 'modelhub.ram.64.sizeLabel',
    query: '32B instruct 4bit',
    author: 'mlx-community',
    minGb: 64,
  },
];

/**
 * 총 메모리(GB)에 해당하는 RAM 프로필을 찾는다.
 * RAM_SIZE_PROFILES는 minGb 오름차순이므로 뒤에서부터 가장 구체적인 프로필을 찾는다.
 */
export function matchRamProfile(totalGb: number): RamSizeProfile {
  for (let i = RAM_SIZE_PROFILES.length - 1; i >= 0; i -= 1) {
    if (totalGb >= RAM_SIZE_PROFILES[i].minGb) return RAM_SIZE_PROFILES[i];
  }
  return RAM_SIZE_PROFILES[0];
}
