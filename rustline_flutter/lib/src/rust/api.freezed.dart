// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'api.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

T _$identity<T>(T value) => value;

final _privateConstructorUsedError = UnsupportedError(
  'It seems like you constructed your class using `MyClass._()`. This constructor is only meant to be used by freezed and you are not supposed to need it nor use it.\nPlease check the documentation here for more information: https://github.com/rrousselGit/freezed#adding-getters-and-methods-to-our-models',
);

/// @nodoc
mixin _$FfiLeg {
  FfiStop? get fromStop => throw _privateConstructorUsedError;
  FfiStop? get toStop => throw _privateConstructorUsedError;
  List<FfiLatLon> get geometry => throw _privateConstructorUsedError;
  @optionalTypeArgs
  TResult when<TResult extends Object?>({
    required TResult Function(
      int startMs,
      int startOffsetSecs,
      int endMs,
      int endOffsetSecs,
      int distanceM,
      FfiStop? fromStop,
      FfiStop? toStop,
      List<FfiLatLon> geometry,
    )
    walk,
    required TResult Function(
      String tripId,
      String routeId,
      String routeName,
      String headsign,
      FfiStop fromStop,
      FfiStop toStop,
      int boardMs,
      int boardOffsetSecs,
      int alightMs,
      int alightOffsetSecs,
      List<FfiLatLon> geometry,
    )
    transit,
  }) => throw _privateConstructorUsedError;
  @optionalTypeArgs
  TResult? whenOrNull<TResult extends Object?>({
    TResult? Function(
      int startMs,
      int startOffsetSecs,
      int endMs,
      int endOffsetSecs,
      int distanceM,
      FfiStop? fromStop,
      FfiStop? toStop,
      List<FfiLatLon> geometry,
    )?
    walk,
    TResult? Function(
      String tripId,
      String routeId,
      String routeName,
      String headsign,
      FfiStop fromStop,
      FfiStop toStop,
      int boardMs,
      int boardOffsetSecs,
      int alightMs,
      int alightOffsetSecs,
      List<FfiLatLon> geometry,
    )?
    transit,
  }) => throw _privateConstructorUsedError;
  @optionalTypeArgs
  TResult maybeWhen<TResult extends Object?>({
    TResult Function(
      int startMs,
      int startOffsetSecs,
      int endMs,
      int endOffsetSecs,
      int distanceM,
      FfiStop? fromStop,
      FfiStop? toStop,
      List<FfiLatLon> geometry,
    )?
    walk,
    TResult Function(
      String tripId,
      String routeId,
      String routeName,
      String headsign,
      FfiStop fromStop,
      FfiStop toStop,
      int boardMs,
      int boardOffsetSecs,
      int alightMs,
      int alightOffsetSecs,
      List<FfiLatLon> geometry,
    )?
    transit,
    required TResult orElse(),
  }) => throw _privateConstructorUsedError;
  @optionalTypeArgs
  TResult map<TResult extends Object?>({
    required TResult Function(FfiLeg_Walk value) walk,
    required TResult Function(FfiLeg_Transit value) transit,
  }) => throw _privateConstructorUsedError;
  @optionalTypeArgs
  TResult? mapOrNull<TResult extends Object?>({
    TResult? Function(FfiLeg_Walk value)? walk,
    TResult? Function(FfiLeg_Transit value)? transit,
  }) => throw _privateConstructorUsedError;
  @optionalTypeArgs
  TResult maybeMap<TResult extends Object?>({
    TResult Function(FfiLeg_Walk value)? walk,
    TResult Function(FfiLeg_Transit value)? transit,
    required TResult orElse(),
  }) => throw _privateConstructorUsedError;

  /// Create a copy of FfiLeg
  /// with the given fields replaced by the non-null parameter values.
  @JsonKey(includeFromJson: false, includeToJson: false)
  $FfiLegCopyWith<FfiLeg> get copyWith => throw _privateConstructorUsedError;
}

/// @nodoc
abstract class $FfiLegCopyWith<$Res> {
  factory $FfiLegCopyWith(FfiLeg value, $Res Function(FfiLeg) then) =
      _$FfiLegCopyWithImpl<$Res, FfiLeg>;
  @useResult
  $Res call({FfiStop fromStop, FfiStop toStop, List<FfiLatLon> geometry});
}

/// @nodoc
class _$FfiLegCopyWithImpl<$Res, $Val extends FfiLeg>
    implements $FfiLegCopyWith<$Res> {
  _$FfiLegCopyWithImpl(this._value, this._then);

  // ignore: unused_field
  final $Val _value;
  // ignore: unused_field
  final $Res Function($Val) _then;

  /// Create a copy of FfiLeg
  /// with the given fields replaced by the non-null parameter values.
  @pragma('vm:prefer-inline')
  @override
  $Res call({
    Object? fromStop = null,
    Object? toStop = null,
    Object? geometry = null,
  }) {
    return _then(
      _value.copyWith(
            fromStop: null == fromStop
                ? _value.fromStop!
                : fromStop // ignore: cast_nullable_to_non_nullable
                      as FfiStop,
            toStop: null == toStop
                ? _value.toStop!
                : toStop // ignore: cast_nullable_to_non_nullable
                      as FfiStop,
            geometry: null == geometry
                ? _value.geometry
                : geometry // ignore: cast_nullable_to_non_nullable
                      as List<FfiLatLon>,
          )
          as $Val,
    );
  }
}

/// @nodoc
abstract class _$$FfiLeg_WalkImplCopyWith<$Res>
    implements $FfiLegCopyWith<$Res> {
  factory _$$FfiLeg_WalkImplCopyWith(
    _$FfiLeg_WalkImpl value,
    $Res Function(_$FfiLeg_WalkImpl) then,
  ) = __$$FfiLeg_WalkImplCopyWithImpl<$Res>;
  @override
  @useResult
  $Res call({
    int startMs,
    int startOffsetSecs,
    int endMs,
    int endOffsetSecs,
    int distanceM,
    FfiStop? fromStop,
    FfiStop? toStop,
    List<FfiLatLon> geometry,
  });
}

/// @nodoc
class __$$FfiLeg_WalkImplCopyWithImpl<$Res>
    extends _$FfiLegCopyWithImpl<$Res, _$FfiLeg_WalkImpl>
    implements _$$FfiLeg_WalkImplCopyWith<$Res> {
  __$$FfiLeg_WalkImplCopyWithImpl(
    _$FfiLeg_WalkImpl _value,
    $Res Function(_$FfiLeg_WalkImpl) _then,
  ) : super(_value, _then);

  /// Create a copy of FfiLeg
  /// with the given fields replaced by the non-null parameter values.
  @pragma('vm:prefer-inline')
  @override
  $Res call({
    Object? startMs = null,
    Object? startOffsetSecs = null,
    Object? endMs = null,
    Object? endOffsetSecs = null,
    Object? distanceM = null,
    Object? fromStop = freezed,
    Object? toStop = freezed,
    Object? geometry = null,
  }) {
    return _then(
      _$FfiLeg_WalkImpl(
        startMs: null == startMs
            ? _value.startMs
            : startMs // ignore: cast_nullable_to_non_nullable
                  as int,
        startOffsetSecs: null == startOffsetSecs
            ? _value.startOffsetSecs
            : startOffsetSecs // ignore: cast_nullable_to_non_nullable
                  as int,
        endMs: null == endMs
            ? _value.endMs
            : endMs // ignore: cast_nullable_to_non_nullable
                  as int,
        endOffsetSecs: null == endOffsetSecs
            ? _value.endOffsetSecs
            : endOffsetSecs // ignore: cast_nullable_to_non_nullable
                  as int,
        distanceM: null == distanceM
            ? _value.distanceM
            : distanceM // ignore: cast_nullable_to_non_nullable
                  as int,
        fromStop: freezed == fromStop
            ? _value.fromStop
            : fromStop // ignore: cast_nullable_to_non_nullable
                  as FfiStop?,
        toStop: freezed == toStop
            ? _value.toStop
            : toStop // ignore: cast_nullable_to_non_nullable
                  as FfiStop?,
        geometry: null == geometry
            ? _value._geometry
            : geometry // ignore: cast_nullable_to_non_nullable
                  as List<FfiLatLon>,
      ),
    );
  }
}

/// @nodoc

class _$FfiLeg_WalkImpl extends FfiLeg_Walk {
  const _$FfiLeg_WalkImpl({
    required this.startMs,
    required this.startOffsetSecs,
    required this.endMs,
    required this.endOffsetSecs,
    required this.distanceM,
    this.fromStop,
    this.toStop,
    required final List<FfiLatLon> geometry,
  }) : _geometry = geometry,
       super._();

  @override
  final int startMs;
  @override
  final int startOffsetSecs;
  @override
  final int endMs;
  @override
  final int endOffsetSecs;
  @override
  final int distanceM;
  @override
  final FfiStop? fromStop;
  @override
  final FfiStop? toStop;
  final List<FfiLatLon> _geometry;
  @override
  List<FfiLatLon> get geometry {
    if (_geometry is EqualUnmodifiableListView) return _geometry;
    // ignore: implicit_dynamic_type
    return EqualUnmodifiableListView(_geometry);
  }

  @override
  String toString() {
    return 'FfiLeg.walk(startMs: $startMs, startOffsetSecs: $startOffsetSecs, endMs: $endMs, endOffsetSecs: $endOffsetSecs, distanceM: $distanceM, fromStop: $fromStop, toStop: $toStop, geometry: $geometry)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$FfiLeg_WalkImpl &&
            (identical(other.startMs, startMs) || other.startMs == startMs) &&
            (identical(other.startOffsetSecs, startOffsetSecs) ||
                other.startOffsetSecs == startOffsetSecs) &&
            (identical(other.endMs, endMs) || other.endMs == endMs) &&
            (identical(other.endOffsetSecs, endOffsetSecs) ||
                other.endOffsetSecs == endOffsetSecs) &&
            (identical(other.distanceM, distanceM) ||
                other.distanceM == distanceM) &&
            (identical(other.fromStop, fromStop) ||
                other.fromStop == fromStop) &&
            (identical(other.toStop, toStop) || other.toStop == toStop) &&
            const DeepCollectionEquality().equals(other._geometry, _geometry));
  }

  @override
  int get hashCode => Object.hash(
    runtimeType,
    startMs,
    startOffsetSecs,
    endMs,
    endOffsetSecs,
    distanceM,
    fromStop,
    toStop,
    const DeepCollectionEquality().hash(_geometry),
  );

  /// Create a copy of FfiLeg
  /// with the given fields replaced by the non-null parameter values.
  @JsonKey(includeFromJson: false, includeToJson: false)
  @override
  @pragma('vm:prefer-inline')
  _$$FfiLeg_WalkImplCopyWith<_$FfiLeg_WalkImpl> get copyWith =>
      __$$FfiLeg_WalkImplCopyWithImpl<_$FfiLeg_WalkImpl>(this, _$identity);

  @override
  @optionalTypeArgs
  TResult when<TResult extends Object?>({
    required TResult Function(
      int startMs,
      int startOffsetSecs,
      int endMs,
      int endOffsetSecs,
      int distanceM,
      FfiStop? fromStop,
      FfiStop? toStop,
      List<FfiLatLon> geometry,
    )
    walk,
    required TResult Function(
      String tripId,
      String routeId,
      String routeName,
      String headsign,
      FfiStop fromStop,
      FfiStop toStop,
      int boardMs,
      int boardOffsetSecs,
      int alightMs,
      int alightOffsetSecs,
      List<FfiLatLon> geometry,
    )
    transit,
  }) {
    return walk(
      startMs,
      startOffsetSecs,
      endMs,
      endOffsetSecs,
      distanceM,
      fromStop,
      toStop,
      geometry,
    );
  }

  @override
  @optionalTypeArgs
  TResult? whenOrNull<TResult extends Object?>({
    TResult? Function(
      int startMs,
      int startOffsetSecs,
      int endMs,
      int endOffsetSecs,
      int distanceM,
      FfiStop? fromStop,
      FfiStop? toStop,
      List<FfiLatLon> geometry,
    )?
    walk,
    TResult? Function(
      String tripId,
      String routeId,
      String routeName,
      String headsign,
      FfiStop fromStop,
      FfiStop toStop,
      int boardMs,
      int boardOffsetSecs,
      int alightMs,
      int alightOffsetSecs,
      List<FfiLatLon> geometry,
    )?
    transit,
  }) {
    return walk?.call(
      startMs,
      startOffsetSecs,
      endMs,
      endOffsetSecs,
      distanceM,
      fromStop,
      toStop,
      geometry,
    );
  }

  @override
  @optionalTypeArgs
  TResult maybeWhen<TResult extends Object?>({
    TResult Function(
      int startMs,
      int startOffsetSecs,
      int endMs,
      int endOffsetSecs,
      int distanceM,
      FfiStop? fromStop,
      FfiStop? toStop,
      List<FfiLatLon> geometry,
    )?
    walk,
    TResult Function(
      String tripId,
      String routeId,
      String routeName,
      String headsign,
      FfiStop fromStop,
      FfiStop toStop,
      int boardMs,
      int boardOffsetSecs,
      int alightMs,
      int alightOffsetSecs,
      List<FfiLatLon> geometry,
    )?
    transit,
    required TResult orElse(),
  }) {
    if (walk != null) {
      return walk(
        startMs,
        startOffsetSecs,
        endMs,
        endOffsetSecs,
        distanceM,
        fromStop,
        toStop,
        geometry,
      );
    }
    return orElse();
  }

  @override
  @optionalTypeArgs
  TResult map<TResult extends Object?>({
    required TResult Function(FfiLeg_Walk value) walk,
    required TResult Function(FfiLeg_Transit value) transit,
  }) {
    return walk(this);
  }

  @override
  @optionalTypeArgs
  TResult? mapOrNull<TResult extends Object?>({
    TResult? Function(FfiLeg_Walk value)? walk,
    TResult? Function(FfiLeg_Transit value)? transit,
  }) {
    return walk?.call(this);
  }

  @override
  @optionalTypeArgs
  TResult maybeMap<TResult extends Object?>({
    TResult Function(FfiLeg_Walk value)? walk,
    TResult Function(FfiLeg_Transit value)? transit,
    required TResult orElse(),
  }) {
    if (walk != null) {
      return walk(this);
    }
    return orElse();
  }
}

abstract class FfiLeg_Walk extends FfiLeg {
  const factory FfiLeg_Walk({
    required final int startMs,
    required final int startOffsetSecs,
    required final int endMs,
    required final int endOffsetSecs,
    required final int distanceM,
    final FfiStop? fromStop,
    final FfiStop? toStop,
    required final List<FfiLatLon> geometry,
  }) = _$FfiLeg_WalkImpl;
  const FfiLeg_Walk._() : super._();

  int get startMs;
  int get startOffsetSecs;
  int get endMs;
  int get endOffsetSecs;
  int get distanceM;
  @override
  FfiStop? get fromStop;
  @override
  FfiStop? get toStop;
  @override
  List<FfiLatLon> get geometry;

  /// Create a copy of FfiLeg
  /// with the given fields replaced by the non-null parameter values.
  @override
  @JsonKey(includeFromJson: false, includeToJson: false)
  _$$FfiLeg_WalkImplCopyWith<_$FfiLeg_WalkImpl> get copyWith =>
      throw _privateConstructorUsedError;
}

/// @nodoc
abstract class _$$FfiLeg_TransitImplCopyWith<$Res>
    implements $FfiLegCopyWith<$Res> {
  factory _$$FfiLeg_TransitImplCopyWith(
    _$FfiLeg_TransitImpl value,
    $Res Function(_$FfiLeg_TransitImpl) then,
  ) = __$$FfiLeg_TransitImplCopyWithImpl<$Res>;
  @override
  @useResult
  $Res call({
    String tripId,
    String routeId,
    String routeName,
    String headsign,
    FfiStop fromStop,
    FfiStop toStop,
    int boardMs,
    int boardOffsetSecs,
    int alightMs,
    int alightOffsetSecs,
    List<FfiLatLon> geometry,
  });
}

/// @nodoc
class __$$FfiLeg_TransitImplCopyWithImpl<$Res>
    extends _$FfiLegCopyWithImpl<$Res, _$FfiLeg_TransitImpl>
    implements _$$FfiLeg_TransitImplCopyWith<$Res> {
  __$$FfiLeg_TransitImplCopyWithImpl(
    _$FfiLeg_TransitImpl _value,
    $Res Function(_$FfiLeg_TransitImpl) _then,
  ) : super(_value, _then);

  /// Create a copy of FfiLeg
  /// with the given fields replaced by the non-null parameter values.
  @pragma('vm:prefer-inline')
  @override
  $Res call({
    Object? tripId = null,
    Object? routeId = null,
    Object? routeName = null,
    Object? headsign = null,
    Object? fromStop = null,
    Object? toStop = null,
    Object? boardMs = null,
    Object? boardOffsetSecs = null,
    Object? alightMs = null,
    Object? alightOffsetSecs = null,
    Object? geometry = null,
  }) {
    return _then(
      _$FfiLeg_TransitImpl(
        tripId: null == tripId
            ? _value.tripId
            : tripId // ignore: cast_nullable_to_non_nullable
                  as String,
        routeId: null == routeId
            ? _value.routeId
            : routeId // ignore: cast_nullable_to_non_nullable
                  as String,
        routeName: null == routeName
            ? _value.routeName
            : routeName // ignore: cast_nullable_to_non_nullable
                  as String,
        headsign: null == headsign
            ? _value.headsign
            : headsign // ignore: cast_nullable_to_non_nullable
                  as String,
        fromStop: null == fromStop
            ? _value.fromStop
            : fromStop // ignore: cast_nullable_to_non_nullable
                  as FfiStop,
        toStop: null == toStop
            ? _value.toStop
            : toStop // ignore: cast_nullable_to_non_nullable
                  as FfiStop,
        boardMs: null == boardMs
            ? _value.boardMs
            : boardMs // ignore: cast_nullable_to_non_nullable
                  as int,
        boardOffsetSecs: null == boardOffsetSecs
            ? _value.boardOffsetSecs
            : boardOffsetSecs // ignore: cast_nullable_to_non_nullable
                  as int,
        alightMs: null == alightMs
            ? _value.alightMs
            : alightMs // ignore: cast_nullable_to_non_nullable
                  as int,
        alightOffsetSecs: null == alightOffsetSecs
            ? _value.alightOffsetSecs
            : alightOffsetSecs // ignore: cast_nullable_to_non_nullable
                  as int,
        geometry: null == geometry
            ? _value._geometry
            : geometry // ignore: cast_nullable_to_non_nullable
                  as List<FfiLatLon>,
      ),
    );
  }
}

/// @nodoc

class _$FfiLeg_TransitImpl extends FfiLeg_Transit {
  const _$FfiLeg_TransitImpl({
    required this.tripId,
    required this.routeId,
    required this.routeName,
    required this.headsign,
    required this.fromStop,
    required this.toStop,
    required this.boardMs,
    required this.boardOffsetSecs,
    required this.alightMs,
    required this.alightOffsetSecs,
    required final List<FfiLatLon> geometry,
  }) : _geometry = geometry,
       super._();

  @override
  final String tripId;
  @override
  final String routeId;
  @override
  final String routeName;
  @override
  final String headsign;
  @override
  final FfiStop fromStop;
  @override
  final FfiStop toStop;
  @override
  final int boardMs;
  @override
  final int boardOffsetSecs;
  @override
  final int alightMs;
  @override
  final int alightOffsetSecs;
  final List<FfiLatLon> _geometry;
  @override
  List<FfiLatLon> get geometry {
    if (_geometry is EqualUnmodifiableListView) return _geometry;
    // ignore: implicit_dynamic_type
    return EqualUnmodifiableListView(_geometry);
  }

  @override
  String toString() {
    return 'FfiLeg.transit(tripId: $tripId, routeId: $routeId, routeName: $routeName, headsign: $headsign, fromStop: $fromStop, toStop: $toStop, boardMs: $boardMs, boardOffsetSecs: $boardOffsetSecs, alightMs: $alightMs, alightOffsetSecs: $alightOffsetSecs, geometry: $geometry)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$FfiLeg_TransitImpl &&
            (identical(other.tripId, tripId) || other.tripId == tripId) &&
            (identical(other.routeId, routeId) || other.routeId == routeId) &&
            (identical(other.routeName, routeName) ||
                other.routeName == routeName) &&
            (identical(other.headsign, headsign) ||
                other.headsign == headsign) &&
            (identical(other.fromStop, fromStop) ||
                other.fromStop == fromStop) &&
            (identical(other.toStop, toStop) || other.toStop == toStop) &&
            (identical(other.boardMs, boardMs) || other.boardMs == boardMs) &&
            (identical(other.boardOffsetSecs, boardOffsetSecs) ||
                other.boardOffsetSecs == boardOffsetSecs) &&
            (identical(other.alightMs, alightMs) ||
                other.alightMs == alightMs) &&
            (identical(other.alightOffsetSecs, alightOffsetSecs) ||
                other.alightOffsetSecs == alightOffsetSecs) &&
            const DeepCollectionEquality().equals(other._geometry, _geometry));
  }

  @override
  int get hashCode => Object.hash(
    runtimeType,
    tripId,
    routeId,
    routeName,
    headsign,
    fromStop,
    toStop,
    boardMs,
    boardOffsetSecs,
    alightMs,
    alightOffsetSecs,
    const DeepCollectionEquality().hash(_geometry),
  );

  /// Create a copy of FfiLeg
  /// with the given fields replaced by the non-null parameter values.
  @JsonKey(includeFromJson: false, includeToJson: false)
  @override
  @pragma('vm:prefer-inline')
  _$$FfiLeg_TransitImplCopyWith<_$FfiLeg_TransitImpl> get copyWith =>
      __$$FfiLeg_TransitImplCopyWithImpl<_$FfiLeg_TransitImpl>(
        this,
        _$identity,
      );

  @override
  @optionalTypeArgs
  TResult when<TResult extends Object?>({
    required TResult Function(
      int startMs,
      int startOffsetSecs,
      int endMs,
      int endOffsetSecs,
      int distanceM,
      FfiStop? fromStop,
      FfiStop? toStop,
      List<FfiLatLon> geometry,
    )
    walk,
    required TResult Function(
      String tripId,
      String routeId,
      String routeName,
      String headsign,
      FfiStop fromStop,
      FfiStop toStop,
      int boardMs,
      int boardOffsetSecs,
      int alightMs,
      int alightOffsetSecs,
      List<FfiLatLon> geometry,
    )
    transit,
  }) {
    return transit(
      tripId,
      routeId,
      routeName,
      headsign,
      fromStop,
      toStop,
      boardMs,
      boardOffsetSecs,
      alightMs,
      alightOffsetSecs,
      geometry,
    );
  }

  @override
  @optionalTypeArgs
  TResult? whenOrNull<TResult extends Object?>({
    TResult? Function(
      int startMs,
      int startOffsetSecs,
      int endMs,
      int endOffsetSecs,
      int distanceM,
      FfiStop? fromStop,
      FfiStop? toStop,
      List<FfiLatLon> geometry,
    )?
    walk,
    TResult? Function(
      String tripId,
      String routeId,
      String routeName,
      String headsign,
      FfiStop fromStop,
      FfiStop toStop,
      int boardMs,
      int boardOffsetSecs,
      int alightMs,
      int alightOffsetSecs,
      List<FfiLatLon> geometry,
    )?
    transit,
  }) {
    return transit?.call(
      tripId,
      routeId,
      routeName,
      headsign,
      fromStop,
      toStop,
      boardMs,
      boardOffsetSecs,
      alightMs,
      alightOffsetSecs,
      geometry,
    );
  }

  @override
  @optionalTypeArgs
  TResult maybeWhen<TResult extends Object?>({
    TResult Function(
      int startMs,
      int startOffsetSecs,
      int endMs,
      int endOffsetSecs,
      int distanceM,
      FfiStop? fromStop,
      FfiStop? toStop,
      List<FfiLatLon> geometry,
    )?
    walk,
    TResult Function(
      String tripId,
      String routeId,
      String routeName,
      String headsign,
      FfiStop fromStop,
      FfiStop toStop,
      int boardMs,
      int boardOffsetSecs,
      int alightMs,
      int alightOffsetSecs,
      List<FfiLatLon> geometry,
    )?
    transit,
    required TResult orElse(),
  }) {
    if (transit != null) {
      return transit(
        tripId,
        routeId,
        routeName,
        headsign,
        fromStop,
        toStop,
        boardMs,
        boardOffsetSecs,
        alightMs,
        alightOffsetSecs,
        geometry,
      );
    }
    return orElse();
  }

  @override
  @optionalTypeArgs
  TResult map<TResult extends Object?>({
    required TResult Function(FfiLeg_Walk value) walk,
    required TResult Function(FfiLeg_Transit value) transit,
  }) {
    return transit(this);
  }

  @override
  @optionalTypeArgs
  TResult? mapOrNull<TResult extends Object?>({
    TResult? Function(FfiLeg_Walk value)? walk,
    TResult? Function(FfiLeg_Transit value)? transit,
  }) {
    return transit?.call(this);
  }

  @override
  @optionalTypeArgs
  TResult maybeMap<TResult extends Object?>({
    TResult Function(FfiLeg_Walk value)? walk,
    TResult Function(FfiLeg_Transit value)? transit,
    required TResult orElse(),
  }) {
    if (transit != null) {
      return transit(this);
    }
    return orElse();
  }
}

abstract class FfiLeg_Transit extends FfiLeg {
  const factory FfiLeg_Transit({
    required final String tripId,
    required final String routeId,
    required final String routeName,
    required final String headsign,
    required final FfiStop fromStop,
    required final FfiStop toStop,
    required final int boardMs,
    required final int boardOffsetSecs,
    required final int alightMs,
    required final int alightOffsetSecs,
    required final List<FfiLatLon> geometry,
  }) = _$FfiLeg_TransitImpl;
  const FfiLeg_Transit._() : super._();

  String get tripId;
  String get routeId;
  String get routeName;
  String get headsign;
  @override
  FfiStop get fromStop;
  @override
  FfiStop get toStop;
  int get boardMs;
  int get boardOffsetSecs;
  int get alightMs;
  int get alightOffsetSecs;
  @override
  List<FfiLatLon> get geometry;

  /// Create a copy of FfiLeg
  /// with the given fields replaced by the non-null parameter values.
  @override
  @JsonKey(includeFromJson: false, includeToJson: false)
  _$$FfiLeg_TransitImplCopyWith<_$FfiLeg_TransitImpl> get copyWith =>
      throw _privateConstructorUsedError;
}
