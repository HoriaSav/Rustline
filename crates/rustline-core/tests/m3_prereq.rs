//! M3 preconditions that hold already (no M3 code needed), so they run by default.

mod m3_common;

use m3_common::*;

/// RAPTOR needs trips in a pattern to never overtake (M1 splits them).
#[test]
fn overtaking_is_impossible_within_a_pattern() {
    let mut feeds = vec![workspace("data/fixtures/tiny")];
    feeds.extend(shared_feed("brasov-manifest"));
    for f in feeds {
        let tt = pack(&f);
        for p in 0..tt.pattern_count() {
            let (a, b) = (
                tt.pattern_trips_start[p] as usize,
                tt.pattern_trips_start[p + 1] as usize,
            );
            for w in tt.pattern_trips[a..b].windows(2) {
                let (x, y) = (tt.trip_times(w[0] as usize), tt.trip_times(w[1] as usize));
                assert!(
                    x.iter().zip(&y).all(|(s, t)| s.1 <= t.1 && s.2 <= t.2),
                    "{}: trips {} and {} overtake in pattern {p}",
                    f.display(),
                    tt.trip_ids[w[0] as usize],
                    tt.trip_ids[w[1] as usize]
                );
            }
        }
    }
}

/// The CSA oracle and fixture expectations assume these feeds have no transfers.txt.
#[test]
fn oracle_feeds_have_no_transfers_txt() {
    for name in ["brasov-manifest", "cluj", "timisoara"] {
        let Some(p) = shared_feed(name) else { continue };
        let bytes = std::fs::read(&p).unwrap();
        let has = bytes.windows(13).any(|w| w == b"transfers.txt");
        assert!(
            !has,
            "{name} has transfers.txt; the CSA oracle must then model it"
        );
    }
}
