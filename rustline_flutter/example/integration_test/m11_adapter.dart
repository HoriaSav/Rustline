// M11 tests-first (Tester, 2026-10-26): the one adapter between the M11 iOS
// integration test (m11_ios_test.dart) and the plugin. Builder (M11): wire the two
// functions marked "Builder (M11)" to the real API, then set `m11Skip` to null.
//
// Plan M11 step 3 + Research note (2026-10-26 M11 iOS, §3):
//   - packs live in `Library/Application Support/<subdir>` (not Documents, not Caches);
//   - the pack (or its subdir) is excluded from iCloud backup, and the flag survives
//     the atomic rename of a reinstall (Apple: some file operations reset it);
//   - Research recommends Rust sets the flag inside `install_pack` (objc2,
//     NSURLIsExcludedFromBackupKey) under cfg(target_os = "ios").
//
// Inputs (dart-define):
//   RUSTLINE_M11_PACK_ZST=<host path to a .rlp.zst>  (ios.yml: built from
//       data/fixtures/tiny with `rustline build-pack`; the simulator can read host
//       paths)
//   RUSTLINE_M11_REQUIRE_PACK=true  (ios.yml: a missing pack fails instead of skipping)
//   RUSTLINE_M11_SIM_UDID=<UDID>  (ios.yml: the booted simulator; required with
//       RUSTLINE_M11_REQUIRE_PACK=true)
import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

import 'package:flutter/services.dart';

import 'm6_adapter.dart';

export 'm6_adapter.dart';

/// Set to null when M11 is implemented.
// ignore: unnecessary_nullable_for_final_variable_declarations
const String? m11Skip = null; // M11 implemented (Builder, 2026-10-26).

const m11PackZstPath = String.fromEnvironment('RUSTLINE_M11_PACK_ZST');
const m11RequirePack = bool.fromEnvironment('RUSTLINE_M11_REQUIRE_PACK');

/// The UDID of the simulator the workflow booted. With
/// RUSTLINE_M11_REQUIRE_PACK=true it must be set, and the app's SIMULATOR_UDID
/// must match it.
const m11SimUdid = String.fromEnvironment('RUSTLINE_M11_SIM_UDID');

/// Skip reason for every M11 test: not implemented yet, or not on iOS.
String? get m11SkipReason =>
    m11Skip ?? (Platform.isIOS ? null : 'M11 tests run on iOS only');

/// The tiny fixture pack (.rlp.zst), or null after printing `skip:`. With
/// RUSTLINE_M11_REQUIRE_PACK=true a missing pack is a failure.
String? m11PackZst() {
  if (m11PackZstPath.isNotEmpty && File(m11PackZstPath).existsSync()) {
    return m11PackZstPath;
  }
  if (m11RequirePack) {
    throw StateError(
      'RUSTLINE_M11_PACK_ZST unset or missing ($m11PackZstPath) but '
      'RUSTLINE_M11_REQUIRE_PACK=true',
    );
  }
  // ignore: avoid_print
  print('skip: RUSTLINE_M11_PACK_ZST unset or missing');
  return null;
}

/// Builder (M11): the directory the app installs packs into, from the product
/// code (the plugin's or the example app's pack-location helper), NOT recomputed
/// here, so the test checks the product's choice. It must be derived from the
/// current container on every launch (the sandbox path changes between installs,
/// plan M11 edge cases), never a stored absolute path.
Future<Directory> appPackDir() => defaultPackDirectory();

/// Builder (M11): install the way the app does on iOS. If the backup flag is set by
/// a separate call (not inside install_pack), add that call here.
Future<void> installForApp(String srcZst, String dest) =>
    installPack(srcZst: srcZst, dest: dest).then((_) => null);

// ---- oracles (independent of the product code) -----------------------------

/// `<container>/Library/Application Support`, from the native NSHomeDirectory()
/// (the app's data container) over the example's method channel, so it needs
/// neither path_provider nor the plugin. Dart's `Platform.environment` is always
/// empty on iOS, so HOME can't come from there.
Future<Directory> appSupportOracle() async =>
    Directory('${await m11HomeDirectory()}/Library/Application Support');

/// NSHomeDirectory() from the example's AppDelegate. Throws if unavailable.
Future<String> m11HomeDirectory() => _m11String('homeDirectory');

/// SIMULATOR_DEVICE_NAME (no fallback). Throws if unset or empty.
Future<String> m11SimulatorDeviceName() => _m11String('simulatorDeviceName');

/// SIMULATOR_RUNTIME_VERSION (no fallback). Throws if unset or empty.
Future<String> m11RuntimeVersion() => _m11String('runtimeVersion');

/// Raw device facts from the AppDelegate: `isSimulator` (compile-time
/// targetEnvironment(simulator)), `simulatorDeviceName`,
/// `simulatorModelIdentifier`, `simulatorRuntimeVersion`, `simulatorUdid` (the
/// simulator's own variables, null when unset), and UIDevice's `deviceModel`,
/// `systemName`, `systemVersion`. Throws if unavailable.
Future<Map<String, Object?>> m11DeviceInfo() async {
  final m = await _m11Channel.invokeMapMethod<String, Object?>('deviceInfo');
  if (m == null || m.isEmpty) {
    throw StateError('rustline_m11_test.deviceInfo returned ${m ?? 'null'}');
  }
  return m;
}

/// A non-empty string from the channel; null, empty or a missing handler throw.
Future<String> _m11String(String method) async {
  final v = await _m11Channel.invokeMethod<String>(method);
  if (v == null || v.isEmpty) {
    throw StateError(
      'rustline_m11_test.$method returned ${v == null ? 'null' : 'empty'}',
    );
  }
  return v;
}

/// Canonical path (resolves /var -> /private/var and the like) of an existing
/// entity, or of its nearest existing parent plus the rest.
String canonical(String p) {
  final f = File(p);
  if (FileSystemEntity.typeSync(p) != FileSystemEntityType.notFound) {
    return f.resolveSymbolicLinksSync();
  }
  final parent = f.parent;
  if (parent.path == p) return p;
  return '${canonical(parent.path)}/${f.uri.pathSegments.last}';
}

/// True when `child` is `parent` or inside it (canonical paths).
bool isWithin(String parent, String child) {
  final a = canonical(parent);
  final b = canonical(child);
  return b == a || b.startsWith('$a/');
}

/// The example's AppDelegate handler: `isExcludedFromBackup` (with
/// `URL(fileURLWithPath:).resourceValues(forKeys:
/// [.isExcludedFromBackupKey]).isExcludedFromBackup`), `homeDirectory`,
/// `simulatorDeviceName`, `runtimeVersion`, `deviceInfo`.
const _m11Channel = MethodChannel('rustline_m11_test');

/// Whether `path` itself carries the "exclude from backup" mark.
/// 1. The example's method channel (Apple's API), when Builder adds it.
/// 2. Otherwise the extended attribute that NSURLIsExcludedFromBackupKey writes on
///    APFS, `com.apple.metadata:com_apple_backup_excludeItem` (read with getxattr
///    from libSystem). (unverified on iOS 26: if the flag is set but this reads
///    false, add the channel above.)
Future<bool> isExcludedFromBackup(String path) async {
  try {
    final v = await _m11Channel.invokeMethod<bool>('isExcludedFromBackup', {
      'path': path,
    });
    if (v != null) return v;
  } on MissingPluginException {
    // fall through to the xattr
  }
  return _hasXattr(path, 'com.apple.metadata:com_apple_backup_excludeItem');
}

/// Excluded itself, or through a parent directory up to (and including)
/// Application Support. Research suggests marking the pack subdir.
Future<bool> excludedSelfOrParent(String path) async {
  final root = canonical((await appSupportOracle()).path);
  var p = canonical(path);
  while (true) {
    if (await isExcludedFromBackup(p)) return true;
    if (p == root || !p.startsWith('$root/')) return false;
    p = File(p).parent.path;
  }
}

typedef _GetxattrC =
    IntPtr Function(
      Pointer<Uint8> path,
      Pointer<Uint8> name,
      Pointer<Void> value,
      IntPtr size,
      Uint32 position,
      Int32 options,
    );
typedef _GetxattrD =
    int Function(
      Pointer<Uint8> path,
      Pointer<Uint8> name,
      Pointer<Void> value,
      int size,
      int position,
      int options,
    );

final _libc = DynamicLibrary.process();
final _getxattr = _libc.lookupFunction<_GetxattrC, _GetxattrD>('getxattr');
final _malloc = _libc
    .lookupFunction<
      Pointer<Uint8> Function(IntPtr),
      Pointer<Uint8> Function(int)
    >('malloc');
final _free = _libc
    .lookupFunction<
      Void Function(Pointer<Uint8>),
      void Function(Pointer<Uint8>)
    >('free');

Pointer<Uint8> _cString(String s) {
  final b = utf8.encode(s);
  final p = _malloc(b.length + 1);
  p.asTypedList(b.length + 1)
    ..setAll(0, b)
    ..[b.length] = 0;
  return p;
}

bool _hasXattr(String path, String name) {
  if (FileSystemEntity.typeSync(path) == FileSystemEntityType.notFound) {
    throw StateError('no such file: $path');
  }
  final p = _cString(path);
  final n = _cString(name);
  try {
    // Darwin: getxattr(path, name, NULL, 0, 0, 0) is the value size, or -1.
    return _getxattr(p, n, nullptr, 0, 0, 0) >= 0;
  } finally {
    _free(p);
    _free(n);
  }
}

/// Tiny fixture (data/fixtures/tiny): stops A and D, Wednesday 2030-01-02 07:55
/// in Toronto (-05:00), as the Rust FFI tests use (ffi_common::tiny_req).
FfiPlanRequest tinyReq({int offsetSecs = -5 * 3600, int maxResults = 3}) =>
    FfiPlanRequest(
      fromLat: 10.0,
      fromLon: 20.0,
      toLat: 10.015,
      toLon: 20.01,
      timeKind: FfiTimeKind.departAt,
      timeMs: DateTime.utc(2030, 1, 2, 12, 55).millisecondsSinceEpoch,
      timeOffsetSecs: offsetSecs,
      maxResults: maxResults,
      maxAccessM: 400,
    );
