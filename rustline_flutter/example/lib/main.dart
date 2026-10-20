// Rustline example app (plan M6): opens a pack and plans one Brașov journey.
// Run: flutter run --release --dart-define=RUSTLINE_PACK=<path on the device>
import 'package:flutter/material.dart';
import 'package:rustline_flutter/rustline_flutter.dart';

const packPath = String.fromEnvironment('RUSTLINE_PACK');

Future<void> main() async {
  await RustLib.init();
  runApp(const MaterialApp(home: PlanPage()));
}

class PlanPage extends StatefulWidget {
  const PlanPage({super.key});

  @override
  State<PlanPage> createState() => _PlanPageState();
}

class _PlanPageState extends State<PlanPage> {
  String _text = packPath.isEmpty
      ? 'Pass --dart-define=RUSTLINE_PACK=<pack path on the device>'
      : 'Tap Plan';

  Future<void> _plan() async {
    try {
      final sw = Stopwatch()..start();
      final e = await RustlineEngine.open(path: packPath);
      final loadMs = sw.elapsedMilliseconds;
      final i = await e.info();
      // Livada Poștei -> Gara Brașov, 08:00 on the third day of the feed (UTC+3).
      final t = (i.validFromDays + 2) * 86400000 + 5 * 3600000;
      sw.reset();
      final r = await e.plan(
          req: FfiPlanRequest(
              fromLat: 45.6427,
              fromLon: 25.5887,
              toLat: 45.6612,
              toLon: 25.6136,
              departAtMs: t,
              departOffsetSecs: 3 * 3600,
              maxResults: 3));
      final lines = r.itineraries.map((it) {
        final dep =
            DateTime.fromMillisecondsSinceEpoch(it.departureMs, isUtc: true)
                .add(Duration(seconds: it.departureOffsetSecs));
        final arr =
            DateTime.fromMillisecondsSinceEpoch(it.arrivalMs, isUtc: true)
                .add(Duration(seconds: it.arrivalOffsetSecs));
        return '${_hm(dep)} → ${_hm(arr)}, ${it.transfers} transfers, ${it.walkM} m walk';
      });
      setState(() => _text = '${i.feedId}: load $loadMs ms, plan '
          '${sw.elapsedMilliseconds} ms\n${lines.join('\n')}');
    } on RustlineException catch (x) {
      setState(() => _text = 'Error ${x.code}: ${x.message}');
    }
  }

  static String _hm(DateTime t) =>
      '${t.hour.toString().padLeft(2, '0')}:${t.minute.toString().padLeft(2, '0')}';

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Rustline')),
      body: Padding(padding: const EdgeInsets.all(16), child: Text(_text)),
      floatingActionButton: packPath.isEmpty
          ? null
          : FloatingActionButton(onPressed: _plan, child: const Text('Plan')),
    );
  }
}
