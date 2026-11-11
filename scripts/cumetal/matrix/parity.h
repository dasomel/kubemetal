// Host-only numerical validation, shared by probes and CPU fault injection.
#pragma once
#include <cmath>
#include <cstddef>
#include <cstdio>
#include <limits>

namespace parity {
struct Check {
  bool finite = true;
  double worst = 0;
  long failed_index = -1;
  long worst_index = -1;

  void add(size_t index, double error) {
    if (!std::isfinite(error)) {
      finite = false;
      if (failed_index < 0) failed_index = (long)index;
    } else if (error > worst) {
      worst = error;
      worst_index = (long)index;
    }
  }
  template<class T> void scan(const T* output, size_t count) {
    for (size_t i = 0; i < count; ++i) {
      if (!std::isfinite((double)output[i])) add(i, output[i]);
    }
  }
  bool passes(double tolerance) const { return finite && worst < tolerance; }
  long failure(double tolerance) const {
    return failed_index >= 0 ? failed_index : (passes(tolerance) ? -1 : worst_index);
  }
  void detail(char* buffer, size_t size, double tolerance) const {
    snprintf(buffer, size, "finite=%s failed_index=%ld max_error=%.17g tol=%.17g",
             finite ? "true" : "false", failure(tolerance), worst, tolerance);
  }
};
// D22: an undefined ratio must fail, not disappear through fmax. Cost: zero
// reference magnitude is rejected; a future absolute-error policy must be explicit.
inline double ratio(double error, double denominator) {
  return std::isfinite(denominator) && denominator > 0
    ? error / denominator : std::numeric_limits<double>::quiet_NaN();
}
}
