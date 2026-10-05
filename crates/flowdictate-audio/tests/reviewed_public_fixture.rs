//! Repository evidence that the admitted public fixture still passes intake.
#![allow(clippy::expect_used)]

use std::{fs, path::PathBuf};

use flowdictate_audio::{
    load_benchmark_fixture, parse_benchmark_fixture_manifest, BenchmarkFixtureManifest,
    FixtureAccentEvidence, FixtureAcousticCondition, FixtureClassification, FixtureLanguageMix,
    FixtureLanguageMode, FixtureSpeechStyle, FixtureVoiceRights,
};

fn assert_manifest_coverage(manifest: &BenchmarkFixtureManifest) {
    assert_eq!(manifest.count_speech_style(FixtureSpeechStyle::Read), 11);
    assert_eq!(
        manifest.count_speech_style(FixtureSpeechStyle::Conversational),
        0
    );
    assert_eq!(
        manifest.count_speech_style(FixtureSpeechStyle::Spontaneous),
        0
    );
    assert_eq!(
        manifest.count_acoustic_condition(FixtureAcousticCondition::Clean),
        6
    );
    assert_eq!(
        manifest.count_acoustic_condition(FixtureAcousticCondition::Uncharacterized),
        5
    );
    assert_eq!(
        manifest.count_acoustic_condition(FixtureAcousticCondition::NaturalNoise),
        0
    );
    assert_eq!(
        manifest.count_language_mix(FixtureLanguageMix::CodeSwitched),
        0
    );
    assert_eq!(
        manifest.count_accent_evidence(FixtureAccentEvidence::NotReviewed),
        11
    );
    assert_eq!(
        manifest.count_accent_evidence(FixtureAccentEvidence::SourceReviewed),
        0
    );
    assert_eq!(
        manifest.count_accent_evidence(FixtureAccentEvidence::NativeReviewerConfirmed),
        0
    );
    assert_eq!(
        manifest.count_voice_rights(FixtureVoiceRights::CorpusLicenseReviewed),
        11
    );
}

#[test]
fn admitted_reviewed_fixtures_pass_the_strict_intake_boundary() {
    let benchmark_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../benches");
    let manifest_bytes =
        fs::read(benchmark_root.join("fixtures.csv")).expect("reviewed manifest should exist");
    let manifest = parse_benchmark_fixture_manifest(&manifest_bytes)
        .expect("reviewed manifest should pass strict parsing");
    assert_eq!(manifest.entries().len(), 11);
    assert_manifest_coverage(&manifest);

    let mut total_duration_ms = 0_u32;
    let mut total_samples = 0_usize;
    let mut english_cases = 0_usize;
    let mut hindi_cases = 0_usize;
    for entry in manifest.entries() {
        assert_eq!(entry.license_spdx(), "CC-BY-4.0");
        assert_eq!(
            entry.classification(),
            FixtureClassification::ReviewedPublic
        );
        assert_eq!(entry.language_tags().len(), 1);
        assert_eq!(entry.speech_style(), FixtureSpeechStyle::Read);
        assert_eq!(entry.language_mix(), FixtureLanguageMix::Monolingual);
        assert_eq!(entry.accent_evidence(), FixtureAccentEvidence::NotReviewed);
        assert_eq!(
            entry.voice_rights(),
            FixtureVoiceRights::CorpusLicenseReviewed
        );
        let language = entry.language_tags()[0].as_str();
        assert_eq!(
            entry.language_mode(),
            &FixtureLanguageMode::Fixed(language.into())
        );
        assert!(matches!(language, "en" | "hi"));
        if language == "en" {
            english_cases += 1;
            assert_eq!(entry.acoustic_condition(), FixtureAcousticCondition::Clean);
        } else {
            hindi_cases += 1;
            assert_eq!(
                entry.acoustic_condition(),
                FixtureAcousticCondition::Uncharacterized
            );
            assert!(entry.fixture_id().starts_with("fleurs-hi-in-dev-"));
            assert_eq!(
                entry.source_revision(),
                "4683b04af03d2d9549064c7d72060a9a94bb6046"
            );
        }
        let fixture = load_benchmark_fixture(&benchmark_root.join("fixtures"), entry)
            .expect("reviewed audio and transcript should verify and decode");
        assert_eq!(fixture.format().sample_rate_hz(), 16_000);
        assert_eq!(fixture.format().channels(), 1);
        assert!(!fixture.expected_transcript().is_empty());
        total_duration_ms = total_duration_ms
            .checked_add(fixture.duration_ms())
            .expect("reviewed durations should fit");
        total_samples = total_samples
            .checked_add(fixture.samples().len())
            .expect("reviewed samples should fit");
    }
    assert_eq!(english_cases, 6);
    assert_eq!(hindi_cases, 5);
    assert_eq!(total_duration_ms, 71_140);
    assert_eq!(total_samples, 1_138_240);
}
