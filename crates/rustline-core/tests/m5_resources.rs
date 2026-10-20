//! M5 resource-limit tests (Tester, written before M5). Plan M5 step 5 + spec §12:
//! "a feed whose frequency expansion exceeds the cap → build error, not OOM"; edge-case
//! block "time arithmetic ≥ 48:00 and near u32::MAX". Current APIs only. The whole
//! binary runs under a counting allocator that refuses to go above 512 MiB, so an
//! uncapped expansion stops there instead of exhausting the machine (the refusal
//! aborts this test binary only).
//! Run: `cargo test --locked -p rustline-core --test m5_resources -- --include-ignored`

mod m5_common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use m5_common::*;

const LIMIT: usize = 512 << 20;

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let now = NOW.fetch_add(l.size(), Ordering::Relaxed) + l.size();
        if now > LIMIT {
            NOW.fetch_sub(l.size(), Ordering::Relaxed);
            return std::ptr::null_mut();
        }
        PEAK.fetch_max(now, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        NOW.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// The two peak-measuring tests take this so they don't see each other's heap
/// (added by Builder with the at-the-caps test).
static MEASURE: Mutex<()> = Mutex::new(());

#[test]
fn frequency_expansion_over_the_cap_is_a_build_error() {
    // 30 rows, headway 1 s, 00:00–48:00 → 5 184 000 trips. No feed needs that; the
    // cap (plan §11, value unspecified) must reject it quickly with little memory.
    let mut f = Feed::daily(BUCHAREST, "20310101", "20310131").trip(
        "S",
        "F",
        &[("00:00:00", "A"), ("00:10:00", "B")],
    );
    for _ in 0..30 {
        f = f.freq("F", "00:00:00", "48:00:00", "1", "1");
    }
    let tf = f.write();
    let _serial = MEASURE.lock().unwrap_or_else(|e| e.into_inner());
    PEAK.store(NOW.load(Ordering::Relaxed), Ordering::Relaxed);
    let t0 = Instant::now();
    let r = tf.build();
    let (secs, peak) = (
        t0.elapsed().as_secs_f64(),
        PEAK.load(Ordering::Relaxed) >> 20,
    );
    println!(
        "expansion: {:.2} s, peak heap {peak} MiB, error = {}",
        secs,
        r.is_err()
    );
    assert!(r.is_err(), "5 184 000 frequency trips accepted");
    assert!(secs < 10.0 && peak < 256, "{secs} s, {peak} MiB");
}

#[test]
fn times_near_u32_max_build_or_fail_cleanly_and_never_panic() {
    // 1193000:00:00 fits in u32, 1193047:00:00 does not (u32::MAX is 1193046:28:15).
    // Since M7 (Leader: hours below 48) both are an InvalidTime row in debug and
    // release: trip W is dropped, the rest of the feed builds and routes.
    for late in ["1193000:00:00", "1193047:00:00"] {
        let tf = Feed::daily(BUCHAREST, "20310101", "20310131")
            .trip("S", "W", &[("08:00:00", "A"), (late, "B")])
            .trip("S", "ok", &[("09:00:00", "A"), ("09:10:00", "C")])
            .write();
        let (tt, report) = tf.build().expect("feed builds");
        assert_eq!(
            report.count(rustline_gtfs::IssueKind::InvalidTime),
            1,
            "{late}"
        );
        let ids: Vec<&str> = tt.trips.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["ok"], "{late}");
        let p = tf.pack();
        assert_eq!(
            earliest(&p, "A", "C", date(2031, 1, 15), u32::MAX - 10),
            None
        );
        assert_eq!(
            earliest(&p, "A", "C", date(2031, 1, 15), hms(8, 0, 0)),
            Some(hms(9, 10, 0))
        );
    }
}

#[test]
fn frequency_shift_past_u32_max_drops_the_row_with_a_warning() {
    // Added by Builder (M5 review MF1). The template ends at 47:00:00 (stop times stop
    // below 48:00 since M7); a window starting at 1193040:00:00 would shift it past
    // u32::MAX. The row is dropped with InvalidFrequency, the other row still expands.
    let tf = Feed::daily(BUCHAREST, "20310101", "20310131")
        .trip("S", "F", &[("08:00:00", "A"), ("47:00:00", "B")])
        .freq("F", "1193040:00:00", "1193041:00:00", "600", "1")
        .freq("F", "08:00:00", "08:20:00", "600", "1")
        .write();
    let (tt, report) = tf.build().expect("feed builds");
    assert_eq!(report.count(rustline_gtfs::IssueKind::InvalidFrequency), 1);
    let ids: Vec<&str> = tt.trips.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["F@08:00:00", "F@08:10:00"]);
    for t in &tt.trips {
        let st = &tt.stop_times[t.stop_times.start as usize..t.stop_times.end as usize];
        assert!(st.windows(2).all(|w| w[0].departure <= w[1].arrival));
    }
}

/// `h:mm:ss` for a GTFS time in seconds.
fn gtfs_time(s: u32) -> String {
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

#[test]
fn frequency_expansion_at_the_caps_stays_under_512_mib() {
    // Added by Builder (M5 review). Exactly at the caps: 20 rows × 10 000 trips
    // (headway 1 s) × 25 stops = 200 000 trips and 5 000 000 stop times. The
    // counting allocator refuses anything above 512 MiB (the binary would abort).
    let calls: Vec<(String, String)> = (0..25u8)
        .map(|i| {
            (
                gtfs_time(u32::from(i) * 60),
                char::from(b'A' + i).to_string(),
            )
        })
        .collect();
    let calls: Vec<(&str, &str)> = calls
        .iter()
        .map(|(t, s)| (t.as_str(), s.as_str()))
        .collect();
    let mut f = Feed::daily(BUCHAREST, "20310101", "20310131").trip("S", "F", &calls);
    for k in 0..20 {
        f = f.freq(
            "F",
            &gtfs_time(k * 10_000),
            &gtfs_time((k + 1) * 10_000),
            "1",
            "1",
        );
    }
    let tf = f.write();
    let _serial = MEASURE.lock().unwrap_or_else(|e| e.into_inner());
    PEAK.store(NOW.load(Ordering::Relaxed), Ordering::Relaxed);
    let base = NOW.load(Ordering::Relaxed);
    let t0 = Instant::now();
    let (tt, _) = tf.build().expect("at the caps the feed builds");
    let trips = tt.trips.len();
    let build = (PEAK.load(Ordering::Relaxed) - base) >> 20;
    drop(tt);
    PEAK.store(NOW.load(Ordering::Relaxed), Ordering::Relaxed);
    let p = tf.pack();
    let pack = (PEAK.load(Ordering::Relaxed) - base) >> 20;
    println!(
        "at the caps: {trips} trips, peak heap build {build} MiB, build + pack {pack} MiB, {:.1} s",
        t0.elapsed().as_secs_f64()
    );
    assert_eq!(trips, 200_000);
    assert_eq!(p.stop_count(), 25);
    // M5 follow-up (M6 tests-first): explicit budget, well under the 512 MiB abort.
    assert!(build < 384, "build peak {build} MiB >= 384 MiB");
    assert!(pack < 384, "build + pack peak {pack} MiB >= 384 MiB");
}

#[test]
fn query_seconds_near_u32_max_do_not_panic() {
    let p = Feed::daily(BUCHAREST, "20310101", "20310131")
        // 47:10 → 47:50: stop times stop below 48:00 since M7 (was 47:50 → 48:30).
        .trip("S", "t", &[("47:10:00", "A"), ("47:50:00", "B")])
        .pack();
    for dep in [
        hms(47, 0, 0),
        hms(48, 0, 0),
        u32::MAX - 86_400,
        u32::MAX - 1,
        u32::MAX,
    ] {
        let r = rides(&p, "A", "B", date(2031, 1, 15), dep);
        assert!(r.is_ok(), "{dep}: {r:?}");
    }
    assert_eq!(
        earliest(&p, "A", "B", date(2031, 1, 15), hms(47, 0, 0)),
        Some(hms(47, 50, 0))
    );
}
