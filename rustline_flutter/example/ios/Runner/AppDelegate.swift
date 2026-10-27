import Flutter
import UIKit

@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate {
  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)
    if let registrar = engineBridge.pluginRegistry.registrar(forPlugin: "RustlineM11Test") {
      registerM11TestChannel(registrar.messenger())
    }
  }

  // Test oracle for integration_test/m11_adapter.dart: reads the backup flag
  // with Apple's API.
  private func registerM11TestChannel(_ messenger: FlutterBinaryMessenger) {
    let channel = FlutterMethodChannel(name: "rustline_m11_test", binaryMessenger: messenger)
    channel.setMethodCallHandler { call, result in
      guard call.method == "isExcludedFromBackup",
        let path = (call.arguments as? [String: Any])?["path"] as? String
      else {
        result(FlutterMethodNotImplemented)
        return
      }
      do {
        let values = try URL(fileURLWithPath: path).resourceValues(forKeys: [.isExcludedFromBackupKey])
        result(values.isExcludedFromBackup ?? false)
      } catch {
        result(FlutterError(code: "resource_values", message: error.localizedDescription, details: path))
      }
    }
  }
}
