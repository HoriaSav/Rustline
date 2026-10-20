//! The FFI layer between Rustline and Flutter (plan M6, spec §11.3).
//!
//! Everything the Dart side sees is in [`api`]: plain data types, every
//! function returns `Result`, errors carry a stable code, and no panic
//! crosses the bridge. `frb_generated` is written by
//! `flutter_rust_bridge_codegen generate` (run in `rustline_flutter/`).

pub mod api;
mod frb_generated;
