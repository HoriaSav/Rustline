// Added by Builder (M6 review MF3): plan M6 Safety, "injected panic in a
// debug-only hook → Dart exception, app alive", end to end through FRB.
// Debug builds only: in profile/release the hook does nothing (invalid_request).
// Run: cd rustline_flutter/example && flutter test integration_test/rustline_panic_test.dart \
//   --dart-define=RUSTLINE_M6_PACK=<device path>
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'm6_adapter.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(init);

  testWidgets('an injected panic is a RustlineException, the app stays alive',
      (tester) async {
    await tester.pumpWidget(const MaterialApp(home: Text('alive')));
    for (var i = 0; i < 3; i++) {
      try {
        await injectPanic();
        fail('expected an exception');
      } on RustlineException catch (e) {
        if (kDebugMode) {
          expect(e.code, 'internal');
          expect(e.message, 'internal error');
        } else {
          expect(e.code, 'invalid_request');
        }
      }
    }
    // The UI still renders and Rust still answers.
    await tester.pump();
    expect(find.text('alive'), findsOneWidget);
    final p = envPack();
    if (p == null) return;
    final e = await open(p);
    final r = await plan(
        e, req(45.6427, 25.5887, 45.6612, 25.6136, await morningInFeed(e)));
    expect(r.itineraries, isNotEmpty);
  }, skip: m6Skip != null);
}
