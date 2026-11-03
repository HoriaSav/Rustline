//! Where each trip's stops lie along its shape (shapes.txt, format v5).
//!
//! Done once when the pack is built, so the phone only cuts the line. With
//! `shape_dist_traveled` on both the shape and the stop times the feed says
//! where the stops are; otherwise each stop is projected onto the shape, in
//! order and only forwards, which also places a loop's first and last stop
//! (often the same one) at the two ends of the line. A trip whose stops end
//! up far from the line gets no shape, and the planner draws its stops.

use std::collections::HashMap;

use gtfs_structures::Gtfs;
use rustline_model::StopIdx;
use rustline_pack::{cumulative_m, haversine_e7};

use crate::timetable::{Stop, StopTime, TripShape};

/// Farthest a stop may be from its place on the shape, in metres.
pub(crate) const MAX_STOP_OFFSET_M: f64 = 100.0;

/// A shape, its points rounded to 1e-7 degrees as stored in the pack.
pub(crate) struct Line {
    points: Vec<(i32, i32)>,
    /// Metres from the first point to each point.
    cum: Vec<f64>,
    /// `shape_dist_traveled` of each point, when every point has one and it never decreases.
    dist: Option<Vec<f64>>,
}

impl Line {
    /// Shape points in `shape_pt_sequence` order; `None` with fewer than two
    /// distinct points or a point out of range.
    pub(crate) fn new(raw: &[(f64, f64, Option<f32>)]) -> Option<Line> {
        let e7 =
            |v: f64, max: f64| (v.is_finite() && v.abs() <= max).then(|| (v * 1e7).round() as i32);
        let mut points: Vec<(i32, i32)> = Vec::with_capacity(raw.len());
        let mut dist = Vec::with_capacity(raw.len());
        for &(lat, lon, d) in raw {
            let p = (e7(lat, 90.0)?, e7(lon, 180.0)?);
            // A repeated point adds nothing to the line.
            if points.last() != Some(&p) {
                points.push(p);
                dist.push(d.map(f64::from));
            }
        }
        if points.len() < 2 {
            return None;
        }
        let dist: Option<Vec<f64>> = dist.into_iter().collect();
        let dist = dist.filter(|d| d.windows(2).all(|w| w[0] <= w[1]));
        Some(Line {
            cum: cumulative_m(&points),
            points,
            dist,
        })
    }

    pub(crate) fn points(&self) -> &[(i32, i32)] {
        &self.points
    }

    /// The point `m` metres along the line (clamped to its ends).
    fn at(&self, m: f64) -> (i32, i32) {
        let j = self
            .cum
            .partition_point(|&c| c <= m)
            .clamp(1, self.points.len() - 1);
        let (a, b) = (self.points[j - 1], self.points[j]);
        let len = self.cum[j] - self.cum[j - 1];
        let f = if len > 0.0 {
            ((m - self.cum[j - 1]) / len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // In f64, like `project`: y - x overflows an i32 across ±180°.
        let mix = |x: i32, y: i32| {
            let (x, y) = (f64::from(x), f64::from(y));
            (x + f * (y - x)).round() as i32
        };
        (mix(a.0, b.0), mix(a.1, b.1))
    }

    /// Metres along the line of each stop, from `shape_dist_traveled`.
    fn by_dist(&self, stop_dist: &[f64]) -> Option<Vec<f64>> {
        let d = self.dist.as_ref()?;
        if stop_dist.windows(2).any(|w| w[0] > w[1]) {
            return None;
        }
        let (first, last) = (d[0], d[d.len() - 1]);
        stop_dist
            .iter()
            .map(|&s| {
                if s < first || s > last {
                    return None;
                }
                let j = d.partition_point(|&x| x <= s).clamp(1, d.len() - 1);
                let span = d[j] - d[j - 1];
                let f = if span > 0.0 {
                    (s - d[j - 1]) / span
                } else {
                    0.0
                };
                Some(self.cum[j - 1] + f * (self.cum[j] - self.cum[j - 1]))
            })
            .collect()
    }

    /// Metres along the line of each stop, projecting them in order: the
    /// placement with the smallest total distance from the stops among those
    /// that only move forwards.
    fn by_projection(&self, stops: &[(i32, i32)]) -> Option<Vec<f64>> {
        let segs = self.points.len() - 1;
        let proj: Vec<Vec<(f64, f64)>> = stops
            .iter()
            .map(|&p| {
                (0..segs)
                    .map(|j| {
                        // Too far from segment j: the stop can't be there.
                        let (off, m) = self.project(p, j);
                        (
                            if off > MAX_STOP_OFFSET_M {
                                f64::INFINITY
                            } else {
                                off
                            },
                            m,
                        )
                    })
                    .collect()
            })
            .collect();
        // cost[j]: best total with the current stop on segment j; back[i][j]:
        // the previous stop's segment for that.
        let mut cost: Vec<f64> = proj.first()?.iter().map(|x| x.0).collect();
        let mut back: Vec<Vec<u32>> = vec![Vec::new()];
        for (i, row) in proj.iter().enumerate().skip(1) {
            // Best over the segments before j; on j itself only if the
            // previous stop lies no further along it.
            let (mut best, mut at) = (f64::INFINITY, 0);
            let mut b = Vec::with_capacity(segs);
            let mut next = Vec::with_capacity(segs);
            for j in 0..segs {
                let (c, from) = if proj[i - 1][j].1 <= row[j].1 && cost[j] < best {
                    (cost[j], j as u32)
                } else {
                    (best, at)
                };
                b.push(from);
                next.push(c + row[j].0);
                if cost[j] < best {
                    (best, at) = (cost[j], j as u32);
                }
            }
            back.push(b);
            cost = next;
        }
        let mut j = (0..segs).min_by(|&a, &b| cost[a].total_cmp(&cost[b]))?;
        // Infinite: some stop is too far from every segment it could be on.
        if !cost[j].is_finite() {
            return None;
        }
        let mut pos = vec![0.0; stops.len()];
        for i in (0..stops.len()).rev() {
            pos[i] = proj[i][j].1;
            if i > 0 {
                j = back[i][j] as usize;
            }
        }
        Some(pos)
    }

    /// Distance of `p` from segment `j`, and metres along the line of the
    /// nearest point (a flat projection, fine over a segment's length).
    fn project(&self, p: (i32, i32), j: usize) -> (f64, f64) {
        let (a, b) = (self.points[j], self.points[j + 1]);
        let k = (f64::from(p.0) * 1e-7).to_radians().cos();
        // In f64: across the ±180° meridian the i32 difference overflows.
        let xy = |q: (i32, i32)| {
            (
                (f64::from(q.1) - f64::from(p.1)) * k,
                f64::from(q.0) - f64::from(p.0),
            )
        };
        let ((ax, ay), (bx, by)) = (xy(a), xy(b));
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 {
            (-(ax * dx + ay * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let m = self.cum[j] + t * (self.cum[j + 1] - self.cum[j]);
        (haversine_e7(p, self.at(m)), m)
    }

    /// Metres along the line of each stop: from `stop_dist` when given and
    /// consistent, else by projection; `None` if a stop is too far from the line.
    pub(crate) fn place(
        &self,
        stops: &[(i32, i32)],
        stop_dist: Option<&[f64]>,
    ) -> Option<Vec<f64>> {
        let near = |pos: &Vec<f64>| {
            stops
                .iter()
                .zip(pos)
                .all(|(&p, &m)| haversine_e7(p, self.at(m)) <= MAX_STOP_OFFSET_M)
        };
        stop_dist
            .and_then(|d| self.by_dist(d))
            .filter(near)
            .or_else(|| self.by_projection(stops))
    }
}

/// The shapes trips use, each read and placed once.
#[derive(Default)]
pub(crate) struct Shapes {
    lines: HashMap<String, Option<(Line, Option<u32>)>>,
    /// Shapes some trip uses, in pack order.
    pub(crate) used: Vec<Vec<(i32, i32)>>,
    /// Projections by shape_id and stop sequence (trips of one pattern share
    /// them). Keyed by shape_id: a pack index is only given on success.
    placed: HashMap<(String, Vec<StopIdx>), Option<Vec<f64>>>,
}

impl Shapes {
    /// The shape of `trip` and where its stops (`times`) lie on it, or `None`.
    pub(crate) fn trip(
        &mut self,
        g: &Gtfs,
        trip: &gtfs_structures::Trip,
        times: &[StopTime],
        stops: &[Stop],
    ) -> Option<TripShape> {
        let id = trip.shape_id.as_deref()?;
        let entry = self.lines.entry(id.to_string()).or_insert_with(|| {
            let raw: Vec<_> = g
                .shapes
                .get(id)?
                .iter()
                .map(|p| (p.latitude, p.longitude, p.dist_traveled))
                .collect();
            Some((Line::new(&raw)?, None))
        });
        let (line, index) = entry.as_mut()?;
        let pts: Vec<(i32, i32)> = times
            .iter()
            .map(|st| {
                let p = stops[st.stop.index()].pos?;
                Some(((p.lat * 1e7).round() as i32, (p.lon * 1e7).round() as i32))
            })
            .collect::<Option<_>>()?;
        // The feed's distances, when every stop time has one (same order as `times`).
        let mut raw: Vec<&gtfs_structures::StopTime> = trip.stop_times.iter().collect();
        raw.sort_by_key(|st| st.stop_sequence);
        let dist: Option<Vec<f64>> = raw
            .iter()
            .map(|st| st.shape_dist_traveled.map(f64::from))
            .collect();
        let at = match dist.filter(|d| d.len() == times.len()) {
            Some(d) => line.place(&pts, Some(&d)),
            None => self
                .placed
                .entry((id.to_string(), times.iter().map(|st| st.stop).collect()))
                .or_insert_with(|| line.place(&pts, None))
                .clone(),
        }?;
        let s = match *index {
            Some(s) => s,
            None => {
                let s = u32::try_from(self.used.len()).ok()?;
                *index = Some(s);
                self.used.push(line.points().to_vec());
                s
            }
        };
        Some(TripShape { shape: s, at_m: at })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight east-west line at the equator: 1e-5 degrees ≈ 1.11 m.
    fn line(n: i32, dist: bool) -> Line {
        let raw: Vec<_> = (0..n)
            .map(|i| (0.0, f64::from(i) * 1e-3, dist.then_some(i as f32 * 10.0)))
            .collect();
        Line::new(&raw).unwrap()
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.5
    }

    #[test]
    fn stops_are_placed_by_shape_dist_traveled() {
        let l = line(11, true); // 0..100 in feed units, about 1113 m
        let m = l
            .place(
                &[(0, 0), (0, 25_000), (0, 100_000)],
                Some(&[0.0, 25.0, 100.0]),
            )
            .unwrap();
        let total = l.cum[10];
        assert!(
            near(m[0], 0.0) && near(m[1], total / 4.0) && near(m[2], total),
            "{m:?}"
        );
    }

    #[test]
    fn stops_are_projected_without_distances() {
        let l = line(11, false);
        // 30 m north of the line, and on it.
        let m = l.place(&[(2_700, 10_000), (0, 55_000)], None).unwrap();
        assert!(
            near(m[0], l.cum[1]) && near(m[1], l.cum[5] + (l.cum[6] - l.cum[5]) / 2.0),
            "{m:?}"
        );
    }

    #[test]
    fn distances_that_do_not_fit_the_stops_fall_back_to_projection() {
        let l = line(11, true);
        // Distances in km instead of metres put every stop near the start.
        let m = l.place(&[(0, 0), (0, 50_000)], Some(&[0.0, 0.05])).unwrap();
        assert!(near(m[1], l.cum[5]), "{m:?}");
    }

    #[test]
    fn a_loop_places_its_first_and_last_stop_at_the_two_ends() {
        // Out east 1 km, back on a parallel street 50 m north, to the start.
        let raw: Vec<_> = [(0, 0), (0, 100_000), (4_500, 100_000), (4_500, 0), (0, 0)]
            .iter()
            .map(|&(y, x)| (f64::from(y) * 1e-7, f64::from(x) * 1e-7, None))
            .collect();
        let l = Line::new(&raw).unwrap();
        let start = (0, 0);
        let far = (0, 100_000);
        let m = l.place(&[start, far, start], None).unwrap();
        let total = *l.cum.last().unwrap();
        assert!(
            near(m[0], 0.0) && near(m[1], l.cum[1]) && near(m[2], total),
            "{m:?}"
        );
    }

    #[test]
    fn a_stop_passed_twice_on_an_out_and_back_line_is_placed_both_ways() {
        // East 1 km and back along the same street; the middle stop twice.
        let raw: Vec<_> = [0, 100_000, 0]
            .iter()
            .map(|&x| (0.0, f64::from(x) * 1e-7, None))
            .collect();
        let l = Line::new(&raw).unwrap();
        let (start, mid, far) = ((0, 0), (0, 50_000), (0, 100_000));
        let m = l.place(&[start, mid, far, mid, start], None).unwrap();
        let out = l.cum[1];
        let want = [0.0, out / 2.0, out, out * 1.5, out * 2.0];
        assert!(m.iter().zip(want).all(|(&a, b)| near(a, b)), "{m:?}");
    }

    #[test]
    fn valid_distances_win_over_the_nearest_projection() {
        // East about 1113 m and back along the same street, distances in
        // metres-ish feed units. The second stop is on the street both ways;
        // its distance says the way back, projection alone the way out.
        let raw: Vec<_> = [(0, 0.0), (100_000, 1000.0), (0, 2000.0)]
            .iter()
            .map(|&(x, d)| (0.0, f64::from(x) * 1e-7, Some(d as f32)))
            .collect();
        let l = Line::new(&raw).unwrap();
        let (start, mid) = ((0, 0), (0, 50_000));
        let m = l.place(&[start, mid], Some(&[0.0, 1500.0])).unwrap();
        let out = l.cum[1];
        assert!(near(m[0], 0.0) && near(m[1], out * 1.5), "{m:?}");
        // Without them, the stop goes on the way out.
        let p = l.place(&[start, mid], None).unwrap();
        assert!(near(p[1], out / 2.0), "{p:?}");
    }

    #[test]
    fn a_stop_far_from_the_shape_gives_no_placement() {
        let l = line(11, false);
        assert!(l.place(&[(0, 0), (20_000, 50_000)], None).is_none()); // 220 m off
    }

    #[test]
    fn a_shape_needs_two_distinct_points() {
        assert!(Line::new(&[(1.0, 1.0, None), (1.0, 1.0, None)]).is_none());
        assert!(Line::new(&[(91.0, 1.0, None), (1.0, 2.0, None)]).is_none());
    }
}
