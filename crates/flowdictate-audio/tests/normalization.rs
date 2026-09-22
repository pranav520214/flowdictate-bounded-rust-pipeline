//! Public-seam tests for worker-side channel conversion.
#![allow(clippy::expect_used, clippy::float_cmp)]

use flowdictate_audio::{downmix_interleaved_to_mono, AudioFormat, NormalizationError};

#[test]
fn stereo_downmix_is_bounded_sanitized_and_uses_caller_storage() {
    let format = AudioFormat::new(48_000, 2).expect("48 kHz stereo is supported");
    let input = [1.0, -1.0, 0.5, 0.5, f32::NAN, 2.0];
    let mut output = [9.0; 4];

    let report = downmix_interleaved_to_mono(&input, format, &mut output)
        .expect("aligned stereo input fits the output");

    assert_eq!(report.frames_written, 3);
    assert_eq!(report.sanitized_samples, 2);
    assert_eq!(&output[..3], &[0.0, 0.5, 0.5]);
    assert_eq!(output[3], 9.0);
}

#[test]
fn downmix_rejects_misaligned_input_and_small_output() {
    let stereo = AudioFormat::new(48_000, 2).expect("48 kHz stereo is supported");
    let mono = AudioFormat::new(16_000, 1).expect("16 kHz mono is supported");

    assert_eq!(
        downmix_interleaved_to_mono(&[0.0, 1.0, 2.0], stereo, &mut [0.0; 2]),
        Err(NormalizationError::MisalignedInput)
    );
    assert_eq!(
        downmix_interleaved_to_mono(&[0.0, 1.0], mono, &mut [0.0; 1]),
        Err(NormalizationError::OutputTooSmall)
    );
}
