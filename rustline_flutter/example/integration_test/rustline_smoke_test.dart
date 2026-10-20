// M6 tests-first (Tester, 2026-10-20): plan M6 step 8 smoke test: open the pack and
// plan one journey, on the emulator (A54 later, by Savin).
// Run: cd rustline_flutter/example && flutter test integration_test/rustline_smoke_test.dart \
//   --dart-define=RUSTLINE_M6_PACK=/data/data/<app id>/files/brasov.rlp
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'm6_adapter.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(init);

  testWidgets('opens the pack and plans one journey', (tester) async {
    final p = envPack();
    if (p == null) return;
    final e = await open(p);
    final i = await info(e);
    expect(i.timezone, 'Europe/Bucharest');
    // Livada Poștei -> Gara Brașov.
    final r = await plan(
        e, req(45.6427, 25.5887, 45.6612, 25.6136, await morningInFeed(e)));
    expect(r.itineraries, isNotEmpty);
    for (final it in r.itineraries) {
      expect(it.arrivalMs, greaterThanOrEqualTo(it.departureMs));
      expect(it.departureOffsetSecs, anyOf(7200, 10800));
    }
  }, skip: m6Skip != null);

  testWidgets('plan does not block the UI thread', (tester) async {
    final p = envPack();
    if (p == null) return;
    final e = await open(p);
    final t = await morningInFeed(e);
    var ticks = 0;
    final sw = Stopwatch()..start();
    final f = plan(e, req(45.6427, 25.5887, 45.6612, 25.6136, t));
    while (sw.elapsedMilliseconds < 50) {
      await Future<void>.delayed(const Duration(milliseconds: 1));
      ticks++;
    }
    await f;
    expect(ticks, greaterThan(5), reason: 'event loop starved during plan');
  }, skip: m6Skip != null);
}
