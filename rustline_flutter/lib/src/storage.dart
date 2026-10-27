import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

const _storage = MethodChannel('rustline_flutter/storage');

/// The directory to install packs into: `packs/` in Application Support,
/// created if missing (plan M11 decisions).
///
/// Call it on every launch instead of storing the path: the iOS container
/// path changes between installs. On iOS the directory is excluded from
/// iCloud backup (packs can be downloaded again); if that fails, this throws
/// the [PlatformException] instead of returning a backed-up directory.
Future<Directory> defaultPackDirectory() async {
  final support = await getApplicationSupportDirectory();
  final dir = await Directory('${support.path}/packs').create(recursive: true);
  // defaultTargetPlatform, not Platform.isIOS, so host unit tests can drive the
  // iOS branch (debugDefaultTargetPlatformOverride); in an app both agree.
  if (defaultTargetPlatform == TargetPlatform.iOS) {
    await _storage.invokeMethod<void>('excludeFromBackup', dir.path);
  }
  return dir;
}
