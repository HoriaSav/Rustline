// M11 tests-first (Tester, 2026-10-26): the names m11_storage_test.dart needs from
// the product, kept in one place. Builder: wire the two hooks to the product and set
// m11StorageSkip to null when M11 lands.
//
// Contract assumed (plan M11 step 3, Research note "2026-10-26 M11 iOS"): on iOS the
// plugin keeps packs in a subdirectory of Library/Application Support, asks the
// platform for that directory and asks it to mark the pack directory excluded from
// iCloud backup (NSURLIsExcludedFromBackupKey). The plan names neither API, so the
// channel and method names below are placeholders: change them to the product's.
// If the flag is set from Rust (objc2, Research's recommendation) rather than over a
// channel, set m11ExclusionViaChannel = false: the exclusion-call tests then skip and
// the simulator test (example/integration_test/m11_ios_test.dart) covers the flag.
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:path_provider_platform_interface/path_provider_platform_interface.dart';
import 'package:plugin_platform_interface/plugin_platform_interface.dart';
import 'package:rustline_flutter/rustline_flutter.dart';

// ignore: unnecessary_nullable_for_final_variable_declarations
const String? m11StorageSkip = null; // M11 implemented (Builder, 2026-10-26).

/// The product's storage channel and its two methods (placeholders).
const MethodChannel m11StorageChannel = MethodChannel(
  'rustline_flutter/storage',
);
const String m11AppSupportMethod = 'applicationSupportDirectory';
const String m11ExcludeMethod = 'excludeFromBackup';

/// False if the backup flag is set in Rust, not over [m11StorageChannel].
const bool m11ExclusionViaChannel = true;

/// Builder: the product's "where do packs live" call (e.g. a static on the plugin).
/// On iOS it must create the directory's path under Application Support and mark it
/// excluded from backup before returning.
/// (Builder, M11: `defaultPackDirectory()`. The fake container path cannot be
/// created on the host, so directories are faked with [IOOverrides] here.)
Future<String> m11PackDir() => IOOverrides.runZoned(
  () async => (await defaultPackDirectory()).path,
  createDirectory: _FakeDir.new,
);

class _FakeDir extends Fake implements Directory {
  _FakeDir(this.path);

  @override
  final String path;

  @override
  Future<Directory> create({bool recursive = false}) async => this;
}

/// Fakes the platform side: answers [m11AppSupportMethod] with [appSupport] and
/// records every call in [log]. [excludeError] makes [m11ExcludeMethod] fail.
/// Builder (M11): the product reads the directory with path_provider, so
/// [_FakePathProvider] answers it and logs the same [m11AppSupportMethod] call.
void m11InstallMocks({
  required String appSupport,
  required List<MethodCall> log,
  PlatformException? excludeError,
}) {
  PathProviderPlatform.instance = _FakePathProvider(appSupport, log);
  TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
      .setMockMethodCallHandler(m11StorageChannel, (call) async {
        log.add(call);
        if (call.method == m11AppSupportMethod) return appSupport;
        if (call.method == m11ExcludeMethod) {
          if (excludeError != null) throw excludeError;
          return true;
        }
        throw MissingPluginException(call.method);
      });
}

class _FakePathProvider extends PathProviderPlatform
    with MockPlatformInterfaceMixin {
  _FakePathProvider(this.appSupport, this.log);

  final String appSupport;
  final List<MethodCall> log;

  @override
  Future<String?> getApplicationSupportPath() async {
    log.add(const MethodCall(m11AppSupportMethod));
    return appSupport;
  }
}

void m11RemoveMocks() {
  TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
      .setMockMethodCallHandler(m11StorageChannel, null);
}

/// The path an exclusion call names, whether the product sends a bare String or a
/// map with a 'path' (or 'url') key.
String? m11ExcludedPath(MethodCall call) {
  final a = call.arguments;
  if (a is String) return a;
  if (a is Map) return (a['path'] ?? a['url']) as String?;
  return null;
}
