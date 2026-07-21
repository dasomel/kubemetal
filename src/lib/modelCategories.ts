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
