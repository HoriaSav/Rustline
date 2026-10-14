//! The `oracle` feature's CSA agrees with RAPTOR (M3 decision 9).
//! `cargo test --locked -p rustline-core --features oracle --test oracle_feature`
#![cfg(feature = "oracle")]

mod m3_common;

use m3_common::*;
use rustline_core::oracle::{connections, earliest_arrival};
use rustline_core::raptor_pareto;
use rustline_model::{Seconds, StopIdx};

fn compare(tt: &Tt, queries: usize, seed: u64) {
    let conns = connections(tt);
    let n = tt.stop_count() as u64;
    let days = (tt.meta().valid_to - tt.meta().valid_from)
        .num_days()
        .min(60) as u64
        + 1;
    let mut rng = Rng(seed);
    for _ in 0..queries {
        let (a, b) = (rng.below(n) as u32, rng.below(n) as u32);
        // The CSA knows nothing about stations (RAPTOR expands them to platforms).
        if tt.stop_kind[a as usize] == 1 || tt.stop_kind[b as usize] == 1 {
            continue;
        }
        let day = rng.below(days) as u32;
        let dep = rng.below(26 * 3600) as u32;
        let date = tt.meta().valid_from + chrono::Days::new(u64::from(day));
        let js = raptor_pareto(tt, StopIdx(a), StopIdx(b), date, Seconds(dep), 8).unwrap();
        let got = js.iter().map(|j| j.arr.0).min().unwrap_or(u32::MAX);
        let want = earliest_arrival(tt, &conns, a, b, day, dep, 9);
        assert_eq!(got, want, "{a}->{b} day {day} dep {dep}");
    }
}

#[test]
fn oracle_agrees_on_tiny() {
    compare(&tiny(), 2000, 0x0AC1_E001);
}

#[test]
fn oracle_agrees_on_brasov() {
    let Some(feed) = shared_feed("brasov-manifest") else {
        return;
    };
    compare(&pack(&feed), 300, 0x0AC1_E002);
}
