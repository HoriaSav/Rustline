//! A slow, simple second router to check RAPTOR against (feature `oracle`).
//!
//! Connection scan (CSA), the same logic as Tester's oracle in
//! `tests/m3_raptor.rs`: changing trips at a stop takes 0 s and transfers.txt
//! and pickup/drop-off types are ignored, so only compare on feeds without them.

use rustline_pack::TimetableData;

/// One hop of one trip: (departure, arrival, from stop, to stop, trip).
pub type Connection = (u32, u32, u32, u32, u32);

/// All connections of the timetable, sorted by departure.
pub fn connections(tt: &TimetableData) -> Vec<Connection> {
    let mut c = Vec::new();
    for t in 0..tt.trip_count() {
        let st = tt.trip_times(t);
        for w in st.windows(2) {
            c.push((w[0].2, w[1].1, w[0].0, w[1].0, t as u32));
        }
    }
    c.sort_unstable();
    c
}

/// Earliest arrival at `to` leaving `from` at `dep` or later on day number
/// `day` (0 = first day of the pack) with at most `max_rides` rides;
/// `u32::MAX` when unreachable. Stops must be in range.
pub fn earliest_arrival(
    tt: &TimetableData,
    conns: &[Connection],
    from: u32,
    to: u32,
    day: u32,
    dep: u32,
    max_rides: usize,
) -> u32 {
    if from == to {
        return dep;
    }
    let n = tt.stop_count();
    // best[k][s]: earliest arrival at s with ≤ k rides.
    let mut best = vec![vec![u32::MAX; n]; max_rides + 1];
    for row in &mut best {
        row[from as usize] = dep;
    }
    let mut boarded = vec![u8::MAX; tt.trip_count()]; // fewest rides used before boarding
    let mut target = u32::MAX;
    for &(d, a, u, v, t) in conns {
        if d < dep {
            continue;
        }
        if d >= target {
            break;
        }
        if !tt.service_runs(tt.trip_service[t as usize] as usize, day) {
            continue;
        }
        let ti = t as usize;
        if let Some(k) = (0..max_rides).find(|&k| best[k][u as usize] <= d) {
            boarded[ti] = boarded[ti].min(k as u8);
        }
        if boarded[ti] != u8::MAX {
            let k = boarded[ti] as usize + 1;
            for row in &mut best[k..] {
                if a < row[v as usize] {
                    row[v as usize] = a;
                }
            }
            if v == to {
                target = target.min(best[max_rides][to as usize]);
            }
        }
    }
    best[max_rides][to as usize]
}
