//! Public-seam tests for the bounded benchmark fixture intake.
#![allow(clippy::expect_used)]

use std::{
    fmt::Write,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use flowdictate_audio::{
    load_benchmark_fixture, parse_benchmark_fixture_manifest, FixtureAccentEvidence,
    FixtureAcousticCondition, FixtureIntakeError, FixtureLanguageMix, FixtureLanguageMode,
    FixtureSpeechStyle, FixtureVoiceRights,
};
use sha2::{Digest, Sha256};

const HEADER: &str = "fixture_id,relative_audio_path,audio_sha256,expected_transcript_path,transcript_sha256,license_spdx,source_url,source_revision,redistributable,language_mode,language_tags,duration_ms,sample_rate_hz,channels,speech_style,acoustic_condition,language_mix,accent_evidence,voice_rights,classification,review_status";

struct TestRoot(PathBuf);

impl TestRoot {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "flowdictate-fixture-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("test root should be creatable");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a String should succeed");
            output
        })
}

fn manifest_row(audio: &[u8], transcript: &[u8]) -> String {
    format!(
        "case-1,case.wav,{},case.txt,{},CC0-1.0,https://example.invalid/source,revision-1,true,automatic,en,100,8000,1,synthetic,clean,monolingual,not-applicable,synthetic-no-person,synthetic-public,approved",
        hex_digest(audio),
        hex_digest(transcript)
    )
}

fn manifest(audio: &[u8], transcript: &[u8]) -> Vec<u8> {
    format!("{HEADER}\n{}\n", manifest_row(audio, transcript)).into_bytes()
}

fn wave(format_tag: u16, bits: u16, channels: u16, rate: u32, data: &[u8]) -> Vec<u8> {
    let bytes_per_sample = bits / 8;
    let block_align = channels * bytes_per_sample;
    let byte_rate = rate * u32::from(block_align);
    let padded = data.len() + (data.len() % 2);
    let riff_size = 4 + 8 + 16 + 8 + padded;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(
        &u32::try_from(riff_size)
            .expect("synthetic RIFF size should fit u32")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&format_tag.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&bits.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(
        &u32::try_from(data.len())
            .expect("synthetic data size should fit u32")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(data);
    if data.len() % 2 == 1 {
        bytes.push(0);
    }
    bytes
}

fn pcm16_fixture() -> Vec<u8> {
    let mut data = Vec::new();
    for index in 0..800 {
        let sample = match index % 4 {
            0 => i16::MIN,
            1 => -1,
            2 => 0,
            _ => i16::MAX,
        };
        data.extend_from_slice(&sample.to_le_bytes());
    }
    wave(1, 16, 1, 8_000, &data)
}

fn float_fixture(sample: f32) -> Vec<u8> {
    let mut data = Vec::new();
    for index in 0..800 {
        let value = if index % 2 == 0 { sample } else { -sample };
        data.extend_from_slice(&value.to_le_bytes());
    }
    wave(3, 32, 1, 8_000, &data)
}

fn install(root: &TestRoot, audio: &[u8], transcript: &[u8]) {
    fs::write(root.path().join("case.wav"), audio).expect("audio fixture should be writable");
    fs::write(root.path().join("case.txt"), transcript)
        .expect("transcript fixture should be writable");
}

#[test]
fn reviewed_pcm16_fixture_is_hash_checked_and_decoded() {
    let root = TestRoot::new("pcm");
    let audio = pcm16_fixture();
    let transcript = "synthetic reference\n".as_bytes();
    install(&root, &audio, transcript);
    let parsed = parse_benchmark_fixture_manifest(&manifest(&audio, transcript))
        .expect("strict manifest should parse");
    let entry = &parsed.entries()[0];
    assert_eq!(entry.fixture_id(), "case-1");
    assert_eq!(entry.license_spdx(), "CC0-1.0");
    assert!(matches!(
        entry.language_mode(),
        FixtureLanguageMode::Automatic
    ));
    assert_eq!(entry.speech_style(), FixtureSpeechStyle::Synthetic);
    assert_eq!(entry.acoustic_condition(), FixtureAcousticCondition::Clean);
    assert_eq!(entry.language_mix(), FixtureLanguageMix::Monolingual);
    assert_eq!(
        entry.accent_evidence(),
        FixtureAccentEvidence::NotApplicable
    );
    assert_eq!(entry.voice_rights(), FixtureVoiceRights::SyntheticNoPerson);

    let verified = load_benchmark_fixture(root.path(), entry).expect("fixture should load");
    assert_eq!(verified.format().sample_rate_hz(), 8_000);
    assert_eq!(verified.format().channels(), 1);
    assert_eq!(verified.duration_ms(), 100);
    assert_eq!(verified.samples().len(), 800);
    assert!((verified.samples()[0] + 1.0).abs() < f32::EPSILON);
    assert_eq!(verified.expected_transcript(), transcript);
}

#[test]
fn reviewed_silence_fixture_may_have_an_empty_reference() {
    let root = TestRoot::new("empty-reference");
    let audio = pcm16_fixture();
    let transcript = b"";
    install(&root, &audio, transcript);
    let parsed = parse_benchmark_fixture_manifest(&manifest(&audio, transcript))
        .expect("an empty reference hash should remain explicit");
    let verified = load_benchmark_fixture(root.path(), &parsed.entries()[0])
        .expect("reviewed silence may have an empty reference");
    assert!(verified.expected_transcript().is_empty());
}

#[test]
fn reviewed_float32_fixture_accepts_only_finite_normalized_samples() {
    let transcript = b"reference";
    for sample in [0.0_f32, 1.0] {
        let root = TestRoot::new("float-good");
        let audio = float_fixture(sample);
        install(&root, &audio, transcript);
        let parsed = parse_benchmark_fixture_manifest(&manifest(&audio, transcript))
            .expect("strict manifest should parse");
        assert!(load_benchmark_fixture(root.path(), &parsed.entries()[0]).is_ok());
    }

    for sample in [f32::NAN, f32::INFINITY, 1.01] {
        let root = TestRoot::new("float-bad");
        let audio = float_fixture(sample);
        install(&root, &audio, transcript);
        let parsed = parse_benchmark_fixture_manifest(&manifest(&audio, transcript))
            .expect("strict manifest should parse");
        assert!(matches!(
            load_benchmark_fixture(root.path(), &parsed.entries()[0]),
            Err(FixtureIntakeError::InvalidWave)
        ));
    }
}

#[test]
fn manifest_schema_and_review_decisions_fail_closed() {
    let audio = pcm16_fixture();
    let transcript = b"reference";
    let valid = String::from_utf8(manifest(&audio, transcript)).expect("manifest is UTF-8");
    for malformed in [
        valid.replacen("fixture_id", "wrong", 1),
        valid.replace("case-1", "case.1"),
        valid.replace("case.wav", "../case.wav"),
        valid.replace("case.wav", "folder/case.wav"),
        valid.replace("case.wav", "C:case.wav"),
        valid.replace(",true,", ",false,"),
        valid.replace(",approved\n", ",pending\n"),
        valid.replace(",automatic,en,", ",fixed:fr,en,"),
        valid.replace(",synthetic-public,", ",private,"),
        valid.replace(",synthetic,clean,", ",read,clean,"),
        valid.replace(
            ",not-applicable,synthetic-no-person,",
            ",not-reviewed,synthetic-no-person,",
        ),
        valid.replace(
            ",synthetic-no-person,synthetic-public,",
            ",corpus-license-reviewed,synthetic-public,",
        ),
        valid.replace("case-1", "\"case-1\""),
    ] {
        assert!(matches!(
            parse_benchmark_fixture_manifest(malformed.as_bytes()),
            Err(FixtureIntakeError::InvalidManifest)
        ));
    }
}

#[test]
fn diversity_strata_require_consistent_language_and_voice_rights() {
    let audio = pcm16_fixture();
    let transcript = b"reference";
    let valid = String::from_utf8(manifest(&audio, transcript)).expect("manifest is UTF-8");

    let reviewed_read = valid.replace("automatic,en", "fixed:hi,hi").replace(
        "synthetic,clean,monolingual,not-applicable,synthetic-no-person,synthetic-public",
        "read,uncharacterized,monolingual,not-reviewed,corpus-license-reviewed,reviewed-public",
    );
    let parsed = parse_benchmark_fixture_manifest(reviewed_read.as_bytes())
        .expect("conservative reviewed-public strata should parse");
    let entry = &parsed.entries()[0];
    assert_eq!(entry.speech_style(), FixtureSpeechStyle::Read);
    assert_eq!(
        entry.acoustic_condition(),
        FixtureAcousticCondition::Uncharacterized
    );
    assert_eq!(entry.accent_evidence(), FixtureAccentEvidence::NotReviewed);
    assert_eq!(
        entry.voice_rights(),
        FixtureVoiceRights::CorpusLicenseReviewed
    );

    for malformed in [
        reviewed_read.replace(",monolingual,", ",code-switched,"),
        reviewed_read.replace(",hi,100,", ",hi|en,100,"),
        reviewed_read.replace(",not-reviewed,", ",not-applicable,"),
        reviewed_read.replace(",corpus-license-reviewed,", ",synthetic-no-person,"),
    ] {
        assert!(matches!(
            parse_benchmark_fixture_manifest(malformed.as_bytes()),
            Err(FixtureIntakeError::InvalidManifest)
        ));
    }

    let code_switched = reviewed_read
        .replace(",hi,100,", ",hi|en,100,")
        .replace(",monolingual,", ",code-switched,");
    let parsed = parse_benchmark_fixture_manifest(code_switched.as_bytes())
        .expect("two reviewed language tags may declare code switching");
    assert_eq!(
        parsed.entries()[0].language_mix(),
        FixtureLanguageMix::CodeSwitched
    );
}

#[test]
fn manifest_rejects_duplicate_ids_paths_and_invalid_digests() {
    let audio = pcm16_fixture();
    let transcript = b"reference";
    let row = manifest_row(&audio, transcript);
    let duplicate = format!("{HEADER}\n{row}\n{row}\n");
    assert!(matches!(
        parse_benchmark_fixture_manifest(duplicate.as_bytes()),
        Err(FixtureIntakeError::InvalidManifest)
    ));
    let uppercase_hash = manifest(&audio, transcript)
        .into_iter()
        .map(|byte| if byte == b'a' { b'A' } else { byte })
        .collect::<Vec<_>>();
    assert!(matches!(
        parse_benchmark_fixture_manifest(&uppercase_hash),
        Err(FixtureIntakeError::InvalidManifest)
    ));
}

#[test]
fn exact_hash_and_transcript_text_are_enforced_without_payload_errors() {
    let root = TestRoot::new("hash");
    let audio = pcm16_fixture();
    let transcript = b"private-marker";
    install(&root, &audio, transcript);
    let parsed = parse_benchmark_fixture_manifest(&manifest(&audio, transcript))
        .expect("strict manifest should parse");
    fs::write(root.path().join("case.txt"), b"changed-marker")
        .expect("transcript mutation should succeed");
    let result = load_benchmark_fixture(root.path(), &parsed.entries()[0]);
    let error = result.err().expect("hash mismatch should fail");
    assert_eq!(error, FixtureIntakeError::HashMismatch);
    assert!(!error.to_string().contains("marker"));

    let invalid_transcript = [0xff_u8];
    install(&root, &audio, &invalid_transcript);
    let parsed = parse_benchmark_fixture_manifest(&manifest(&audio, &invalid_transcript))
        .expect("manifest should not inspect transcript payload");
    assert!(matches!(
        load_benchmark_fixture(root.path(), &parsed.entries()[0]),
        Err(FixtureIntakeError::InvalidTranscript)
    ));
}

#[test]
fn wav_container_requires_exact_canonical_chunks() {
    let transcript = b"reference";
    let good = pcm16_fixture();
    let mut cases = Vec::new();
    let mut bad_magic = good.clone();
    bad_magic[0] = b'X';
    cases.push(bad_magic);
    let mut bad_size = good.clone();
    bad_size[4..8].copy_from_slice(&1_u32.to_le_bytes());
    cases.push(bad_size);
    let mut unknown_chunk = good.clone();
    unknown_chunk[12..16].copy_from_slice(b"JUNK");
    cases.push(unknown_chunk);
    let mut bad_align = good.clone();
    bad_align[32..34].copy_from_slice(&4_u16.to_le_bytes());
    cases.push(bad_align);
    let mut truncated = good.clone();
    truncated.pop();
    cases.push(truncated);
    for audio in cases {
        let root = TestRoot::new("wav-bad");
        install(&root, &audio, transcript);
        let parsed = parse_benchmark_fixture_manifest(&manifest(&audio, transcript))
            .expect("hashes should match malformed test bytes");
        assert!(matches!(
            load_benchmark_fixture(root.path(), &parsed.entries()[0]),
            Err(FixtureIntakeError::InvalidWave)
        ));
    }
}

#[test]
fn unsupported_wav_encodings_and_manifest_shape_mismatches_are_rejected() {
    let transcript = b"reference";
    let unsupported = wave(1, 24, 1, 8_000, &[0; 2_400]);
    let root = TestRoot::new("unsupported");
    install(&root, &unsupported, transcript);
    let parsed = parse_benchmark_fixture_manifest(&manifest(&unsupported, transcript))
        .expect("strict manifest should parse");
    assert!(matches!(
        load_benchmark_fixture(root.path(), &parsed.entries()[0]),
        Err(FixtureIntakeError::InvalidWave)
    ));

    let audio = pcm16_fixture();
    install(&root, &audio, transcript);
    let wrong_shape = String::from_utf8(manifest(&audio, transcript))
        .expect("manifest is UTF-8")
        .replace(",100,8000,1,", ",101,8000,1,");
    let parsed = parse_benchmark_fixture_manifest(wrong_shape.as_bytes())
        .expect("shape remains within manifest bounds");
    assert!(matches!(
        load_benchmark_fixture(root.path(), &parsed.entries()[0]),
        Err(FixtureIntakeError::AudioShapeMismatch)
    ));
}

#[test]
fn non_regular_fixture_paths_are_rejected() {
    let root = TestRoot::new("directory");
    let audio = pcm16_fixture();
    let transcript = b"reference";
    fs::create_dir(root.path().join("case.wav")).expect("fixture directory should be creatable");
    fs::write(root.path().join("case.txt"), transcript).expect("transcript should be writable");
    let parsed = parse_benchmark_fixture_manifest(&manifest(&audio, transcript))
        .expect("strict manifest should parse");
    assert!(matches!(
        load_benchmark_fixture(root.path(), &parsed.entries()[0]),
        Err(FixtureIntakeError::UnsafeFileType)
    ));
}

#[cfg(unix)]
#[test]
fn symlinks_and_hardlinks_are_rejected() {
    use std::os::unix::fs::symlink;

    let root = TestRoot::new("links");
    let outside = TestRoot::new("links-outside");
    let audio = pcm16_fixture();
    let transcript = b"reference";
    fs::write(outside.path().join("outside.wav"), &audio).expect("outside file should be writable");
    symlink(
        outside.path().join("outside.wav"),
        root.path().join("case.wav"),
    )
    .expect("symlink should be creatable");
    fs::write(root.path().join("case.txt"), transcript).expect("transcript should be writable");
    let parsed = parse_benchmark_fixture_manifest(&manifest(&audio, transcript))
        .expect("strict manifest should parse");
    assert!(matches!(
        load_benchmark_fixture(root.path(), &parsed.entries()[0]),
        Err(FixtureIntakeError::ReparsePoint)
    ));
}
