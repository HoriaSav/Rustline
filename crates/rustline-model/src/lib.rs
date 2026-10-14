//! Shared vocabulary of Rustline: index types, GTFS times and coordinates.
//!
//! Every other crate depends on this one, so it stays small and has no
//! knowledge of file formats or parsers.

pub mod geo;
pub mod ids;
pub mod time;

pub use geo::LatLon;
pub use ids::{PatternIdx, RouteIdx, ServiceIdx, StopIdx, TripIdx};
pub use time::{parse_gtfs_time, Seconds, TimeError};
