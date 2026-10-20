// M6 tests-first (Tester, 2026-10-20): lifecycle edge cases: engine opened twice and
// concurrently, concurrent queries, cancellation / stale results, pack swap mid-query.
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'm6_adapter.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(init);

  testWidgets('engine opened twice and concurrently', (t) async {
    final p = envPack();
    if (p == null) return;
    final es = await Future.wait(List.generate(4, (_) => open(p)));
    final when = await morningInFeed(es.first);
    final rs = await Future.wait(
        es.map((e) => plan(e, req(45.6427, 25.5887, 45.6612, 25.6136, when))));
    for (final r in rs) {
      expect(r.itineraries.length, rs.first.itineraries.length);
      expect(
          r.itineraries.first.arrivalMs, rs.first.itineraries.first.arrivalMs);
    }
  }, skip: m6Skip != null);

  testWidgets('20 concurrent queries on one engine', (t) async {
    final p = envPack();
    if (p == null) return;
    final e = await open(p);
    final when = await morningInFeed(e);
    final pairs = fixedPairs().take(20).toList();
    Future<String> one((double, double, double, double) q) => plan(
            e, req(q.$1, q.$2, q.$3, q.$4, when))
        .then((r) => 'ok ${r.itineraries.map((i) => i.arrivalMs).join(',')}')
        .catchError((Object x) => 'err ${codeOf(x)}');
    final seq = <String>[];
    for (final q in pairs) {
      seq.add(await one(q));
    }
    expect(await Future.wait(pairs.map(one)), seq);
  }, skip: m6Skip != null);

  testWidgets('a superseded query is discarded (latest wins)', (t) async {
    final p = envPack();
    if (p == null) return;
    final e = await open(p);
    final when = await morningInFeed(e);
    final pl = planner(e);
    final first = pl.plan(req(45.6427, 25.5887, 45.6612, 25.6136, when));
    final second = pl.plan(req(45.6427, 25.5887, 45.6300, 25.5800, when));
    expect(await first, isNull, reason: 'stale result must be dropped');
    expect(await second, isNotNull);
  }, skip: m6Skip != null);

  testWidgets('pack swap mid-query', (t) async {
    final p = envPack();
    if (p == null) return;
    final dir = await Directory.systemTemp.createTemp('rustline_m6_swap_');
    final dest = '${dir.path}/brasov.rlp';
    await File(p).copy(dest);
    var engine = await open(dest);
    final when = await morningInFeed(engine);
    final running = List.generate(
        10, (_) => plan(engine, req(45.6427, 25.5887, 45.6612, 25.6136, when)));
    // New pack: written next to it, then renamed over (atomic), then reopened.
    await File(p).copy('$dest.new');
    await File('$dest.new').rename(dest);
    engine = await open(dest);
    final old = await Future.wait(running);
    expect(old.every((r) => r.itineraries.isNotEmpty), isTrue);
    expect(
        (await plan(engine, req(45.6427, 25.5887, 45.6612, 25.6136, when)))
            .itineraries,
        isNotEmpty);
    await dir.delete(recursive: true);
  }, skip: m6Skip != null);

  testWidgets('device timezone differs from the feed', (t) async {
    final p = envPack();
    if (p == null) return;
    final e = await open(p);
    final utc = await morningInFeed(e);
    final a = await plan(e, req(45.6427, 25.5887, 45.6612, 25.6136, utc));
    final b =
        await plan(e, req(45.6427, 25.5887, 45.6612, 25.6136, utc.toLocal()));
    expect(b.itineraries.map((i) => i.arrivalMs),
        a.itineraries.map((i) => i.arrivalMs));
    for (final i in a.itineraries) {
      expect(i.departureOffsetSecs, anyOf(7200, 10800),
          reason: 'feed offset, not device');
    }
  }, skip: m6Skip != null);
}
