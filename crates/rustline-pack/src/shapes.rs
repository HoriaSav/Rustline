//! Transit shapes (format v5): the line each trip drives along, from
//! shapes.txt, and where its stops lie on it.
//!
//! Stored after the timetable body as its own rkyv archive, so the body keeps
//! the v4 layout and a v4 pack still loads (without shapes).

use rkyv::{Archive, Deserialize, Serialize};

use crate::data::haversine_e7;

/// Marks a trip without a shape in [`TransitShapes::trip_cut`].
pub const NO_SHAPE: u32 = u32::MAX;

/// Shapes and, per trip, where its stops lie along one.
///
/// All arrays empty = no shapes (a v4 pack, or a feed without shapes.txt).
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct TransitShapes {
    /// Shape s's points are `lat/lon[point_start[s]..[s + 1]]` (at least two).
    pub point_start: Vec<u32>,
    /// Latitude in 1e-7 degrees. The first point of a shape holds the value,
    /// the others the change from the point before (small numbers compress
    /// well).
    pub lat: Vec<i32>,
    /// Longitude in 1e-7 degrees, coded like `lat`.
    pub lon: Vec<i32>,
    /// Cut c of each trip, [`NO_SHAPE`] when it has none. Empty = no trip has one.
    pub trip_cut: Vec<u32>,
    /// The shape of each cut.
    pub cut_shape: Vec<u32>,
    /// Cut c's positions are `cut_dm[cut_start[c]..[c + 1]]`, one per stop
    /// of the trip's pattern.
    pub cut_start: Vec<u32>,
    /// Where each stop lies along the shape, in decimetres from its first
    /// point (measured with [`cumulative_m`]); never decreasing within a cut.
    pub cut_dm: Vec<u32>,
}

/// Distance along `points` (1e-7 degrees) from the first point to each point, in metres.
pub fn cumulative_m(points: &[(i32, i32)]) -> Vec<f64> {
    let mut m = 0.0;
    let mut out = Vec::with_capacity(points.len());
    for (i, &p) in points.iter().enumerate() {
        if i > 0 {
            m += haversine_e7(points[i - 1], p);
        }
        out.push(m);
    }
    out
}

impl TransitShapes {
    /// Number of shapes.
    pub fn shape_count(&self) -> usize {
        self.point_start.len().saturating_sub(1)
    }

    /// The points of shape `s` as `(lat, lon)` in 1e-7 degrees (`None` if out of range).
    pub fn points(&self, s: usize) -> Option<Vec<(i32, i32)>> {
        let r = *self.point_start.get(s)? as usize..*self.point_start.get(s + 1)? as usize;
        let (lat, lon) = (self.lat.get(r.clone())?, self.lon.get(r)?);
        let (mut y, mut x) = (0i32, 0i32);
        lat.iter()
            .zip(lon)
            .enumerate()
            .map(|(i, (&dy, &dx))| {
                (y, x) = if i == 0 {
                    (dy, dx)
                } else {
                    (y.checked_add(dy)?, x.checked_add(dx)?)
                };
                Some((y, x))
            })
            .collect()
    }

    /// The shape of `trip` and where each of its stops lies on it, in
    /// decimetres (`None`: the trip has no shape).
    pub fn trip_cut(&self, trip: usize) -> Option<(usize, &[u32])> {
        let c = *self.trip_cut.get(trip)? as usize;
        let shape = *self.cut_shape.get(c)? as usize;
        let r = *self.cut_start.get(c)? as usize..*self.cut_start.get(c + 1)? as usize;
        Some((shape, self.cut_dm.get(r)?))
    }

    /// Checks that the arrays fit together and with a timetable of `trips`
    /// trips, where trip t has `stops(t)` stops. Returns what is wrong as text.
    pub fn check(
        &self,
        trips: usize,
        stops: impl Fn(usize) -> Option<usize>,
    ) -> Result<(), String> {
        let csr = |name: &str, start: &[u32], items: usize| -> Result<(), String> {
            if start.is_empty() && items == 0 {
                return Ok(());
            }
            if start.first() != Some(&0)
                || start.windows(2).any(|w| w[0] > w[1])
                || start.last().map(|&e| e as usize) != Some(items)
            {
                return Err(format!("{name}: bad start offsets"));
            }
            Ok(())
        };
        if self.lat.len() != self.lon.len() {
            return Err("shape lon does not match shape lat".to_string());
        }
        csr("shape point_start", &self.point_start, self.lat.len())?;
        for s in 0..self.shape_count() {
            let p = self
                .points(s)
                .ok_or_else(|| format!("shape {s}: coordinates overflow"))?;
            if p.len() < 2 {
                return Err(format!("shape {s} has fewer than two points"));
            }
            if p.iter()
                .any(|&(y, x)| y.abs() > 900_000_000 || x.abs() > 1_800_000_000)
            {
                return Err(format!("shape {s}: point out of range"));
            }
        }
        let cuts = self.cut_shape.len();
        let want = if cuts == 0 { 0 } else { cuts + 1 };
        if self.cut_start.len() != want {
            return Err(format!(
                "shape cut_start has {} entries, expected {want}",
                self.cut_start.len()
            ));
        }
        csr("shape cut_start", &self.cut_start, self.cut_dm.len())?;
        for c in 0..cuts {
            if self.cut_shape[c] as usize >= self.shape_count() {
                return Err(format!("shape cut {c}: no shape {}", self.cut_shape[c]));
            }
            let r = self.cut_start[c] as usize..self.cut_start[c + 1] as usize;
            if self.cut_dm[r].windows(2).any(|w| w[0] > w[1]) {
                return Err(format!("shape cut {c}: positions go back"));
            }
        }
        if !self.trip_cut.is_empty() && self.trip_cut.len() != trips {
            return Err(format!(
                "trip_cut has {} entries, expected {trips}",
                self.trip_cut.len()
            ));
        }
        for (t, &c) in self.trip_cut.iter().enumerate() {
            if c == NO_SHAPE {
                continue;
            }
            let c = c as usize;
            if c >= cuts {
                return Err(format!("trip {t}: no shape cut {c}"));
            }
            let n = (self.cut_start[c + 1] - self.cut_start[c]) as usize;
            if stops(t) != Some(n) {
                return Err(format!(
                    "trip {t}: shape cut has {n} positions, not one per stop"
                ));
            }
        }
        Ok(())
    }
}
