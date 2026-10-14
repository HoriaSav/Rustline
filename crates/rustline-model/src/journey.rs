//! Journeys returned by the router (M3: stop to stop, transit and transfers).

use crate::{RouteIdx, Seconds, StopIdx, TripIdx};

/// One part of a journey.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Leg {
    /// A ride on one trip, boarding at `from` and getting off at `to`.
    Transit {
        /// Route of the trip.
        route: RouteIdx,
        /// The trip.
        trip: TripIdx,
        /// Boarding stop.
        from: StopIdx,
        /// Alighting stop.
        to: StopIdx,
        /// Departure from `from` (seconds since the service day's noon − 12 h).
        dep: Seconds,
        /// Arrival at `to`.
        arr: Seconds,
    },
    /// Changing from one stop to another (M3: only transfers.txt links).
    Walk {
        /// Stop left.
        from: StopIdx,
        /// Stop reached.
        to: StopIdx,
        /// Time it takes, in seconds.
        secs: u32,
    },
}

/// A way to get from the origin to the destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journey {
    /// Legs in travel order (empty when origin = destination).
    pub legs: Vec<Leg>,
    /// When the journey starts.
    pub dep: Seconds,
    /// When it reaches the destination.
    pub arr: Seconds,
    /// Number of transit legs.
    pub rides: u8,
}
