"""Compile the real pure helper on the CPU; never execute CuMetal."""
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
MATRIX = ROOT / 'scripts/cumetal/matrix'


class ParityTests(unittest.TestCase):
    def test_host_helper_and_classifier(self):
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp)
            source = work / 'parity.cpp'
            source.write_text(r'''
#include "parity.h"
#include <cassert>
int main() {
  double values[] = {1, 2, 3, 4};
  parity::Check good; good.scan(values, 4); good.add(0, parity::ratio(0, 1));
  assert(good.passes(0.125)); assert(good.failure(0.125) == -1);
  good.add(2, 0.125); assert(!good.passes(0.125)); assert(good.failure(0.125) == 2);
  assert(good.passes(0.126));
  for (double bad : {NAN, INFINITY, -INFINITY}) {
    values[3] = bad; // sampled reference checks only index 0
    parity::Check check; check.scan(values, 4); check.add(0, 0);
    assert(!check.passes(1)); assert(!check.finite); assert(check.failure(1) == 3);
    char detail[160]; check.detail(detail, sizeof detail, 1);
    printf("MATRIX_RESULT probe=sgemm_handwritten status=MISMATCH digest=abc %s\n", detail);
  }
  for (double denominator : {0., -1., (double)NAN, (double)INFINITY}) {
    parity::Check check; check.add(5, parity::ratio(0, denominator));
    assert(!check.passes(1)); assert(check.failure(1) == 5);
  }
  parity::Check overflow; overflow.add(7, parity::ratio(INFINITY, 1));
  assert(!overflow.passes(1));
}
'''.replace('#include <cassert>', '#include <cassert>\n#include <initializer_list>'))
            compiler = shutil.which('c++')
            self.assertIsNotNone(compiler, 'host C++ compiler required')
            subprocess.run([compiler, '-std=c++17', '-I', str(MATRIX), str(source),
                            '-o', str(work / 'check')], check=True, capture_output=True)
            run = subprocess.run([str(work / 'check')], check=True, capture_output=True, text=True)
            self.assertIn('finite=false failed_index=3', run.stdout)
            spec = importlib.util.spec_from_file_location('parity_classifier', MATRIX / 'classify.py')
            classifier = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(classifier)
            (work / 'jit.compile.rc').write_text('0')
            for i in range(1, 4):
                (work / f'jit.run{i}.rc').write_text('1')
                (work / f'jit.run{i}.out').write_text(run.stdout.splitlines()[0])
            self.assertEqual(classifier.classify_path(str(work), 'jit', True)['class'], 'failed')

    def test_all_four_probes_use_full_scan(self):
        for name, marker in [('sgemm_handwritten', 'check.scan(C, (size_t)N * N)'),
                             ('cublas_sgemm', 'check.scan(C, (size_t)N * N)'),
                             ('fp64', 'check.scan(o, n)'),
                             ('cufft_c2c', 'std::isfinite(o[i].y)')]:
            with self.subTest(name=name):
                text = (MATRIX / (name + '.cu')).read_text()
                self.assertIn('#include "parity.h"', text)
                self.assertIn(marker, text)
                self.assertIn('.passes(', text)
