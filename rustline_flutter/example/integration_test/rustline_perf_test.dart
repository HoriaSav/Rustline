// M6 tests-first (Tester, 2026-10-20): plan M6 step 7 / DoD: 100 fixed queries in
// profile mode, p50/p95 and pack load time. Emulator numbers are recorded only; the
// A54 targets (< 150 ms p95, < 500 ms load) are asserted with
// --dart-define=RUSTLINE_M6_ENFORCE=true (Savin, on the A54).
// Run (profile): cd rustline_flutter/example && flutter drive --profile \
//   --driver=test_driver/integration_test.dart \
//   --target=integration_test/rustline_perf_test.dart \
//   --dart-define=RUSTLINE_M6_PACK=<device path>
import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'm6_adapter.dart';

int pct(List<int> sorted, double q) =>
    sorted[((sorted.length - 1) * q).round().clamp(0, sorted.length - 1)];

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(init);

  testWidgets('100 fixed queries: p50/p95 and load time', (t) async {
    final p = envPack();
    if (p == null) return;
    if (!kProfileMode && !kReleaseMode) {
      // ignore: avoid_print
      print('warning: not profile/release, numbers are not comparable');
    }
    final loads = <int>[];
    late RustlineEngine e;
    for (var i = 0; i < 5; i++) {
      final sw = Stopwatch()..start();
      e = await open(p);
      loads.add(sw.elapsedMicroseconds);
    }
    loads.sort();
    final when = await morningInFeed(e);
    final us = <int>[];
    var ok = 0;
    for (final q in fixedPairs()) {
      final sw = Stopwatch()..start();
      try {
        await plan(e, req(q.$1, q.$2, q.$3, q.$4, when));
        ok++;
      } catch (x) {
        codeOf(x); // must be a stable code
      }
      us.add(sw.elapsedMicroseconds);
    }
    us.sort();
    final report = {
      'mode': kProfileMode ? 'profile' : (kReleaseMode ? 'release' : 'debug'),
      'queries': us.length,
      'ok': ok,
      'p50_ms': pct(us, 0.5) / 1000,
      'p95_ms': pct(us, 0.95) / 1000,
      'max_ms': us.last / 1000,
      'load_median_ms': pct(loads, 0.5) / 1000,
    };
    binding.reportData = report;
    // ignore: avoid_print
    print('RUSTLINE_PERF $report');
    expect(us.length, 100);
    // Tester (M6 final check item 1): >= 40, below the 8-seed minimum of 49.
    expect(ok, greaterThanOrEqualTo(40),
        reason: 'most Brașov pairs have a route');
    expect(us.last, lessThanOrEqualTo(1000000),
        reason: 'single query bound 1 s');
    if (enforceBudgets) {
      expect(pct(us, 0.95), lessThan(150000));
      expect(pct(loads, 0.5), lessThan(500000));
    }
  }, skip: m6Skip != null);
}
