//! Public-seam synthetic test for the worker-side audio pipeline.
#![allow(clippy::expect_used, clippy::float_cmp)]

use flowdictate_audio::{
    AudioFormat, AudioProcessingError, AudioProcessor, SegmentEvent, VadConfig,
};

#[test]
fn processor_converts_a_bounded_stereo_chunk_into_canonical_vad_frames() {
    let format = AudioFormat::new(48_000, 2).expect("48 kHz stereo is supported");
    let vad = VadConfig::new(0.6, 0.4, 2, 8, 1_000).expect("bounded VAD policy");
    let mut processor = AudioProcessor::new(format, 768, vad).expect("processor preallocates");
    let input = vec![0.25_f32; 768 * 2];
    let mut events = vec![SegmentEvent::Idle; processor.maximum_events_per_chunk()];

    let report = processor
        .process_interleaved(&input, &mut events)
        .expect("exact caller-owned buffers are accepted");

    assert_eq!(report.input_frames, 768);
    assert!((250..=265).contains(&report.output_frames));
    assert!(report.vad_frames_processed <= report.events_written);
    assert_eq!(report.peak_level, 0.25);
    assert!((report.rms_level - 0.25).abs() < f32::EPSILON);
}

#[test]
fn processor_rejects_wrong_input_and_insufficient_event_storage() {
    let format = AudioFormat::new(16_000, 1).expect("16 kHz mono is supported");
    let vad = VadConfig::new(0.6, 0.4, 2, 8, 1_000).expect("bounded VAD policy");
    let mut processor = AudioProcessor::new(format, 256, vad).expect("processor preallocates");

    assert_eq!(
        processor.process_interleaved(&[0.0; 255], &mut [SegmentEvent::Idle; 2]),
        Err(AudioProcessingError::WrongInputFrameCount)
    );
    assert_eq!(
        processor.process_interleaved(&[0.0; 256], &mut []),
        Err(AudioProcessingError::EventOutputTooSmall)
    );
    let mut events = vec![SegmentEvent::Idle; processor.maximum_events_per_chunk()];
    assert_eq!(
        processor.process_interleaved_with_frames(&[0.0; 256], &mut events, &mut []),
        Err(AudioProcessingError::CanonicalOutputTooSmall)
    );
}

#[test]
fn completed_vad_events_have_exact_canonical_frame_slices() {
    let format = AudioFormat::new(16_000, 1).expect("16 kHz mono is supported");
    let vad = VadConfig::new(0.6, 0.4, 2, 8, 1_000).expect("bounded VAD policy");
    let mut processor = AudioProcessor::new(format, 256, vad).expect("processor preallocates");
    let mut events = vec![SegmentEvent::Idle; processor.maximum_events_per_chunk()];
    let mut canonical = vec![-0.5; processor.maximum_canonical_samples_per_chunk()];

    let mut emitted = None;
    for _ in 0..8 {
        canonical.fill(-0.5);
        let report = processor
            .process_interleaved_with_frames(&[0.0; 256], &mut events, &mut canonical)
            .expect("canonical frame output is sufficiently sized");
        if report.events_written == 1 {
            emitted = Some(report);
            break;
        }
    }

    let report = emitted.expect("bounded streaming input emits one canonical frame");
    assert_eq!(report.vad_frames_processed, 1);
    assert!(canonical[..256].iter().all(|sample| *sample == 0.0));
    assert!(canonical[256..].iter().all(|sample| *sample == -0.5));
}
