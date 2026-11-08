#include <cstdio>
#include <cmath>
#include <cuda_runtime.h>
__global__ void saxpy(int n, float a, const float* x, float* y) {
  int i = blockIdx.x * blockDim.x + threadIdx.x;
  if (i < n) y[i] = a * x[i] + y[i];
}
int main() {
  const int n = 1 << 20; const float a = 2.5f; size_t bytes = n * sizeof(float);
  float *hx = new float[n], *hy = new float[n], *ref = new float[n];
  for (int i = 0; i < n; ++i) { hx[i] = sinf(i); hy[i] = cosf(i); ref[i] = a * hx[i] + hy[i]; }
  float *dx, *dy;
  if (cudaMalloc(&dx, bytes) || cudaMalloc(&dy, bytes)) return 10;
  cudaMemcpy(dx, hx, bytes, cudaMemcpyHostToDevice); cudaMemcpy(dy, hy, bytes, cudaMemcpyHostToDevice);
  saxpy<<<(n + 255) / 256, 256>>>(n, a, dx, dy);
  if (cudaDeviceSynchronize() != cudaSuccess) return 11;
  cudaMemcpy(hy, dy, bytes, cudaMemcpyDeviceToHost);
  double maxrel = 0; for (int i = 0; i < n; ++i) maxrel = fmax(maxrel, fabs(hy[i] - ref[i]) / fmax(1e-6, fabs(ref[i])));
  printf("saxpy n=%d max_rel_error=%g\n", n, maxrel);
  return maxrel < 1e-5 ? 0 : 1;
}
