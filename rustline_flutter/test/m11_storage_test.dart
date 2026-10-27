// M11 tests-first (Tester, 2026-10-26): host unit tests (no device, no Mac) for the
// parts of iOS pack storage that Linux can check: which directory is chosen and that
// the exclude-from-backup call is made for it. The platform side is a mocked
// channel; the real flag is only checked on the simulator (m11_ios_test.dart, run by
// ios.yml on macOS CI). Product names live in m11_storage_adapter.dart.
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'm11_storage_adapter.dart';

/// An iOS-style container path: a space in "Application Support" and a non-ASCII
/// segment, so encoding bugs (%20, NFD) show up.
const appSupport =
    '/var/mobile/Containers/Data/Application/0A1B-Brașov/Library/Application Support';

String _trimSlash(String p) =>
    p.endsWith('/') ? p.substring(0, p.length - 1) : p;

bool _strictlyInside(String child, String parent) {
  final c = _trimSlash(child), p = _trimSlash(parent);
  return c.startsWith('$p/') && c.length > p.length + 1;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  late List<MethodCall> log;

  setUp(() => log = <MethodCall>[]);
  tearDown(() {
    debugDefaultTargetPlatformOverride = null;
    m11RemoveMocks();
  });

  group('iOS', () {
    setUp(() {
      debugDefaultTargetPlatformOverride = TargetPlatform.iOS;
      m11InstallMocks(appSupport: appSupport, log: log);
    });

    test('packs go in a subdirectory of Application Support', () async {
      final dir = await m11PackDir();
      expect(
        _strictlyInside(dir, appSupport),
        isTrue,
        reason:
            '$dir must be a subdirectory of $appSupport, not App Support '
            'itself (the flag goes on our subdir, never the whole directory)',
      );
      for (final bad in ['/Documents', '/Library/Caches', '/tmp']) {
        expect(dir.contains(bad), isFalse, reason: '$dir must not use $bad');
      }
      expect(
        dir.contains('%20'),
        isFalse,
        reason: 'path must not be URL-encoded',
      );
      expect(
        log.map((c) => c.method),
        contains(m11AppSupportMethod),
        reason: 'the directory must come from the platform, not a guess',
      );
    }, skip: m11StorageSkip);

    test('the same directory every time', () async {
      final a = await m11PackDir();
      final b = await m11PackDir();
      expect(b, a);
    }, skip: m11StorageSkip);

    test(
      'the pack directory is marked excluded from backup',
      () async {
        final dir = await m11PackDir();
        final excl = log.where((c) => c.method == m11ExcludeMethod).toList();
        expect(excl, isNotEmpty, reason: 'no $m11ExcludeMethod call on iOS');
        final paths = excl.map(m11ExcludedPath).toList();
        expect(
          paths,
          contains(_trimSlash(dir)),
          reason: 'exclusion must name the pack directory, verbatim',
        );
        for (final p in paths) {
          expect(
            p == _trimSlash(appSupport),
            isFalse,
            reason: 'never exclude all of Application Support',
          );
        }
        final firstExclude = log.indexWhere(
          (c) => c.method == m11ExcludeMethod,
        );
        final firstSupport = log.indexWhere(
          (c) => c.method == m11AppSupportMethod,
        );
        expect(
          firstSupport,
          lessThan(firstExclude),
          reason: 'resolve the directory before excluding it',
        );
      },
      skip:
          m11StorageSkip ??
          (m11ExclusionViaChannel ? null : 'flag set in Rust'),
    );

    test('a failed exclusion is not silently ignored', () async {
      m11InstallMocks(
        appSupport: appSupport,
        log: log,
        excludeError: PlatformException(
          code: 'io',
          message: 'read-only volume',
        ),
      );
      // Ambiguity (see the Tester note): the plan does not say whether this should
      // fail or warn. Tester's reading: the call must at least be attempted, and
      // the error must surface (throw) rather than vanish, so a broken flag is seen.
      await expectLater(m11PackDir(), throwsA(anything));
      expect(log.map((c) => c.method), contains(m11ExcludeMethod));
    }, skip: m11StorageSkip ?? (m11ExclusionViaChannel ? null : 'flag set in Rust'));
  });

  test('no backup call on Android (iOS-only API)', () async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    m11InstallMocks(appSupport: '/data/user/0/app/files', log: log);
    try {
      await m11PackDir();
    } on MissingPluginException {
      // Android may resolve its directory another way; only the call below matters.
    }
    expect(log.where((c) => c.method == m11ExcludeMethod), isEmpty);
  }, skip: m11StorageSkip);
}
