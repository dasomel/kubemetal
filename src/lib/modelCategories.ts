export interface ModelCategory {
  id: string;
  label: string;
  query: string;
  author?: string;
  description: string;
}

/**
 * 검색어는 Hugging Face `/api/models` 실측(2026-07-21, curl)으로 의미 있는 결과가
 * 나오는 값만 채택했다. 각 항목의 실측 결과 개수는 useModelHub 훅 도입 커밋 로그 참고.
 */
export const MODEL_CATEGORIES: ModelCategory[] = [
  {
    id: 'popular',
    label: '인기 (MLX)',
    query: '',
    author: 'mlx-community',
    description: 'mlx-community가 배포한 다운로드 상위 MLX 변환 모델입니다.',
  },
  {
    id: 'korean',
    label: '한국어',
    query: 'EXAONE',
    description: 'LG AI연구원 EXAONE 계열 등 한국어 특화 모델입니다.',
  },
  {
    id: 'coding',
    label: '코딩',
    query: 'coder mlx',
    description: '코드 생성·보완에 특화된 MLX 변환 모델입니다.',
  },
  {
    id: 'lightweight',
    label: '경량 (~3B)',
    query: '3B instruct 4bit',
    author: 'mlx-community',
    description: '3B급 이하 4bit 양자화 모델로 메모리 부담이 적습니다.',
  },
  {
    id: 'general',
    label: '범용 챗',
    query: 'instruct 4bit',
    author: 'mlx-community',
    description: '일반 대화·지시 수행용 4bit 양자화 인스트럭트 모델입니다.',
  },
];

export interface RamSizeProfile {
  id: string;
  /** 가이드 표에 보이는 메모리 구간 표시 문구 */
  range: string;
  /** 가이드 표에 보이는 권장 모델 규모 표시 문구 */
  sizeLabel: string;
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
    sizeLabel: '1~4B급',
    query: '3B instruct 4bit',
    author: 'mlx-community',
    minGb: 0,
  },
  {
    id: 'ram-32',
    range: '32~48GB',
    sizeLabel: '7~14B급',
    query: '7B instruct 4bit',
    author: 'mlx-community',
    minGb: 32,
  },
  {
    id: 'ram-64',
    range: '64GB+',
    sizeLabel: '32B급 이상',
    query: '32B instruct 4bit',
    author: 'mlx-community',
    minGb: 64,
  },
];

/** 총 메모리(GB)에 해당하는 RAM 프로필을 찾는다 — minGb 내림차순으로 첫 매치를 반환. */
export function matchRamProfile(totalGb: number): RamSizeProfile {
  const sorted = [...RAM_SIZE_PROFILES].sort((a, b) => b.minGb - a.minGb);
  return sorted.find((p) => totalGb >= p.minGb) ?? RAM_SIZE_PROFILES[0];
}
