// M6 tests-first (Tester, 2026-10-20): plugin unit test (host, no device): the Dart
// side maps every stable Rust error code to a typed exception and an unknown code to
// `internal`. Assumes the plugin exports `RustlineException` with `code` and
// `RustlineException.fromMessage(String)` (not in the plan; Builder's choice of names,
// adapt here).
import 'package:flutter_test/flutter_test.dart';
import 'package:rustline_flutter/rustline_flutter.dart';

const String? m6Skip = null; // M6 implemented (Builder, 2026-10-20).

void main() {
  const codes = [
    'invalid_request',
    'outside_coverage',
    'no_route',
    'date_outside_feed',
    'pack_corrupt',
    'pack_version',
    'internal',
  ];

  test('every stable code survives the trip to Dart', () {
    for (final c in codes) {
      expect(RustlineException.fromMessage('$c: some text').code, c);
    }
  }, skip: m6Skip);

  test('an unknown or empty code is internal, never a crash', () {
    for (final m in ['', 'weird', '🚌 ăîșț', 'x' * 100000]) {
      expect(RustlineException.fromMessage(m).code, 'internal');
    }
  }, skip: m6Skip);
}
