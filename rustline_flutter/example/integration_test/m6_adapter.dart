// M6 tests-first (Tester, 2026-10-20): the one adapter between the M6 Dart tests and
// the FRB-generated `rustline_flutter` API. Builder: change only this file when the
// generated names differ, then set `m6Skip` to null.
//
// Assumed (plan M6 step 2, spec §11.3):
//   await RustLib.init();
//   final e = await RustlineEngine.open(path: p);
//   final r = await e.plan(req: FfiPlanRequest(...));   // times: epoch ms + offset s
//   final i = await e.info();
//   await installPack(srcZst: z, dest: d);              // free space, .tmp, rename
//   errors: RustlineException with a stable `code` field (Builder, M6)
//   RustlinePlanner: latest-wins wrapper (plan M6 "cancellation"), returns null for a
//   superseded query.
//
// Inputs (all optional; tests print `skip:` when missing):
//   --dart-define=RUSTLINE_M6_PACK=<path on the device to a Brașov .rlp>
//   --dart-define=RUSTLINE_M6_PACK_ZST=<path on the device to the .rlp.zst>
//   --dart-define=RUSTLINE_M6_ENFORCE=true   (assert the phone budgets, A54 only)
import 'dart:io';

import 'package:rustline_flutter/rustline_flutter.dart';

// Builder (M6): the tests name plugin types (e.g. RustlineEngine) through this file.
export 'package:rustline_flutter/rustline_flutter.dart';

/// Set to null when M6 is implemented.
const String? m6Skip = null; // M6 implemented (Builder, 2026-10-20).

const packPath = String.fromEnvironment('RUSTLINE_M6_PACK');
const packZstPath = String.fromEnvironment('RUSTLINE_M6_PACK_ZST');
const enforceBudgets = bool.fromEnvironment('RUSTLINE_M6_ENFORCE');

const knownCodes = {
  'invalid_request',
  'outside_coverage',
  'no_route',
  'date_outside_feed',
  'pack_corrupt',
  'pack_version',
  'internal',
  'pack_missing',
  'pack_io',
  'no_space',
  'pack_too_large',
};

/// Builder (M6): host runs (`flutter test -d flutter-tester`, no Android device)
/// pass the library built by `cargo build --release -p rustline-ffi`.
const ffiLib = String.fromEnvironment('RUSTLINE_FFI_LIB');

bool _inited = false;
Future<void> init() async {
  if (!_inited) {
    await RustLib.init(
        externalLibrary: ffiLib.isEmpty ? null : ExternalLibrary.open(ffiLib));
    _inited = true;
  }
}

/// The pack given by --dart-define, or null after printing `skip:`.
String? envPack({bool zst = false}) {
  final p = zst ? packZstPath : packPath;
  if (p.isEmpty || !File(p).existsSync()) {
    // ignore: avoid_print
    print(
        'skip: ${zst ? 'RUSTLINE_M6_PACK_ZST' : 'RUSTLINE_M6_PACK'} unset or missing');
    return null;
  }
  return p;
}

Future<RustlineEngine> open(String path) => RustlineEngine.open(path: path);

Future<FfiPlanResponse> plan(RustlineEngine e, FfiPlanRequest r) =>
    e.plan(req: r);

Future<FfiPackInfo> info(RustlineEngine e) => e.info();

Future<void> install(String src, String dest) =>
    installPack(srcZst: src, dest: dest).then((_) => null);

/// A request at a Dart DateTime: epoch ms + that DateTime's offset.
FfiPlanRequest req(
    double fLat, double fLon, double tLat, double tLon, DateTime t,
    {int maxResults = 3}) {
  return FfiPlanRequest(
    fromLat: fLat,
    fromLon: fLon,
    toLat: tLat,
    toLon: tLon,
    timeKind: FfiTimeKind.departAt,
    timeMs: t.millisecondsSinceEpoch,
    timeOffsetSecs: t.timeZoneOffset.inSeconds,
    maxResults: maxResults,
    maxAccessM: 400,
    walkSpeedMps: null,
    maxTransfers: null,
  );
}

/// 08:00 at +03:00 on the third day of the pack (Brașov, summer offset is fine: the
/// engine answers in the feed's offset anyway).
Future<DateTime> morningInFeed(RustlineEngine e) async {
  final i = await info(e);
  return DateTime.fromMillisecondsSinceEpoch(
      (i.validFromDays + 2) * 86400000 + 5 * 3600000,
      isUtc: true);
}

/// The stable code of a thrown error.
String codeOf(Object e) {
  // Builder (M6): Rust errors arrive typed (FRB-mapped RustlineException).
  if (e is RustlineException) return e.code;
  final s = e.toString();
  for (final c in knownCodes) {
    if (s.contains(c)) return c;
  }
  throw StateError('error without a stable code: $s');
}

/// Runs `f`, expects it to throw, returns the code.
Future<String> errCode(Future<Object?> Function() f) async {
  try {
    await f();
  } catch (e) {
    return codeOf(e);
  }
  throw StateError('expected an error');
}

RustlinePlanner planner(RustlineEngine e) => RustlinePlanner(engine: e);

/// Builder (M6 review MF3): the debug-only panic hook (a panic in a debug Rust
/// build, `invalid_request` in profile/release).
Future<void> injectPanic() => debugInjectPanic();

/// Brașov city box (lat 45.63..45.68, lon 25.56..25.64): 100 fixed point pairs.
/// Builder (M6): was lat 45.60..45.68, lon 25.52..25.64; its south and west parts
/// are forest and fields, so with 400 m access only 19 of 100 pairs had a route
/// (70 no_route, 11 outside_coverage) and `ok > 50` could not hold.
List<(double, double, double, double)> fixedPairs() {
  var x = 0x6d36;
  double next() {
    x = (x * 1103515245 + 12345) & 0x7fffffff;
    return (x % 10000) / 10000.0;
  }

  return List.generate(
      100,
      (_) => (
            45.63 + next() * 0.05,
            25.56 + next() * 0.08,
            45.63 + next() * 0.05,
            25.56 + next() * 0.08
          ));
}
