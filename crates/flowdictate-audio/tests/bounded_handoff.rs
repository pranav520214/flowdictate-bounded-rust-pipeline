//! Public-seam tests for the callback-to-worker audio handoff.
#![allow(clippy::expect_used, clippy::float_cmp)]

use flowdictate_audio::{bounded_audio_ring, CaptureWrite};

#[test]
fn full_ring_drops_the_entire_incoming_batch_and_marks_discontinuity() {
    let (mut producer, mut consumer) = bounded_audio_ring(4).expect("capacity is non-zero");

    assert_eq!(
        producer.try_push_f32(&[0.0, 0.25, 0.5, 0.75]),
        CaptureWrite::Written {
            samples: 4,
            sanitized_samples: 0,
            discontinuity_epoch: 0,
        }
    );
    assert_eq!(
        producer.try_push_f32(&[1.0, -1.0]),
        CaptureWrite::Dropped {
            samples: 2,
            discontinuity_epoch: 1,
        }
    );

    let mut output = [9.0; 6];
    let report = consumer.read(&mut output);

    assert_eq!(report.samples_read, 4);
    assert_eq!(report.discontinuity_epoch, 1);
    assert_eq!(&output[..4], &[0.0, 0.25, 0.5, 0.75]);
    assert_eq!(&output[4..], &[9.0, 9.0]);
}

#[test]
fn callback_sample_types_are_normalized_without_non_finite_values() {
    let (mut producer, mut consumer) = bounded_audio_ring(8).expect("capacity is non-zero");

    assert_eq!(
        producer.try_push_f32(&[f32::NAN, f32::INFINITY, 2.0, -2.0]),
        CaptureWrite::Written {
            samples: 4,
            sanitized_samples: 4,
            discontinuity_epoch: 0,
        }
    );
    let mut f32_output = [9.0; 4];
    assert_eq!(consumer.read(&mut f32_output).samples_read, 4);
    assert_eq!(f32_output, [0.0, 0.0, 1.0, -1.0]);

    assert_eq!(
        producer.try_push_i16(&[i16::MIN, 0, i16::MAX]),
        CaptureWrite::Written {
            samples: 3,
            sanitized_samples: 0,
            discontinuity_epoch: 0,
        }
    );
    let mut i16_output = [9.0; 3];
    assert_eq!(consumer.read(&mut i16_output).samples_read, 3);
    assert_eq!(i16_output[0], -1.0);
    assert_eq!(i16_output[1], 0.0);
    assert!((i16_output[2] - 0.999_969_5).abs() < 0.000_001);

    assert_eq!(
        producer.try_push_u16(&[u16::MIN, 32_768, u16::MAX]),
        CaptureWrite::Written {
            samples: 3,
            sanitized_samples: 0,
            discontinuity_epoch: 0,
        }
    );
    let mut u16_output = [9.0; 3];
    assert_eq!(consumer.read(&mut u16_output).samples_read, 3);
    assert_eq!(u16_output[0], -1.0);
    assert_eq!(u16_output[1], 0.0);
    assert!((u16_output[2] - 0.999_969_5).abs() < 0.000_001);
}
