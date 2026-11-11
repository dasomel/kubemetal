#include "common.h"
#include "parity.h"
#include <cublas_v2.h>
int main() {
  const int N = 256; size_t b = (size_t)N * N * 4; float *A = (float*)malloc(b), *B = (float*)malloc(b), *C = (float*)malloc(b); float *dA, *dB, *dC;
  for (int i = 0; i < N * N; ++i) { A[i] = sinf(i * 0.01f); B[i] = cosf(i * 0.017f); }
  CK(cudaMalloc(&dA, b)); CK(cudaMalloc(&dB, b)); CK(cudaMalloc(&dC, b));
  CK(cudaMemcpy(dA, A, b, cudaMemcpyHostToDevice)); CK(cudaMemcpy(dB, B, b, cudaMemcpyHostToDevice));
  cublasHandle_t h; cublasStatus_t st = cublasCreate(&h); if (st != CUBLAS_STATUS_SUCCESS) { fprintf(stderr, "MATRIX_API_FAIL cublasCreate=%d\n", (int)st); return 21; }
  float al = 1.0f, be = 0.0f;
  // column-major: C = A*B with lda=ldb=ldc=N
  st = cublasSgemm(h, CUBLAS_OP_N, CUBLAS_OP_N, N, N, N, &al, dA, N, dB, N, &be, dC, N);
  if (st != CUBLAS_STATUS_SUCCESS) { fprintf(stderr, "MATRIX_API_FAIL cublasSgemm=%d\n", (int)st); return 22; }
  CK(cudaDeviceSynchronize()); CK(cudaMemcpy(C, dC, b, cudaMemcpyDeviceToHost)); cublasDestroy(h);
  parity::Check check; check.scan(C, (size_t)N * N);
  for (int i = 0; i < N; i += 3) for (int j = 0; j < N; j += 3) {  // col-major C(i,j)=sum_k A(i,k)B(k,j)
    double r = 0, m = 0; for (int k = 0; k < N; ++k) { double p = (double)A[k * N + i] * B[j * N + k]; r += p; m += fabs(p); }
    check.add(j * N + i, parity::ratio(fabs(C[j * N + i] - r), m));
  }
  char buf[160]; check.detail(buf, sizeof buf, 1e-5);
  return report("cublas_sgemm", check.passes(1e-5), fnv(C, b), true, buf);
}
