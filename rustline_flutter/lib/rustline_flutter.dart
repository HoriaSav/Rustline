/// Offline journey planning with Rustline (plan M6, spec §11.3).
///
/// Call `await RustLib.init()` once, then `RustlineEngine.open(path: ...)`.
/// Every Rust error arrives as a [RustlineException] with a stable `code`.
library;

export 'src/planner.dart';
export 'src/rust/api.dart';
export 'src/rust/frb_generated.dart' show RustLib;
// For RustLib.init(externalLibrary: ...) when the library is not bundled
// (host tests load target/release/librustline_ffi.so).
export 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart'
    show ExternalLibrary;
