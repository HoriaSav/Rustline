import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

import 'rust/frb_generated.dart' as frb;

/// Starts the Rust bridge; call `await RustLib.init()` once before any other
/// call.
///
/// On iOS the Rust library is linked into this plugin's framework
/// (`rustline_flutter.framework`, see ios/rustline_flutter.podspec), not the
/// `rustline_ffi.framework` flutter_rust_bridge looks for, so it is opened
/// here. That needs dynamic frameworks (`use_frameworks!`) in the app's
/// Podfile. Elsewhere flutter_rust_bridge's default loading is used.
abstract final class RustLib {
  static Future<void> init({ExternalLibrary? externalLibrary}) =>
      frb.RustLib.init(
        externalLibrary:
            externalLibrary ?? (Platform.isIOS ? _openIosFramework() : null),
      );

  static void dispose() => frb.RustLib.dispose();
}

const _iosFramework = 'rustline_flutter.framework/rustline_flutter';

ExternalLibrary _openIosFramework() {
  try {
    return ExternalLibrary.open(_iosFramework);
  } on ArgumentError catch (e) {
    throw StateError(
      'rustline_flutter on iOS needs use_frameworks! (dynamic frameworks) in '
      'the app Podfile; static linkage is not supported. Opening '
      '$_iosFramework failed: $e',
    );
  }
}
