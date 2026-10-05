//! Public-seam tests for accepted and rejected hardware audio formats.
#![allow(clippy::expect_used, clippy::float_cmp)]

use flowdictate_audio::{AudioFormat, FormatError};

#[test]
fn audio_format_accepts_supported_capture_shape() {
    let format = AudioFormat::new(48_000, 2).expect("48 kHz stereo is supported");
    let high_rate = AudioFormat::new(192_000, 2).expect("192 kHz stereo is bounded and supported");

    assert_eq!(format.sample_rate_hz(), 48_000);
    assert_eq!(format.channels(), 2);
    assert_eq!(high_rate.sample_rate_hz(), 192_000);
    assert_eq!(
        format.samples_for_duration(std::time::Duration::from_secs(2)),
        192_000
    );
}

#[test]
fn audio_format_rejects_shapes_outside_hard_limits() {
    assert_eq!(
        AudioFormat::new(7_999, 1),
        Err(FormatError::UnsupportedSampleRate)
    );
    assert_eq!(
        AudioFormat::new(192_001, 1),
        Err(FormatError::UnsupportedSampleRate)
    );
    assert_eq!(
        AudioFormat::new(48_000, 0),
        Err(FormatError::UnsupportedChannelCount)
    );
    assert_eq!(
        AudioFormat::new(48_000, 3),
        Err(FormatError::UnsupportedChannelCount)
    );
}
