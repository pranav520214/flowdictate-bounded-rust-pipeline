//! Public-seam tests for bounded worker-side resampling.
#![allow(clippy::expect_used, clippy::float_cmp)]

use flowdictate_audio::{MonoResampler, ResampleBoundaryError};

#[test]
fn mono_resampler_uses_fixed_input_and_caller_owned_output() {
    let mut resampler = MonoResampler::new(48_000, 480).expect("supported fixed input shape");
    assert_eq!(resampler.required_input_frames(), 480);
    let mut output = vec![9.0; resampler.maximum_output_frames()];
    let input = [0.25_f32; 480];

    let report = resampler
        .process(&input, &mut output)
        .expect("preallocated output is large enough");

    assert_eq!(report.consumed_frames, 480);
    assert!((150..=170).contains(&report.produced_frames));
    assert!(output[..report.produced_frames]
        .iter()
        .all(|sample| sample.is_finite() && (-1.0..=1.0).contains(sample)));
}

#[test]
fn mono_resampler_rejects_wrong_chunk_and_small_output() {
    let mut resampler = MonoResampler::new(44_100, 441).expect("supported fixed input shape");
    let mut adequate = vec![0.0; resampler.maximum_output_frames()];

    assert_eq!(
        resampler.process(&[0.0; 440], &mut adequate),
        Err(ResampleBoundaryError::WrongInputFrameCount)
    );

    let required = resampler.required_output_frames();
    let mut too_small = vec![0.0; required.saturating_sub(1)];
    assert_eq!(
        resampler.process(&[0.0; 441], &mut too_small),
        Err(ResampleBoundaryError::OutputTooSmall)
    );
}
