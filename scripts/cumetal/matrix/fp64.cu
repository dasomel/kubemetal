#include "common.h"
// FP64 policy: CuMetal virtualises double (default fast48 = ~48 significand bits), so tolerance is 1e-9 rel, not bit-exact.
__global__ void k(const double* a, const double* b, double* o, int n) {
  int i = blockIdx.x * blockDim.x + threadIdx.x; if (i < n) o[i] = a[i] * b[i] + sqrt(a[i]) / (1.0 + b[i]);
}
int main() {
  const int n = 1 << 14; double *a = (double*)malloc(n * 8), *b = (double*)malloc(n * 8), *o = (double*)malloc(n * 8), *da, *db, *dout;
  for (int i = 0; i < n; ++i) { a[i] = 1.0 + i * 1e-3; b[i] = 0.5 + (i % 100) * 1e-2; }
  CK(cudaMalloc(&da, n * 8)); CK(cudaMalloc(&db, n * 8)); CK(cudaMalloc(&dout, n * 8));
  CK(cudaMemcpy(da, a, n * 8, cudaMemcpyHostToDevice)); CK(cudaMemcpy(db, b, n * 8, cudaMemcpyHostToDevice));
  k<<<n / 256, 256>>>(da, db, dout, n); CK(cudaGetLastError()); CK(cudaDeviceSynchronize());
  CK(cudaMemcpy(o, dout, n * 8, cudaMemcpyDeviceToHost));
  double mx = 0; for (int i = 0; i < n; ++i) { double r = a[i] * b[i] + sqrt(a[i]) / (1.0 + b[i]); mx = fmax(mx, fabs(o[i] - r) / fabs(r)); }
  char buf[64]; snprintf(buf, sizeof buf, "max_rel_error=%.3e tol=1e-9", mx);
  return report("fp64", mx < 1e-9, fnv(o, n * 8), true, buf);
}
