//! Public-seam tests for codec/container separation.
#![allow(clippy::expect_used, clippy::float_cmp)]

use flowdictate_audio::{AudioCodec, CodecPolicy, CodecPolicyError, CodecPurpose, CodecRoute};

#[test]
fn milestone_one_live_path_accepts_only_volatile_pcm() {
    let policy = CodecPolicy::milestone_one();

    assert_eq!(
        policy.validate(CodecPurpose::LiveInference, AudioCodec::PcmF32),
        Ok(CodecRoute::VolatilePcm)
    );
    for codec in [AudioCodec::WavPcm, AudioCodec::Flac, AudioCodec::Opus] {
        assert_eq!(
            policy.validate(CodecPurpose::LiveInference, codec),
            Err(CodecPolicyError::UnsupportedForPurpose)
        );
    }
}

#[test]
fn broad_media_codecs_are_excluded_and_wav_is_fixture_only() {
    let policy = CodecPolicy::milestone_one();

    assert_eq!(
        policy.validate(CodecPurpose::RegressionFixture, AudioCodec::WavPcm),
        Ok(CodecRoute::ExplicitBoundedFile)
    );
    assert_eq!(
        policy.validate(CodecPurpose::DiagnosticExport, AudioCodec::WavPcm),
        Err(CodecPolicyError::FeatureDisabled)
    );
    for codec in [AudioCodec::Mp3, AudioCodec::Aac, AudioCodec::VideoContainer] {
        assert_eq!(
            policy.validate(CodecPurpose::RegressionFixture, codec),
            Err(CodecPolicyError::ExcludedCodec)
        );
    }
}
