import 'rust/api.dart';

/// Latest wins: when a new query starts before the previous one answered,
/// the previous one resolves to null (plan M6 "cancellation"). Each Rust call
/// is bounded (≤ 1 s), so a superseded call ends on its own.
class RustlinePlanner {
  RustlinePlanner({required this.engine});

  final RustlineEngine engine;
  int _latest = 0;

  /// Drops the query in flight (its answer resolves to null), e.g. when the
  /// user leaves the screen.
  void cancel() => _latest++;

  /// The answer, or null when a newer [plan] call superseded this one.
  /// Errors of a superseded query are dropped too.
  Future<FfiPlanResponse?> plan(FfiPlanRequest req) async {
    final id = ++_latest;
    try {
      final r = await engine.plan(req: req);
      return id == _latest ? r : null;
    } catch (_) {
      if (id != _latest) return null;
      rethrow;
    }
  }
}
