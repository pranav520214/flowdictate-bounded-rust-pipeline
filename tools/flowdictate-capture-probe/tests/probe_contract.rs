//! Public CLI-policy seam tests for the human microphone probe.
#![allow(clippy::expect_used, clippy::float_cmp)]

use std::time::Duration;

use flowdictate_audio::{
    AudioFormat, CaptureHealthSnapshot, CapturePlan, CaptureSampleFormat, DEFAULT_RING_DURATION,
};
use flowdictate_capture_probe::{ConsentToken, ProbeDuration, ProbeReport, CONSENT_NOTICE};

#[test]
fn microphone_access_requires_the_exact_consent_phrase() {
    assert!(ConsentToken::parse(b"I CONSENT").is_ok());
    assert!(ConsentToken::parse(b"I CONSENT\r\n").is_ok());

    for rejected in [
        b"".as_slice(),
        b"yes".as_slice(),
        b"i consent".as_slice(),
        b"I CONSENT NOW".as_slice(),
    ] {
        assert!(ConsentToken::parse(rejected).is_err());
    }
    assert!(CONSENT_NOTICE.contains("microphone"));
    assert!(CONSENT_NOTICE.contains("not saved"));
}

#[test]
fn probe_duration_is_short_and_compiled_bounded() {
    assert!(ProbeDuration::new(0).is_err());
    assert_eq!(
        ProbeDuration::new(10)
            .expect("ten-second probe is permitted")
            .as_duration(),
        Duration::from_secs(10)
    );
    assert!(ProbeDuration::new(31).is_err());
}

#[test]
fn report_contains_only_fixed_labels_formats_and_numeric_counters() {
    let format = AudioFormat::new(48_000, 2).expect("supported format");
    let plan = CapturePlan::new(format, CaptureSampleFormat::F32, DEFAULT_RING_DURATION)
        .expect("bounded plan");
    let health = CaptureHealthSnapshot {
        callback_batches: 5,
        written_samples: 4_096,
        sanitized_samples: 0,
        dropped_batches: 0,
        dropped_samples: 0,
        stream_errors: 0,
    };
    let report = ProbeReport::new(
        ProbeDuration::new(10).expect("bounded duration"),
        plan,
        health,
        4_096,
        0,
    );

    assert_eq!(
        report.to_string(),
        "probe_seconds=10 sample_rate_hz=48000 channels=2 sample_format=f32 \
ring_capacity_samples=192000 callback_batches=5 written_samples=4096 \
sanitized_samples=0 dropped_batches=0 dropped_samples=0 stream_errors=0 \
drained_samples=4096 discontinuity_epoch=0"
    );
}
