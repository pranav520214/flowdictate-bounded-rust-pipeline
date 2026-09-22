//! End-to-end synthetic test from callback handoff through worker processing.
#![allow(clippy::expect_used, clippy::float_cmp)]

use flowdictate_audio::{
    bounded_audio_ring, AudioFormat, AudioProcessor, CaptureWrite, SegmentEvent, VadConfig,
};

#[test]
fn synthetic_callback_audio_reaches_the_local_worker_without_persistence() {
    let format = AudioFormat::new(48_000, 2).expect("48 kHz stereo is supported");
    let (mut producer, mut consumer) = bounded_audio_ring(768 * 2).expect("bounded ring");
    let input = vec![0.25_f32; 768 * 2];
    assert!(matches!(
        producer.try_push_f32(&input),
        CaptureWrite::Written {
            samples,
            sanitized_samples: 0,
            discontinuity_epoch: 0,
        } if samples == input.len()
    ));

    let mut worker_input = vec![0.0_f32; input.len()];
    let read = consumer.read(&mut worker_input);
    assert_eq!(read.samples_read, input.len());
    assert_eq!(read.discontinuity_epoch, 0);

    let vad = VadConfig::new(0.6, 0.4, 2, 8, 1_000).expect("bounded VAD policy");
    let mut processor = AudioProcessor::new(format, 768, vad).expect("preallocated worker");
    let mut events = vec![SegmentEvent::Idle; processor.maximum_events_per_chunk()];
    let report = processor
        .process_interleaved(&worker_input, &mut events)
        .expect("synthetic audio is processed locally");

    assert_eq!(report.input_frames, 768);
    assert!(report.output_frames > 0);
    assert_eq!(report.peak_level, 0.25);
}
