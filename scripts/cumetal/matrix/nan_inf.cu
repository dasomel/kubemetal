#include "common.h"
// IEEE special-value propagation on device must match the host: NaN, +-Inf, signed zero.
__global__ void k(const float* z, const float* one, float* o) {
  float Z = z[0], O = one[0];
  o[0] = Z / Z;           // NaN
  o[1] = O / Z;           // +Inf
  o[2] = -O / Z;          // -Inf
  o[3] = o[1] - o[1];     // Inf-Inf = NaN
  o[4] = sqrtf(-O);       // NaN
  o[5] = o[1] * Z;        // Inf*0 = NaN
  o[6] = -Z;              // -0
  o[7] = fminf(o[0], O);  // fmin ignores NaN -> 1
  o[8] = (o[0] == o[0]) ? 1.f : 0.f;  // NaN != NaN -> 0
}
int main() {
  float hz = 0.f, ho = 1.f, out[9]; float *dz, *d1, *dout;
  CK(cudaMalloc(&dz, 4)); CK(cudaMalloc(&d1, 4)); CK(cudaMalloc(&dout, sizeof out));
  CK(cudaMemcpy(dz, &hz, 4, cudaMemcpyHostToDevice)); CK(cudaMemcpy(d1, &ho, 4, cudaMemcpyHostToDevice));
  k<<<1, 1>>>(dz, d1, dout); CK(cudaGetLastError()); CK(cudaDeviceSynchronize());
  CK(cudaMemcpy(out, dout, sizeof out, cudaMemcpyDeviceToHost));
  volatile float vz = 0.f, vo = 1.f; float ref[9];
  ref[0] = vz / vz; ref[1] = vo / vz; ref[2] = -vo / vz; ref[3] = ref[1] - ref[1]; ref[4] = sqrtf(-vo); ref[5] = ref[1] * vz; ref[6] = -vz; ref[7] = fminf(ref[0], vo); ref[8] = 0.f;
  long bad = 0; char cls[40] = {0};
  for (int i = 0; i < 9; ++i) {
    bool same = (std::isnan(ref[i]) && std::isnan(out[i])) || (!std::isnan(ref[i]) && ref[i] == out[i] && std::signbit(ref[i]) == std::signbit(out[i]));
    if (!same) { ++bad; fprintf(stderr, "nan_inf mismatch idx=%d dev=%g host=%g\n", i, out[i], ref[i]); }
    cls[i] = std::isnan(out[i]) ? 'N' : std::isinf(out[i]) ? (out[i] > 0 ? 'I' : 'i') : 'f';
  }
  char buf[80]; snprintf(buf, sizeof buf, "bad=%ld classes=%s", bad, cls);
  uint64_t d = fnv(cls, 9);
  return report("nan_inf", bad == 0, d, true, buf);
}
