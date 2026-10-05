//! Public-seam tests for local VAD scoring and bounded utterance segmentation.
#![allow(clippy::expect_used, clippy::float_cmp)]

use flowdictate_audio::{
    EarshotVad, FinalizeReason, SegmentEvent, SegmentState, UtteranceSegmenter, VadConfig, VadError,
};

#[test]
fn segmenter_starts_pauses_resumes_and_finalizes_on_bounded_silence() {
    let config = VadConfig::new(0.6, 0.4, 2, 3, 20).expect("valid bounded VAD policy");
    let mut segmenter = UtteranceSegmenter::new(config);

    assert_eq!(segmenter.observe(0.8), SegmentEvent::Idle);
    assert_eq!(segmenter.observe(0.8), SegmentEvent::SpeechStarted);
    assert_eq!(segmenter.state(), SegmentState::Speech);
    assert_eq!(segmenter.observe(0.1), SegmentEvent::ShortPause);
    assert_eq!(segmenter.state(), SegmentState::MaybePause);
    assert_eq!(segmenter.observe(0.8), SegmentEvent::SpeechResumed);
    assert_eq!(segmenter.observe(0.1), SegmentEvent::ShortPause);
    assert_eq!(segmenter.observe(0.1), SegmentEvent::ShortPause);
    assert_eq!(
        segmenter.observe(0.1),
        SegmentEvent::Finalized(FinalizeReason::Silence)
    );
    assert_eq!(segmenter.state(), SegmentState::Silence);
}

#[test]
fn segmenter_hard_limit_and_forced_boundary_cannot_grow_forever() {
    let config = VadConfig::new(0.5, 0.4, 1, 3, 3).expect("valid bounded VAD policy");
    let mut segmenter = UtteranceSegmenter::new(config);

    assert_eq!(segmenter.observe(0.9), SegmentEvent::SpeechStarted);
    assert_eq!(segmenter.observe(0.9), SegmentEvent::SpeechContinued);
    assert_eq!(
        segmenter.observe(0.9),
        SegmentEvent::Finalized(FinalizeReason::MaximumDuration)
    );

    assert_eq!(
        segmenter.force_finalize(FinalizeReason::HotkeyReleased),
        SegmentEvent::Idle
    );
    assert_eq!(segmenter.observe(0.9), SegmentEvent::SpeechStarted);
    assert_eq!(
        segmenter.force_finalize(FinalizeReason::Discontinuity),
        SegmentEvent::Finalized(FinalizeReason::Discontinuity)
    );
}

#[test]
fn discontinuity_resets_unconfirmed_speech_start() {
    let config = VadConfig::new(0.5, 0.4, 2, 3, 20).expect("valid bounded VAD policy");
    let mut segmenter = UtteranceSegmenter::new(config);

    assert_eq!(segmenter.observe(0.9), SegmentEvent::Idle);
    assert_eq!(
        segmenter.force_finalize(FinalizeReason::Discontinuity),
        SegmentEvent::Idle
    );
    assert_eq!(segmenter.observe(0.9), SegmentEvent::Idle);
}

#[test]
fn earshot_adapter_accepts_only_exact_canonical_frames() {
    let mut vad = EarshotVad::new();
    let silence = [0_i16; 256];

    let score = vad
        .score_i16(&silence)
        .expect("canonical frame is accepted");
    assert!(score.is_finite());
    assert!((0.0..=1.0).contains(&score));
    assert_eq!(
        vad.score_i16(&silence[..255]),
        Err(VadError::WrongFrameLength)
    );
}
