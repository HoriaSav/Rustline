// Runs the Dart integration test that `flutter build ios --config-only
// integration_test/<file>.dart` baked into Runner.app, through XCTest
// (packages/integration_test README, "iOS Device Testing"). Each Dart test
// becomes one XCTest case, named after it (testPackPathWithSpacesAndUnicode),
// that fails with the Dart failure message, so `xcodebuild test` fails when a
// Dart test does.

@import XCTest;
@import integration_test;

INTEGRATION_TEST_IOS_RUNNER(RunnerTests)
