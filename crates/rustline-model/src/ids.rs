//! Small, typed indices into the timetable arrays.
//!
//! Instead of passing GTFS string ids around, Rustline stores every stop,
//! route, trip, … once in a `Vec` and refers to it by position. Wrapping the
//! position in its own type (`StopIdx` vs `TripIdx`) lets the compiler catch
//! mix-ups that a plain `u32` would allow.

use serde::{Deserialize, Serialize};

macro_rules! index_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub u32);

        impl $name {
            /// Builds the index from a `Vec` position.
            ///
            /// # Panics
            /// If `i` does not fit in a `u32` (more than ~4 billion items).
            pub fn from_usize(i: usize) -> Self {
                Self(u32::try_from(i).expect(concat!(stringify!($name), " overflow")))
            }

            /// The position to use for indexing a `Vec`.
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

index_type!(
    /// Index of a stop in `Timetable::stops`.
    StopIdx
);
index_type!(
    /// Index of a route in `Timetable::routes`.
    RouteIdx
);
index_type!(
    /// Index of a trip in `Timetable::trips`.
    TripIdx
);
index_type!(
    /// Index of a stop pattern in `Timetable::patterns`.
    PatternIdx
);
index_type!(
    /// Index of a service (calendar) in `Timetable::services`.
    ServiceIdx
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_usize() {
        let s = StopIdx::from_usize(42);
        assert_eq!(s, StopIdx(42));
        assert_eq!(s.index(), 42);
    }

    #[test]
    fn orders_like_numbers() {
        assert!(TripIdx(1) < TripIdx(2));
    }
}
