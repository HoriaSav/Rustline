//! Geographic types.

use serde::{Deserialize, Serialize};

/// A WGS84 coordinate in degrees.
///
/// `f64` on purpose: `f32` loses about a metre of precision at these
/// magnitudes. The pack format may compress later; the model does not.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LatLon {
    /// Latitude, -90..=90.
    pub lat: f64,
    /// Longitude, -180..=180.
    pub lon: f64,
}

impl LatLon {
    /// Creates a coordinate from latitude and longitude in degrees.
    pub fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }

    /// True when both values are finite and inside the valid WGS84 range.
    pub fn is_valid(&self) -> bool {
        self.lat.is_finite()
            && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_values() {
        let p = LatLon::new(12.5, -45.25);
        assert_eq!(p.lat, 12.5);
        assert_eq!(p.lon, -45.25);
        assert!(p.is_valid());
    }

    #[test]
    fn rejects_out_of_range() {
        assert!(!LatLon::new(91.0, 0.0).is_valid());
        assert!(!LatLon::new(0.0, -180.5).is_valid());
        assert!(!LatLon::new(f64::NAN, 0.0).is_valid());
    }
}
