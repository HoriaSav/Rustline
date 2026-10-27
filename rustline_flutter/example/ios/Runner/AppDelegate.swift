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

  // Test oracles for integration_test/m11_adapter.dart: the backup flag read with
  // Apple's API, and the container and simulator facts Dart can't read on iOS
  // (Dart's Platform.environment is always empty there).
  private func registerM11TestChannel(_ messenger: FlutterBinaryMessenger) {
    let channel = FlutterMethodChannel(name: "rustline_m11_test", binaryMessenger: messenger)
    channel.setMethodCallHandler { call, result in
      let env = ProcessInfo.processInfo.environment
      switch call.method {
      case "isExcludedFromBackup":
        guard let path = (call.arguments as? [String: Any])?["path"] as? String else {
          result(FlutterMethodNotImplemented)
          return
        }
        do {
          let values = try URL(fileURLWithPath: path).resourceValues(forKeys: [.isExcludedFromBackupKey])
          result(values.isExcludedFromBackup ?? false)
        } catch {
          result(FlutterError(code: "resource_values", message: error.localizedDescription, details: path))
        }
      case "homeDirectory":
        result(NSHomeDirectory())
      // The simulator's own variables only (nil when unset, so the Dart side
      // throws): no UIDevice fallback that could answer for a missing one.
      case "simulatorDeviceName":
        result(env["SIMULATOR_DEVICE_NAME"])
      case "runtimeVersion":
        result(env["SIMULATOR_RUNTIME_VERSION"])
      case "deviceInfo":
        // Raw values (NSNull when unset); UIDevice only in its own keys.
        #if targetEnvironment(simulator)
          let isSimulator = true
        #else
          let isSimulator = false
        #endif
        let raw: (String) -> Any = { key in env[key] ?? NSNull() }
        let info: [String: Any] = [
          "isSimulator": isSimulator,
          "simulatorDeviceName": raw("SIMULATOR_DEVICE_NAME"),
          "simulatorModelIdentifier": raw("SIMULATOR_MODEL_IDENTIFIER"),
          "simulatorRuntimeVersion": raw("SIMULATOR_RUNTIME_VERSION"),
          "simulatorUdid": raw("SIMULATOR_UDID"),
          "deviceModel": UIDevice.current.model,
          "systemName": UIDevice.current.systemName,
          "systemVersion": UIDevice.current.systemVersion,
        ]
        result(info)
      default:
        result(FlutterMethodNotImplemented)
      }
    }
  }
}
