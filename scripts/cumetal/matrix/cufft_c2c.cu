#include "common.h"
#include "parity.h"
#include <cufft.h>
int main() {
  const int n = 1024; size_t b = n * sizeof(cufftComplex); cufftComplex *h = (cufftComplex*)malloc(b), *o = (cufftComplex*)malloc(b), *d;
  for (int i = 0; i < n; ++i) { h[i].x = sinf(0.05f * i) + 0.5f * cosf(0.31f * i); h[i].y = 0; }
  CK(cudaMalloc(&d, b)); CK(cudaMemcpy(d, h, b, cudaMemcpyHostToDevice));
  cufftHandle p; cufftResult r = cufftPlan1d(&p, n, CUFFT_C2C, 1); if (r != CUFFT_SUCCESS) { fprintf(stderr, "MATRIX_API_FAIL cufftPlan1d=%d\n", (int)r); return 21; }
  r = cufftExecC2C(p, d, d, CUFFT_FORWARD); if (r != CUFFT_SUCCESS) { fprintf(stderr, "MATRIX_API_FAIL fwd=%d\n", (int)r); return 22; }
  CK(cudaDeviceSynchronize()); CK(cudaMemcpy(o, d, b, cudaMemcpyDeviceToHost));
  // forward check vs naive DFT on sampled bins
  parity::Check forward;
  for (int i = 0; i < n; ++i) { forward.add(i, std::isfinite(o[i].y) && std::isfinite(o[i].x) ? 0 : NAN); }
  double mag = 0; for (int k = 0; k < n; k += 31) {
    double re = 0, im = 0; for (int t = 0; t < n; ++t) { double a = -2.0 * M_PI * k * t / n; re += h[t].x * cos(a); im += h[t].x * sin(a); }
    forward.add(k, hypot(o[k].x - re, o[k].y - im)); mag = fmax(mag, hypot(re, im)); }
  uint64_t dg = fnv(o, b);
  r = cufftExecC2C(p, d, d, CUFFT_INVERSE); if (r != CUFFT_SUCCESS) { fprintf(stderr, "MATRIX_API_FAIL inv=%d\n", (int)r); return 23; }
  CK(cudaDeviceSynchronize()); CK(cudaMemcpy(o, d, b, cudaMemcpyDeviceToHost)); cufftDestroy(p);
  parity::Check reverse; for (int i = 0; i < n; ++i) { reverse.add(i, std::isfinite(o[i].x) && std::isfinite(o[i].y) ? fabs(o[i].x / n - h[i].x) + fabs(o[i].y / n) : NAN); }  // cuFFT inverse is unnormalised
  // Forward tolerance is relative to the largest sampled reference magnitude.
  forward.worst = parity::ratio(forward.worst, mag);
  forward.add(forward.worst_index < 0 ? 0 : forward.worst_index, forward.worst);
  char f[160], rdetail[160], buf[360]; forward.detail(f, sizeof f, 1e-3); reverse.detail(rdetail, sizeof rdetail, 1e-3);
  snprintf(buf, sizeof buf, "forward{%s} reverse{%s}", f, rdetail);
  return report("cufft_c2c", forward.passes(1e-3) && reverse.passes(1e-3), dg, true, buf);
}
