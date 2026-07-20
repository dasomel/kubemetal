export interface VmResources {
  cpu: number;
  memoryGb: number;
}

/**
 * 감지된 전체 RAM(totalMemoryGb) 기준 VM 기본 사양 자동 산정 (D4)
 * - 64GB+ → 12GB / 6 CPU
 * - 32 ~ 48GB → 8GB / 4 CPU
 * - < 32GB (16GB급) → 4GB / 2 CPU
 */
export function recommendVmResources(totalMemoryGb: number): VmResources {
  if (totalMemoryGb >= 64) {
    return { cpu: 6, memoryGb: 12 };
  }
  if (totalMemoryGb >= 32) {
    return { cpu: 4, memoryGb: 8 };
  }
  return { cpu: 2, memoryGb: 4 };
}
