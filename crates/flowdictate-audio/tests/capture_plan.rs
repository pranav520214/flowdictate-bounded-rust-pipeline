//! Public-seam tests for CPAL capture planning without live hardware.
#![allow(clippy::expect_used, clippy::float_cmp)]

use std::time::Duration;

use flowdictate_audio::{
    AudioFormat, CapturePlan, CapturePlanError, CaptureSampleFormat, DEFAULT_RING_DURATION,
};

#[test]
fn capture_plan_preallocates_exactly_two_seconds_of_interleaved_samples() {
    let format = AudioFormat::new(48_000, 2).expect("48 kHz stereo is supported");
    let plan = CapturePlan::new(format, CaptureSampleFormat::F32, DEFAULT_RING_DURATION)
        .expect("default plan is bounded");

    assert_eq!(plan.audio_format(), format);
    assert_eq!(plan.sample_format(), CaptureSampleFormat::F32);
    assert_eq!(plan.ring_capacity_samples(), 192_000);
}

#[test]
fn capture_plan_rejects_zero_or_more_than_two_seconds() {
    let format = AudioFormat::new(16_000, 1).expect("16 kHz mono is supported");

    assert_eq!(
        CapturePlan::new(format, CaptureSampleFormat::I16, Duration::ZERO),
        Err(CapturePlanError::InvalidRingDuration)
    );
    assert_eq!(
        CapturePlan::new(
            format,
            CaptureSampleFormat::I16,
            Duration::from_millis(2_001)
        ),
        Err(CapturePlanError::InvalidRingDuration)
    );
}
