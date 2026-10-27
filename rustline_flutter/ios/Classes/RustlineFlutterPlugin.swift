// Pack storage on iOS (plan M11 step 3): marks the pack directory as excluded
// from iCloud backup for defaultPackDirectory() in lib/src/storage.dart. The
// Rust library itself is linked by the podspec's cargokit script phase.
import Flutter
import Foundation

public class RustlineFlutterPlugin: NSObject, FlutterPlugin {
  public static func register(with registrar: FlutterPluginRegistrar) {
    let channel = FlutterMethodChannel(
      name: "rustline_flutter/storage", binaryMessenger: registrar.messenger())
    registrar.addMethodCallDelegate(RustlineFlutterPlugin(), channel: channel)
  }

  public func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    guard call.method == "excludeFromBackup", let path = call.arguments as? String else {
      result(FlutterMethodNotImplemented)
      return
    }
    // The item must exist; a failure goes back to Dart as a PlatformException.
    var url = URL(fileURLWithPath: path)
    var values = URLResourceValues()
    values.isExcludedFromBackup = true
    do {
      try url.setResourceValues(values)
      result(nil)
    } catch {
      result(
        FlutterError(
          code: "exclude_from_backup", message: error.localizedDescription, details: path))
    }
  }
}
