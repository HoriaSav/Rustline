// M11 tests-first (Tester, 2026-10-26): the iOS side of plan M11, run by ios.yml on an
// iPhone 17 simulator (macos-26, Xcode 26.6). Skipped elsewhere (Android emulator,
// host) and until Builder sets `m11Skip` to null in m11_adapter.dart.
// Covers: pack stored in Application Support and excluded from backup (also after a
// reinstall), a plan through the FFI on the simulator, the Android FFI checks on iOS
// (error codes, the 256 MiB cap path, two writers on one pack, cancellation), a
// killed install's temp file, spaces/Unicode in the path, device tz != feed tz.
// Run (on a Mac): cd rustline_flutter/example && flutter test integration_test -d <sim> \
//   --dart-define=RUSTLINE_M11_PACK_ZST=<abs path to tiny .rlp.zst> \
//   --dart-define=RUSTLINE_M11_REQUIRE_PACK=true
import 'dart:ffi';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'm11_adapter.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async {
    if (m11SkipReason == null) await init();
  });

  // Each test gets a fresh subdir of the product's pack dir and removes it after.
  late Directory dir;
  setUp(() async {
    if (m11SkipReason != null) return;
    final base = await appPackDir();
    dir = await Directory(
      '${base.path}/m11_${DateTime.now().microsecondsSinceEpoch}',
    ).create(recursive: true);
  });
  tearDown(() async {
    if (m11SkipReason != null) return;
    if (dir.existsSync()) await dir.delete(recursive: true);
  });

  Set<String> names(Directory d) =>
      d.listSync().map((f) => f.uri.pathSegments.last).toSet();

  testWidgets('runs on an arm64 iOS simulator (records the device)', (t) async {
    final abi = Abi.current();
    // ignore: avoid_print
    print(
      'm11: abi=$abi device=${Platform.environment['SIMULATOR_DEVICE_NAME']} '
      'runtime=${Platform.environment['SIMULATOR_RUNTIME_VERSION']} '
      'home=${Platform.environment['HOME']}',
    );
    expect(abi, anyOf(Abi.iosArm64, Abi.iosX64));
    if (m11RequirePack) {
      // CI (ios.yml): Apple Silicon runner, iPhone 17 simulator (plan M11 edge case
      // "arm64-sim vs x86_64": the arm64 slice is the one CI can run).
      expect(Platform.environment['SIMULATOR_DEVICE_NAME'], 'iPhone 17');
      expect(abi, Abi.iosArm64);
    }
  }, skip: m11SkipReason != null);

  testWidgets('the pack dir is in Application Support of this container', (
    t,
  ) async {
    final support = appSupportOracle().path;
    final base = (await appPackDir()).path;
    expect(
      isWithin(support, base),
      isTrue,
      reason: '$base is not under $support',
    );
    // Not the backed-up, user-visible Documents, not the purgeable Caches/tmp.
    final home = Platform.environment['HOME']!;
    for (final bad in ['Documents', 'Library/Caches', 'tmp']) {
      expect(isWithin('$home/$bad', base), isFalse, reason: 'under $bad');
    }
    // Derived from the current container (its path changes between installs).
    expect(isWithin(home, base), isTrue);
  }, skip: m11SkipReason != null);

  testWidgets('install into App Support, open it and plan (FFI on iOS)', (
    t,
  ) async {
    final z = m11PackZst();
    if (z == null) return;
    final dest = '${dir.path}/tiny.rlp';
    await installForApp(z, dest);
    expect(isWithin(appSupportOracle().path, dest), isTrue);
    expect(names(dir), {'tiny.rlp', 'tiny.rlp.lock'});
    final e = await open(dest);
    final i = await info(e);
    expect(i.timezone, 'America/Toronto');
    expect(i.stops, 5);
    final r = await plan(e, tinyReq());
    expect(r.itineraries, isNotEmpty);
    for (final it in r.itineraries) {
      expect(it.arrivalMs, greaterThan(it.departureMs));
      expect(
        it.departureMs,
        greaterThanOrEqualTo(
          DateTime.utc(2030, 1, 2, 12, 55).millisecondsSinceEpoch,
        ),
      );
      expect(it.legs, isNotEmpty);
    }
  }, skip: m11SkipReason != null);

  testWidgets(
    'the installed pack is excluded from backup, also after a reinstall',
    (t) async {
      final z = m11PackZst();
      if (z == null) return;
      // Control: the oracle reads false for an ordinary new file, so a true below
      // is the product's doing.
      final plain = File('${dir.path}/plain.bin')..writeAsBytesSync([1, 2, 3]);
      expect(
        await isExcludedFromBackup(plain.path),
        isFalse,
        reason: 'oracle says a fresh file is excluded',
      );
      plain.deleteSync();

      final dest = '${dir.path}/tiny.rlp';
      await installForApp(z, dest);
      expect(
        await excludedSelfOrParent(dest),
        isTrue,
        reason: 'pack (or its folder) not excluded from iCloud backup',
      );
      // A reinstall renames a new file over the pack: the flag must still hold.
      await installForApp(z, dest);
      expect(
        await excludedSelfOrParent(dest),
        isTrue,
        reason: 'flag lost after the atomic replace of a reinstall',
      );
      expect(await open(dest), isNotNull);
    },
    skip: m11SkipReason != null,
  );

  testWidgets('device timezone differs from the feed: answers in feed time', (
    t,
  ) async {
    final z = m11PackZst();
    if (z == null) return;
    final dest = '${dir.path}/tiny.rlp';
    await installForApp(z, dest);
    final e = await open(dest);
    // ignore: avoid_print
    print('m11: device tz ${DateTime.now().timeZoneName}');
    final toronto = await plan(e, tinyReq());
    final utc = await plan(e, tinyReq(offsetSecs: 0));
    final local = await plan(
      e,
      tinyReq(offsetSecs: DateTime.now().timeZoneOffset.inSeconds),
    );
    expect(toronto.itineraries, isNotEmpty);
    for (final r in [utc, local]) {
      expect(
        r.itineraries.map((i) => (i.departureMs, i.arrivalMs)),
        toronto.itineraries.map((i) => (i.departureMs, i.arrivalMs)),
      );
    }
    for (final i in toronto.itineraries) {
      expect(i.departureOffsetSecs, -5 * 3600, reason: 'feed offset (EST)');
      expect(i.arrivalOffsetSecs, -5 * 3600);
    }
  }, skip: m11SkipReason != null);

  testWidgets('error codes on iOS, the pack stays intact', (t) async {
    final z = m11PackZst();
    if (z == null) return;
    final dest = '${dir.path}/tiny.rlp';
    await installForApp(z, dest);
    final good = await File(dest).readAsBytes();
    final e = await open(dest);

    expect(
      await errCode(() => open('${dir.path}/missing.rlp')),
      anyOf('pack_missing', 'pack_io'),
    );
    final junk = File('${dir.path}/junk.rlp')
      ..writeAsBytesSync(List.filled(64, 7));
    expect(await errCode(() => open(junk.path)), 'pack_corrupt');
    expect(
      await errCode(
        () => RustlineEngine.openWithLimit(
          path: dest,
          maxBodyBytes: BigInt.from(64),
        ),
      ),
      'pack_too_large',
    );
    final nan = FfiPlanRequest(
      fromLat: double.nan,
      fromLon: 20.0,
      toLat: 10.015,
      toLon: 20.01,
      timeKind: FfiTimeKind.departAt,
      timeMs: DateTime.utc(2030, 1, 2, 12, 55).millisecondsSinceEpoch,
      timeOffsetSecs: -5 * 3600,
      maxResults: 3,
    );
    expect(await errCode(() => plan(e, nan)), 'invalid_request');
    expect(
      await errCode(() => plan(e, tinyReq(maxResults: 0))),
      'invalid_request',
    );

    // A bad download never replaces the installed pack.
    final zb = await File(z).readAsBytes();
    final bad = File('${dir.path}/bad.rlp.zst')
      ..writeAsBytesSync(zb.sublist(0, zb.length ~/ 2));
    expect(
      await errCode(() => installForApp(bad.path, dest)),
      anyOf('pack_corrupt', 'pack_io'),
    );
    expect(await File(dest).readAsBytes(), good);
    if (kDebugMode) {
      expect(await errCode(injectPanic), 'internal');
    }
    // Still alive and planning.
    expect((await plan(e, tinyReq())).itineraries, isNotEmpty);
  }, skip: m11SkipReason != null);

  testWidgets('two installs of one pack at once (flock in the iOS sandbox)', (
    t,
  ) async {
    final z = m11PackZst();
    if (z == null) return;
    final dest = '${dir.path}/tiny.rlp';
    await Future.wait([installForApp(z, dest), installForApp(z, dest)]);
    expect(names(dir), {
      'tiny.rlp',
      'tiny.rlp.lock',
    }, reason: 'no temp files left by either writer');
    expect((await plan(await open(dest), tinyReq())).itineraries, isNotEmpty);
  }, skip: m11SkipReason != null);

  testWidgets('relaunch after a kill during install: old temp file is removed', (
    t,
  ) async {
    final z = m11PackZst();
    if (z == null) return;
    final dest = '${dir.path}/tiny.rlp';
    // What a killed install leaves: its `{name}.{pid}.{n}.tmp` and a half pack.
    File(
      '${dir.path}/tiny.rlp.99999.0.tmp',
    ).writeAsBytesSync(List.filled(100, 1));
    File(dest).writeAsBytesSync(List.filled(100, 1));
    expect(await errCode(() => open(dest)), 'pack_corrupt');
    await installForApp(z, dest);
    expect(names(dir), {'tiny.rlp', 'tiny.rlp.lock'});
    expect(await open(dest), isNotNull);
  }, skip: m11SkipReason != null);

  testWidgets('pack path with spaces and Unicode', (t) async {
    final z = m11PackZst();
    if (z == null) return;
    final sub = await Directory('${dir.path}/Brașov ü 東京 packs').create();
    final dest = '${sub.path}/oraș nou.rlp';
    await installForApp(z, dest);
    expect(await excludedSelfOrParent(dest), isTrue);
    final e = await open(dest);
    expect((await plan(e, tinyReq())).itineraries, isNotEmpty);
  }, skip: m11SkipReason != null);

  testWidgets('a superseded plan resolves to null (latest wins) on iOS', (
    t,
  ) async {
    final z = m11PackZst();
    if (z == null) return;
    final dest = '${dir.path}/tiny.rlp';
    await installForApp(z, dest);
    final pl = planner(await open(dest));
    final first = pl.plan(tinyReq());
    final second = pl.plan(tinyReq(offsetSecs: 0));
    expect(await first, isNull, reason: 'stale result must be dropped');
    expect((await second)!.itineraries, isNotEmpty);
  }, skip: m11SkipReason != null);
}
