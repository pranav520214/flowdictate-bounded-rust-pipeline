//! Hours-equivalent virtual-time tests for bounded utterance segmentation.
#![allow(clippy::expect_used, clippy::float_cmp)]

use flowdictate_audio::{
    FinalizeReason, SegmentEvent, SegmentState, UtteranceSegmenter, VadConfig,
};

#[test]
fn eight_hours_of_continuous_speech_never_creates_an_unbounded_segment() {
    let config = VadConfig::new(0.6, 0.4, 2, 63, 7_500)
        .expect("120 seconds is 7,500 canonical 16 ms frames");
    let mut segmenter = UtteranceSegmenter::new(config);
    let virtual_frames = 1_800_000_u32;
    let mut maximum_frames_between_boundaries = 0_u32;
    let mut frames_since_boundary = 0_u32;
    let mut finalized_segments = 0_u32;

    for _ in 0..virtual_frames {
        frames_since_boundary += 1;
        if segmenter.observe(1.0) == SegmentEvent::Finalized(FinalizeReason::MaximumDuration) {
            maximum_frames_between_boundaries =
                maximum_frames_between_boundaries.max(frames_since_boundary);
            frames_since_boundary = 0;
            finalized_segments += 1;
        }
    }

    assert_eq!(finalized_segments, 240);
    assert_eq!(maximum_frames_between_boundaries, 7_500);
    assert_eq!(frames_since_boundary, 0);
    assert_eq!(segmenter.state(), SegmentState::Silence);
}

#[test]
fn repeated_discontinuities_never_accumulate_unconfirmed_speech() {
    let config = VadConfig::new(0.6, 0.4, 2, 63, 7_500).expect("bounded VAD policy");
    let mut segmenter = UtteranceSegmenter::new(config);

    for _ in 0..100_000 {
        assert_eq!(segmenter.observe(1.0), SegmentEvent::Idle);
        assert_eq!(
            segmenter.force_finalize(FinalizeReason::Discontinuity),
            SegmentEvent::Idle
        );
    }

    assert_eq!(segmenter.state(), SegmentState::Silence);
    assert_eq!(segmenter.observe(1.0), SegmentEvent::Idle);
    assert_eq!(segmenter.observe(1.0), SegmentEvent::SpeechStarted);
}
