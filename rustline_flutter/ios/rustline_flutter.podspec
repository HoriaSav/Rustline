#
# Rustline iOS pod (plan M11): cargokit builds crates/rustline-ffi as a static
# library for the current target (aarch64-apple-ios or aarch64-apple-ios-sim) and
# it is linked into this pod's framework. Based on the flutter_rust_bridge 2.13.0
# cargokit template; the module keeps the plugin's name because of the Swift
# plugin class (backup exclusion), see lib/src/rust_lib.dart. Requires
# `use_frameworks!` (dynamic frameworks, Flutter's default Podfile): with static
# linkage the library is not linked and RustLib.init() fails.
#
Pod::Spec.new do |s|
  s.name             = 'rustline_flutter'
  s.version          = '0.0.1'
  s.summary          = 'Offline GTFS journey planner (RAPTOR + OSM walking) for Flutter, in Rust.'
  s.description      = <<-DESC
Rustline plans public-transport journeys on the device from a compact routing pack built from any GTFS feed; the Rust engine is built by cargokit.
                       DESC
  s.homepage         = 'https://github.com/HoriaSav/Rustline'
  s.license          = { :type => 'Apache-2.0', :file => '../LICENSE' }
  s.author           = { 'Horia Savin' => '155181609+HoriaSav@users.noreply.github.com' }

  s.source           = { :path => '.' }
  s.source_files     = 'Classes/**/*'
  s.dependency 'Flutter'
  s.platform = :ios, '15.0'
  s.swift_version = '5.0'
  # Required-reason APIs used by rustline-ffi (statvfs, file metadata).
  s.resource_bundles = { 'rustline_flutter_privacy' => ['Resources/PrivacyInfo.xcprivacy'] }

  s.script_phase = {
    :name => 'Build Rust library',
    # Path to the crate (relative to this folder) and the Rust library name.
    :script => 'sh "$PODS_TARGET_SRCROOT/../cargokit/build_pod.sh" ../../crates/rustline-ffi rustline_ffi',
    :execution_position => :before_compile,
    :input_files => ['${BUILT_PRODUCTS_DIR}/cargokit_phony'],
    # Tells Xcode that the library used by -force_load below comes from this step.
    :output_files => ["${BUILT_PRODUCTS_DIR}/librustline_ffi.a"],
  }
  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    # Flutter.framework has no i386 slice, and M11 is arm64-only: no x86_64
    # simulator slice, so cargokit builds only aarch64-apple-ios-sim (Flutter's
    # podhelper sets ONLY_ACTIVE_ARCH=NO on Debug pods, which otherwise builds
    # x86_64 too). Not in user_target_xcconfig: the app already builds only the
    # active arch (arm64) for a simulator destination.
    'EXCLUDED_ARCHS[sdk=iphonesimulator*]' => 'i386 x86_64',
    # -force_load keeps the FFI symbols that Dart looks up at runtime.
    'OTHER_LDFLAGS' => '-force_load ${BUILT_PRODUCTS_DIR}/librustline_ffi.a',
  }
end
