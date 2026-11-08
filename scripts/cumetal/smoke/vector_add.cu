#include <cstdio>
#include <cmath>
#include <cuda_runtime.h>
__global__ void add(const float* a, const float* b, float* c, int n) {
  int i = blockIdx.x * blockDim.x + threadIdx.x;
  if (i < n) c[i] = a[i] + b[i];
}
int main() {
  const int n = 1 << 20; size_t bytes = n * sizeof(float);
  float *ha = new float[n], *hb = new float[n], *hc = new float[n];
  for (int i = 0; i < n; ++i) { ha[i] = i * 0.5f; hb[i] = 1.0f - i * 0.25f; }
  float *da, *db, *dc;
  if (cudaMalloc(&da, bytes) || cudaMalloc(&db, bytes) || cudaMalloc(&dc, bytes)) return 10;
  cudaMemcpy(da, ha, bytes, cudaMemcpyHostToDevice); cudaMemcpy(db, hb, bytes, cudaMemcpyHostToDevice);
  add<<<(n + 255) / 256, 256>>>(da, db, dc, n);
  if (cudaDeviceSynchronize() != cudaSuccess) return 11;
  cudaMemcpy(hc, dc, bytes, cudaMemcpyDeviceToHost);
  double maxerr = 0; for (int i = 0; i < n; ++i) maxerr = fmax(maxerr, fabs(hc[i] - (ha[i] + hb[i])));
  printf("vectorAdd n=%d max_abs_error=%g\n", n, maxerr);
  return maxerr == 0 ? 0 : 1;
}
