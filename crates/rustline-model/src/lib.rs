//! Shared data types for Rustline.

/// A WGS84 coordinate in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latlon_holds_values() {
        let p = LatLon {
            lat: 45.6427,
            lon: 25.5887,
        };
        assert_eq!(p.lat, 45.6427);
        assert_eq!(p.lon, 25.5887);
    }
}
