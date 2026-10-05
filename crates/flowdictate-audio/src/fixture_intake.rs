//! Strict, offline intake for reviewed benchmark fixtures.

use std::{
    collections::HashSet,
    fs::{self, File, Metadata, OpenOptions},
    io::Read,
    path::Path,
};

use sha2::{Digest, Sha256};

use crate::AudioFormat;

/// Maximum accepted fixture manifest size.
pub const MAX_FIXTURE_MANIFEST_BYTES: usize = 256 * 1024;
/// Maximum accepted records in one fixture manifest.
pub const MAX_FIXTURE_MANIFEST_ENTRIES: usize = 512;
/// Maximum encoded WAV size for one benchmark fixture.
pub const MAX_FIXTURE_AUDIO_BYTES: u64 = 32 * 1024 * 1024;
/// Maximum expected transcript size for one benchmark fixture.
pub const MAX_FIXTURE_TRANSCRIPT_BYTES: u64 = 64 * 1024;
/// Maximum fixture duration accepted by this narrow benchmark boundary.
pub const MAX_FIXTURE_DURATION_MS: u32 = 30_000;
/// Maximum fixture sample rate accepted by this narrow benchmark boundary.
pub const MAX_FIXTURE_SAMPLE_RATE_HZ: u32 = 96_000;

const MANIFEST_HEADER: &str = "fixture_id,relative_audio_path,audio_sha256,expected_transcript_path,transcript_sha256,license_spdx,source_url,source_revision,redistributable,language_mode,language_tags,duration_ms,sample_rate_hz,channels,speech_style,acoustic_condition,language_mix,accent_evidence,voice_rights,classification,review_status";
const FIELD_COUNT: usize = 21;

/// Reviewed language-selection mode for a benchmark case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FixtureLanguageMode {
    /// Permit the reviewed local model to detect language automatically.
    Automatic,
    /// Force one reviewed lowercase ISO 639-1 code.
    Fixed(String),
}

/// Human-reviewed fixture data classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureClassification {
    /// Locally generated, non-personal synthetic audio.
    SyntheticPublic,
    /// Redistributable public audio whose provenance was reviewed.
    ReviewedPublic,
}

/// Human-reviewed speaking-style stratum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureSpeechStyle {
    /// Generated audio with no natural person speaking.
    Synthetic,
    /// Prompted or prepared read speech.
    Read,
    /// Multi-party or interaction-shaped conversational speech.
    Conversational,
    /// Unscripted single-party speech.
    Spontaneous,
}

/// Human-reviewed acoustic-condition stratum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureAcousticCondition {
    /// The reviewed source identifies the recording as clean.
    Clean,
    /// Naturally occurring background noise is present.
    NaturalNoise,
    /// Noise was deliberately mixed into a derived fixture.
    SyntheticNoise,
    /// No acoustic-condition claim has been reviewed.
    Uncharacterized,
}

/// Human-reviewed language-composition stratum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureLanguageMix {
    /// Exactly one reviewed language is present.
    Monolingual,
    /// At least two reviewed languages occur in the utterance.
    CodeSwitched,
}

/// Strength of the evidence supporting an accent-related claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureAccentEvidence {
    /// Accent is inapplicable because no natural person is speaking.
    NotApplicable,
    /// No accent claim has been reviewed.
    NotReviewed,
    /// The pinned source supplies reviewed accent metadata.
    SourceReviewed,
    /// A native reviewer confirmed the recorded accent claim.
    NativeReviewerConfirmed,
}

/// Reviewed rights basis for retaining a voice fixture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureVoiceRights {
    /// The fixture contains no natural person's voice.
    SyntheticNoPerson,
    /// Corpus redistribution terms and provenance were reviewed.
    CorpusLicenseReviewed,
}

/// One strictly validated fixture manifest record.
pub struct BenchmarkFixtureEntry {
    fixture_id: String,
    audio_file_name: String,
    audio_sha256: [u8; 32],
    transcript_file_name: String,
    transcript_sha256: [u8; 32],
    license_spdx: String,
    source_url: String,
    source_revision: String,
    language_mode: FixtureLanguageMode,
    language_tags: Vec<String>,
    duration_ms: u32,
    sample_rate_hz: u32,
    channels: u16,
    speech_style: FixtureSpeechStyle,
    acoustic_condition: FixtureAcousticCondition,
    language_mix: FixtureLanguageMix,
    accent_evidence: FixtureAccentEvidence,
    voice_rights: FixtureVoiceRights,
    classification: FixtureClassification,
}

impl BenchmarkFixtureEntry {
    /// Returns the non-personal case identifier.
    #[must_use]
    pub fn fixture_id(&self) -> &str {
        &self.fixture_id
    }

    /// Returns the reviewed SPDX license identifier.
    #[must_use]
    pub fn license_spdx(&self) -> &str {
        &self.license_spdx
    }

    /// Returns the provenance URL as inert metadata. Intake never fetches it.
    #[must_use]
    pub fn source_url(&self) -> &str {
        &self.source_url
    }

    /// Returns the immutable reviewed source revision.
    #[must_use]
    pub fn source_revision(&self) -> &str {
        &self.source_revision
    }

    /// Returns the reviewed language-selection mode.
    #[must_use]
    pub fn language_mode(&self) -> &FixtureLanguageMode {
        &self.language_mode
    }

    /// Returns the reviewed lowercase language tags.
    #[must_use]
    pub fn language_tags(&self) -> &[String] {
        &self.language_tags
    }

    /// Returns the exact reviewed duration in milliseconds.
    #[must_use]
    pub const fn duration_ms(&self) -> u32 {
        self.duration_ms
    }

    /// Returns the exact reviewed sample rate.
    #[must_use]
    pub const fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    /// Returns the exact reviewed channel count.
    #[must_use]
    pub const fn channels(&self) -> u16 {
        self.channels
    }

    /// Returns the reviewed speaking-style stratum.
    #[must_use]
    pub const fn speech_style(&self) -> FixtureSpeechStyle {
        self.speech_style
    }

    /// Returns the reviewed acoustic-condition stratum.
    #[must_use]
    pub const fn acoustic_condition(&self) -> FixtureAcousticCondition {
        self.acoustic_condition
    }

    /// Returns whether the fixture is monolingual or code-switched.
    #[must_use]
    pub const fn language_mix(&self) -> FixtureLanguageMix {
        self.language_mix
    }

    /// Returns the reviewed strength of any accent claim.
    #[must_use]
    pub const fn accent_evidence(&self) -> FixtureAccentEvidence {
        self.accent_evidence
    }

    /// Returns the reviewed rights basis for retaining the voice fixture.
    #[must_use]
    pub const fn voice_rights(&self) -> FixtureVoiceRights {
        self.voice_rights
    }

    /// Returns the reviewed public/synthetic classification.
    #[must_use]
    pub const fn classification(&self) -> FixtureClassification {
        self.classification
    }
}

/// Strictly validated benchmark fixture manifest.
pub struct BenchmarkFixtureManifest {
    entries: Vec<BenchmarkFixtureEntry>,
}

impl BenchmarkFixtureManifest {
    /// Returns the validated records in manifest order.
    #[must_use]
    pub fn entries(&self) -> &[BenchmarkFixtureEntry] {
        &self.entries
    }

    /// Counts fixtures in one reviewed speaking-style stratum.
    #[must_use]
    pub fn count_speech_style(&self, style: FixtureSpeechStyle) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.speech_style == style)
            .count()
    }

    /// Counts fixtures in one reviewed acoustic-condition stratum.
    #[must_use]
    pub fn count_acoustic_condition(&self, condition: FixtureAcousticCondition) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.acoustic_condition == condition)
            .count()
    }

    /// Counts fixtures in one reviewed language-composition stratum.
    #[must_use]
    pub fn count_language_mix(&self, language_mix: FixtureLanguageMix) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.language_mix == language_mix)
            .count()
    }

    /// Counts fixtures at one reviewed accent-evidence level.
    #[must_use]
    pub fn count_accent_evidence(&self, evidence: FixtureAccentEvidence) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.accent_evidence == evidence)
            .count()
    }

    /// Counts fixtures admitted under one reviewed voice-rights basis.
    #[must_use]
    pub fn count_voice_rights(&self, rights: FixtureVoiceRights) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.voice_rights == rights)
            .count()
    }
}

/// Decoded fixture data whose files, hashes, and WAV shape passed intake.
pub struct VerifiedBenchmarkFixture {
    format: AudioFormat,
    duration_ms: u32,
    language_mode: FixtureLanguageMode,
    samples: Vec<f32>,
    expected_transcript: Vec<u8>,
}

impl VerifiedBenchmarkFixture {
    /// Returns the validated source audio format.
    #[must_use]
    pub const fn format(&self) -> AudioFormat {
        self.format
    }

    /// Returns the validated whole-frame duration in milliseconds.
    #[must_use]
    pub const fn duration_ms(&self) -> u32 {
        self.duration_ms
    }

    /// Returns the reviewed automatic/fixed language policy for this fixture.
    #[must_use]
    pub fn language_mode(&self) -> &FixtureLanguageMode {
        &self.language_mode
    }

    /// Returns decoded, normalized, interleaved samples.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Returns the reviewed UTF-8 reference transcript.
    #[must_use]
    pub fn expected_transcript(&self) -> &[u8] {
        &self.expected_transcript
    }
}

impl Drop for VerifiedBenchmarkFixture {
    fn drop(&mut self) {
        self.samples.fill(0.0);
        self.expected_transcript.fill(0);
    }
}

/// Payload-free failures from fixture manifest, path, hash, or WAV validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureIntakeError {
    /// The manifest encoding, schema, field, or record set is invalid.
    InvalidManifest,
    /// The approved fixture root cannot be safely inspected.
    ApprovedRootUnavailable,
    /// A fixture resolved outside its approved root.
    PathOutsideApprovedRoot,
    /// A fixture is not a regular file or has multiple hard links.
    UnsafeFileType,
    /// A root or fixture uses a symbolic link or filesystem reparse point.
    ReparsePoint,
    /// A fixture file could not be opened.
    OpenFailed,
    /// A fixture exceeds its encoded byte limit or changed size during intake.
    SizeLimit,
    /// Bounded memory could not be reserved.
    AllocationFailed,
    /// A fixture could not be read completely.
    ReadFailed,
    /// A fixture digest differs from its reviewed manifest digest.
    HashMismatch,
    /// The WAV container or sample encoding is outside the narrow allowlist.
    InvalidWave,
    /// WAV rate, channels, or duration differs from the reviewed manifest.
    AudioShapeMismatch,
    /// The expected transcript is not permitted UTF-8 text.
    InvalidTranscript,
}

impl std::fmt::Display for FixtureIntakeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidManifest => "benchmark fixture manifest is invalid",
            Self::ApprovedRootUnavailable => "approved benchmark root is unavailable",
            Self::PathOutsideApprovedRoot => "benchmark fixture is outside the approved root",
            Self::UnsafeFileType => "benchmark fixture file type is unsafe",
            Self::ReparsePoint => "benchmark fixture uses a filesystem reparse point",
            Self::OpenFailed => "benchmark fixture could not be opened",
            Self::SizeLimit => "benchmark fixture size is invalid",
            Self::AllocationFailed => "benchmark fixture memory allocation failed",
            Self::ReadFailed => "benchmark fixture could not be read",
            Self::HashMismatch => "benchmark fixture hash does not match its manifest",
            Self::InvalidWave => "benchmark WAV data is invalid or unsupported",
            Self::AudioShapeMismatch => "benchmark WAV shape does not match its manifest",
            Self::InvalidTranscript => "benchmark transcript is invalid",
        })
    }
}

impl std::error::Error for FixtureIntakeError {}

/// Parses a deliberately simple, unquoted review manifest without filesystem access.
///
/// # Errors
///
/// Returns [`FixtureIntakeError::InvalidManifest`] for every encoding, schema,
/// bounds, allowlist, or uniqueness violation. Raw fields are never echoed.
pub fn parse_benchmark_fixture_manifest(
    bytes: &[u8],
) -> Result<BenchmarkFixtureManifest, FixtureIntakeError> {
    if bytes.is_empty()
        || bytes.len() > MAX_FIXTURE_MANIFEST_BYTES
        || bytes.starts_with(&[0xef, 0xbb, 0xbf])
    {
        return Err(FixtureIntakeError::InvalidManifest);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| FixtureIntakeError::InvalidManifest)?;
    if text.contains('\0') || text.contains('\r') || text.chars().any(is_forbidden_control) {
        return Err(FixtureIntakeError::InvalidManifest);
    }
    let mut lines = text.split_terminator('\n');
    if lines.next() != Some(MANIFEST_HEADER) {
        return Err(FixtureIntakeError::InvalidManifest);
    }

    let mut entries = Vec::new();
    entries
        .try_reserve(MAX_FIXTURE_MANIFEST_ENTRIES.min(text.lines().count()))
        .map_err(|_| FixtureIntakeError::AllocationFailed)?;
    let mut fixture_ids = HashSet::new();
    let mut audio_names = HashSet::new();
    let mut transcript_names = HashSet::new();

    for line in lines {
        if line.is_empty() || entries.len() == MAX_FIXTURE_MANIFEST_ENTRIES || line.contains('"') {
            return Err(FixtureIntakeError::InvalidManifest);
        }
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() != FIELD_COUNT || fields.iter().any(|field| field.is_empty()) {
            return Err(FixtureIntakeError::InvalidManifest);
        }
        let entry = parse_entry(&fields)?;
        if !fixture_ids.insert(entry.fixture_id.clone())
            || !audio_names.insert(entry.audio_file_name.clone())
            || !transcript_names.insert(entry.transcript_file_name.clone())
        {
            return Err(FixtureIntakeError::InvalidManifest);
        }
        entries.push(entry);
    }
    if entries.is_empty() {
        return Err(FixtureIntakeError::InvalidManifest);
    }
    Ok(BenchmarkFixtureManifest { entries })
}

/// Opens, hashes, validates, and decodes one manifest-approved local fixture.
///
/// This function performs no network access and never follows a manifest URL.
///
/// # Errors
///
/// Returns a payload-free [`FixtureIntakeError`] when the approved-root gate,
/// byte bounds, exact digests, transcript checks, or narrow WAV checks fail.
pub fn load_benchmark_fixture(
    approved_root: &Path,
    entry: &BenchmarkFixtureEntry,
) -> Result<VerifiedBenchmarkFixture, FixtureIntakeError> {
    let audio = read_approved_file(
        approved_root,
        &entry.audio_file_name,
        44,
        MAX_FIXTURE_AUDIO_BYTES,
        &entry.audio_sha256,
    )?;
    let transcript = read_approved_file(
        approved_root,
        &entry.transcript_file_name,
        0,
        MAX_FIXTURE_TRANSCRIPT_BYTES,
        &entry.transcript_sha256,
    )?;
    validate_transcript(&transcript.0)?;
    let (format, duration_ms, samples) = decode_wave(&audio.0, entry)?;
    Ok(VerifiedBenchmarkFixture {
        format,
        duration_ms,
        language_mode: entry.language_mode.clone(),
        samples,
        expected_transcript: transcript.into_vec(),
    })
}

fn parse_entry(fields: &[&str]) -> Result<BenchmarkFixtureEntry, FixtureIntakeError> {
    if !valid_fixture_id(fields[0])
        || !valid_file_name(fields[1], ".wav")
        || !valid_file_name(fields[3], ".txt")
        || !valid_spdx(fields[5])
        || !valid_source_url(fields[6])
        || !valid_identifier(fields[7], 128)
        || fields[8] != "true"
        || fields[20] != "approved"
    {
        return Err(FixtureIntakeError::InvalidManifest);
    }
    let audio_sha256 = parse_digest(fields[2])?;
    let transcript_sha256 = parse_digest(fields[4])?;
    let language_mode = parse_language_mode(fields[9])?;
    let language_tags = parse_language_tags(fields[10])?;
    if let FixtureLanguageMode::Fixed(code) = &language_mode {
        if !language_tags.iter().any(|tag| tag == code) {
            return Err(FixtureIntakeError::InvalidManifest);
        }
    }
    let duration_ms = parse_bounded_u32(fields[11], 1, MAX_FIXTURE_DURATION_MS)?;
    let sample_rate_hz = parse_bounded_u32(fields[12], 8_000, MAX_FIXTURE_SAMPLE_RATE_HZ)?;
    let channels = fields[13]
        .parse::<u16>()
        .map_err(|_| FixtureIntakeError::InvalidManifest)?;
    if !(1..=2).contains(&channels) {
        return Err(FixtureIntakeError::InvalidManifest);
    }
    let speech_style = parse_speech_style(fields[14])?;
    let acoustic_condition = parse_acoustic_condition(fields[15])?;
    let language_mix = parse_language_mix(fields[16])?;
    let accent_evidence = parse_accent_evidence(fields[17])?;
    let voice_rights = parse_voice_rights(fields[18])?;
    match language_mix {
        FixtureLanguageMix::Monolingual if language_tags.len() != 1 => {
            return Err(FixtureIntakeError::InvalidManifest);
        }
        FixtureLanguageMix::CodeSwitched if language_tags.len() < 2 => {
            return Err(FixtureIntakeError::InvalidManifest);
        }
        _ => {}
    }
    let classification = match fields[19] {
        "synthetic-public" => FixtureClassification::SyntheticPublic,
        "reviewed-public" => FixtureClassification::ReviewedPublic,
        _ => return Err(FixtureIntakeError::InvalidManifest),
    };
    let consistent_privacy_classification = match classification {
        FixtureClassification::SyntheticPublic => {
            speech_style == FixtureSpeechStyle::Synthetic
                && accent_evidence == FixtureAccentEvidence::NotApplicable
                && voice_rights == FixtureVoiceRights::SyntheticNoPerson
        }
        FixtureClassification::ReviewedPublic => {
            speech_style != FixtureSpeechStyle::Synthetic
                && accent_evidence != FixtureAccentEvidence::NotApplicable
                && voice_rights == FixtureVoiceRights::CorpusLicenseReviewed
        }
    };
    if !consistent_privacy_classification {
        return Err(FixtureIntakeError::InvalidManifest);
    }
    Ok(BenchmarkFixtureEntry {
        fixture_id: fields[0].to_owned(),
        audio_file_name: fields[1].to_owned(),
        audio_sha256,
        transcript_file_name: fields[3].to_owned(),
        transcript_sha256,
        license_spdx: fields[5].to_owned(),
        source_url: fields[6].to_owned(),
        source_revision: fields[7].to_owned(),
        language_mode,
        language_tags,
        duration_ms,
        sample_rate_hz,
        channels,
        speech_style,
        acoustic_condition,
        language_mix,
        accent_evidence,
        voice_rights,
        classification,
    })
}

fn parse_speech_style(value: &str) -> Result<FixtureSpeechStyle, FixtureIntakeError> {
    match value {
        "synthetic" => Ok(FixtureSpeechStyle::Synthetic),
        "read" => Ok(FixtureSpeechStyle::Read),
        "conversational" => Ok(FixtureSpeechStyle::Conversational),
        "spontaneous" => Ok(FixtureSpeechStyle::Spontaneous),
        _ => Err(FixtureIntakeError::InvalidManifest),
    }
}

fn parse_acoustic_condition(value: &str) -> Result<FixtureAcousticCondition, FixtureIntakeError> {
    match value {
        "clean" => Ok(FixtureAcousticCondition::Clean),
        "natural-noise" => Ok(FixtureAcousticCondition::NaturalNoise),
        "synthetic-noise" => Ok(FixtureAcousticCondition::SyntheticNoise),
        "uncharacterized" => Ok(FixtureAcousticCondition::Uncharacterized),
        _ => Err(FixtureIntakeError::InvalidManifest),
    }
}

fn parse_language_mix(value: &str) -> Result<FixtureLanguageMix, FixtureIntakeError> {
    match value {
        "monolingual" => Ok(FixtureLanguageMix::Monolingual),
        "code-switched" => Ok(FixtureLanguageMix::CodeSwitched),
        _ => Err(FixtureIntakeError::InvalidManifest),
    }
}

fn parse_accent_evidence(value: &str) -> Result<FixtureAccentEvidence, FixtureIntakeError> {
    match value {
        "not-applicable" => Ok(FixtureAccentEvidence::NotApplicable),
        "not-reviewed" => Ok(FixtureAccentEvidence::NotReviewed),
        "source-reviewed" => Ok(FixtureAccentEvidence::SourceReviewed),
        "native-reviewer-confirmed" => Ok(FixtureAccentEvidence::NativeReviewerConfirmed),
        _ => Err(FixtureIntakeError::InvalidManifest),
    }
}

fn parse_voice_rights(value: &str) -> Result<FixtureVoiceRights, FixtureIntakeError> {
    match value {
        "synthetic-no-person" => Ok(FixtureVoiceRights::SyntheticNoPerson),
        "corpus-license-reviewed" => Ok(FixtureVoiceRights::CorpusLicenseReviewed),
        _ => Err(FixtureIntakeError::InvalidManifest),
    }
}

fn valid_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn valid_fixture_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_file_name(value: &str, suffix: &str) -> bool {
    value.len() <= 128
        && value != "."
        && value != ".."
        && value.ends_with(suffix)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn valid_spdx(value: &str) -> bool {
    value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-'))
}

fn valid_source_url(value: &str) -> bool {
    value.len() <= 512
        && value.starts_with("https://")
        && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

fn parse_digest(value: &str) -> Result<[u8; 32], FixtureIntakeError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(FixtureIntakeError::InvalidManifest);
    }
    let mut digest = [0_u8; 32];
    for (destination, pair) in digest.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        *destination = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
    }
    Ok(digest)
}

const fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => 0,
    }
}

fn parse_language_mode(value: &str) -> Result<FixtureLanguageMode, FixtureIntakeError> {
    if value == "automatic" {
        return Ok(FixtureLanguageMode::Automatic);
    }
    let code = value
        .strip_prefix("fixed:")
        .filter(|code| valid_language_code(code))
        .ok_or(FixtureIntakeError::InvalidManifest)?;
    Ok(FixtureLanguageMode::Fixed(code.to_owned()))
}

fn parse_language_tags(value: &str) -> Result<Vec<String>, FixtureIntakeError> {
    let mut tags = Vec::new();
    for tag in value.split('|') {
        if !valid_language_code(tag) || tags.len() == 16 || tags.iter().any(|seen| seen == tag) {
            return Err(FixtureIntakeError::InvalidManifest);
        }
        tags.push(tag.to_owned());
    }
    if tags.is_empty() {
        return Err(FixtureIntakeError::InvalidManifest);
    }
    Ok(tags)
}

fn valid_language_code(value: &str) -> bool {
    value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_lowercase())
}

fn parse_bounded_u32(value: &str, minimum: u32, maximum: u32) -> Result<u32, FixtureIntakeError> {
    if value.starts_with('+') || (value.len() > 1 && value.starts_with('0')) {
        return Err(FixtureIntakeError::InvalidManifest);
    }
    let parsed = value
        .parse::<u32>()
        .map_err(|_| FixtureIntakeError::InvalidManifest)?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(FixtureIntakeError::InvalidManifest);
    }
    Ok(parsed)
}

fn is_forbidden_control(character: char) -> bool {
    character.is_control() && character != '\n'
}

struct SensitiveBytes(Vec<u8>);

impl SensitiveBytes {
    fn into_vec(mut self) -> Vec<u8> {
        std::mem::take(&mut self.0)
    }
}

impl Drop for SensitiveBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

fn read_approved_file(
    approved_root: &Path,
    file_name: &str,
    minimum_bytes: u64,
    maximum_bytes: u64,
    expected_digest: &[u8; 32],
) -> Result<SensitiveBytes, FixtureIntakeError> {
    let root_metadata = fs::symlink_metadata(approved_root)
        .map_err(|_| FixtureIntakeError::ApprovedRootUnavailable)?;
    if is_reparse_point(&root_metadata) {
        return Err(FixtureIntakeError::ReparsePoint);
    }
    if !root_metadata.is_dir() {
        return Err(FixtureIntakeError::ApprovedRootUnavailable);
    }
    let canonical_root =
        fs::canonicalize(approved_root).map_err(|_| FixtureIntakeError::ApprovedRootUnavailable)?;
    let candidate = approved_root.join(file_name);
    let candidate_metadata =
        fs::symlink_metadata(&candidate).map_err(|_| FixtureIntakeError::OpenFailed)?;
    validate_file_metadata(&candidate_metadata)?;
    let canonical_candidate =
        fs::canonicalize(&candidate).map_err(|_| FixtureIntakeError::OpenFailed)?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(FixtureIntakeError::PathOutsideApprovedRoot);
    }
    let mut file = open_read_only(&canonical_candidate)?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| FixtureIntakeError::OpenFailed)?;
    validate_file_metadata(&opened_metadata)?;
    let length = opened_metadata.len();
    if length < minimum_bytes || length > maximum_bytes || candidate_metadata.len() != length {
        return Err(FixtureIntakeError::SizeLimit);
    }
    let size = usize::try_from(length).map_err(|_| FixtureIntakeError::SizeLimit)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| FixtureIntakeError::AllocationFailed)?;
    bytes.resize(size, 0);
    file.read_exact(&mut bytes)
        .map_err(|_| FixtureIntakeError::ReadFailed)?;
    let actual: [u8; 32] = Sha256::digest(&bytes).into();
    if !digest_matches(expected_digest, &actual) {
        bytes.fill(0);
        return Err(FixtureIntakeError::HashMismatch);
    }
    Ok(SensitiveBytes(bytes))
}

fn open_read_only(path: &Path) -> Result<File, FixtureIntakeError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        options.share_mode(FILE_SHARE_READ);
    }
    options
        .open(path)
        .map_err(|_| FixtureIntakeError::OpenFailed)
}

fn validate_file_metadata(metadata: &Metadata) -> Result<(), FixtureIntakeError> {
    if is_reparse_point(metadata) {
        return Err(FixtureIntakeError::ReparsePoint);
    }
    if !metadata.is_file() || has_multiple_links(metadata) {
        return Err(FixtureIntakeError::UnsafeFileType);
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn is_reparse_point(_: &Metadata) -> bool {
    false
}

#[cfg(unix)]
fn has_multiple_links(metadata: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink() != 1
}

// Stable Rust does not currently expose the Windows link count. Safety does
// not depend on it here: the read-only handle blocks concurrent writers and
// decoded bytes are owned before the handle is released.
#[cfg(not(unix))]
const fn has_multiple_links(_: &Metadata) -> bool {
    false
}

fn digest_matches(expected: &[u8; 32], actual: &[u8; 32]) -> bool {
    expected
        .iter()
        .zip(actual)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn validate_transcript(bytes: &[u8]) -> Result<(), FixtureIntakeError> {
    let text = std::str::from_utf8(bytes).map_err(|_| FixtureIntakeError::InvalidTranscript)?;
    if text.chars().any(|character| {
        character == '\0' || (character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    }) {
        return Err(FixtureIntakeError::InvalidTranscript);
    }
    Ok(())
}

fn decode_wave(
    bytes: &[u8],
    entry: &BenchmarkFixtureEntry,
) -> Result<(AudioFormat, u32, Vec<f32>), FixtureIntakeError> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(FixtureIntakeError::InvalidWave);
    }
    let riff_size =
        usize::try_from(read_u32(bytes, 4)?).map_err(|_| FixtureIntakeError::InvalidWave)?;
    if riff_size.checked_add(8) != Some(bytes.len()) {
        return Err(FixtureIntakeError::InvalidWave);
    }
    let mut offset = 12_usize;
    let mut wave_format = None;
    let mut data = None;
    while offset < bytes.len() {
        let header_end = offset
            .checked_add(8)
            .ok_or(FixtureIntakeError::InvalidWave)?;
        if header_end > bytes.len() {
            return Err(FixtureIntakeError::InvalidWave);
        }
        let chunk_id = &bytes[offset..offset + 4];
        let chunk_size = usize::try_from(read_u32(bytes, offset + 4)?)
            .map_err(|_| FixtureIntakeError::InvalidWave)?;
        let payload_end = header_end
            .checked_add(chunk_size)
            .ok_or(FixtureIntakeError::InvalidWave)?;
        let padded_end = payload_end
            .checked_add(chunk_size % 2)
            .ok_or(FixtureIntakeError::InvalidWave)?;
        if padded_end > bytes.len() {
            return Err(FixtureIntakeError::InvalidWave);
        }
        match chunk_id {
            b"fmt " if wave_format.is_none() && data.is_none() && chunk_size == 16 => {
                wave_format = Some(parse_wave_format(&bytes[header_end..payload_end])?);
            }
            b"data" if data.is_none() && wave_format.is_some() && chunk_size > 0 => {
                data = Some(&bytes[header_end..payload_end]);
            }
            _ => return Err(FixtureIntakeError::InvalidWave),
        }
        offset = padded_end;
    }
    if offset != bytes.len() {
        return Err(FixtureIntakeError::InvalidWave);
    }
    let format_fields = wave_format.ok_or(FixtureIntakeError::InvalidWave)?;
    let data = data.ok_or(FixtureIntakeError::InvalidWave)?;
    if data.len() % usize::from(format_fields.block_align) != 0 {
        return Err(FixtureIntakeError::InvalidWave);
    }
    let frames = data.len() / usize::from(format_fields.block_align);
    let maximum_frames = usize::try_from(format_fields.sample_rate_hz)
        .ok()
        .and_then(|rate| rate.checked_mul(30))
        .ok_or(FixtureIntakeError::InvalidWave)?;
    if frames == 0 || frames > maximum_frames {
        return Err(FixtureIntakeError::InvalidWave);
    }
    let duration_ms = u64::try_from(frames)
        .ok()
        .and_then(|count| count.checked_mul(1_000))
        .map(|value| value / u64::from(format_fields.sample_rate_hz))
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(FixtureIntakeError::InvalidWave)?;
    if duration_ms == 0
        || duration_ms != entry.duration_ms
        || format_fields.sample_rate_hz != entry.sample_rate_hz
        || format_fields.channels != entry.channels
    {
        return Err(FixtureIntakeError::AudioShapeMismatch);
    }
    let samples = decode_samples(data, format_fields, frames)?;
    let format = AudioFormat::new(format_fields.sample_rate_hz, format_fields.channels)
        .map_err(|_| FixtureIntakeError::InvalidWave)?;
    Ok((format, duration_ms, samples))
}

fn decode_samples(
    data: &[u8],
    format: WaveFormat,
    frames: usize,
) -> Result<Vec<f32>, FixtureIntakeError> {
    let sample_count = frames
        .checked_mul(usize::from(format.channels))
        .ok_or(FixtureIntakeError::InvalidWave)?;
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(sample_count)
        .map_err(|_| FixtureIntakeError::AllocationFailed)?;
    match format.encoding {
        WaveEncoding::Pcm16 => {
            for pair in data.chunks_exact(2) {
                samples.push(f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32_768.0);
            }
        }
        WaveEncoding::Float32 => {
            for word in data.chunks_exact(4) {
                let sample = f32::from_le_bytes([word[0], word[1], word[2], word[3]]);
                if !sample.is_finite() || !(-1.0..=1.0).contains(&sample) {
                    samples.fill(0.0);
                    return Err(FixtureIntakeError::InvalidWave);
                }
                samples.push(sample);
            }
        }
    }
    if samples.len() != sample_count {
        samples.fill(0.0);
        return Err(FixtureIntakeError::InvalidWave);
    }
    Ok(samples)
}

#[derive(Clone, Copy)]
enum WaveEncoding {
    Pcm16,
    Float32,
}

#[derive(Clone, Copy)]
struct WaveFormat {
    encoding: WaveEncoding,
    channels: u16,
    sample_rate_hz: u32,
    block_align: u16,
}

fn parse_wave_format(bytes: &[u8]) -> Result<WaveFormat, FixtureIntakeError> {
    if bytes.len() != 16 {
        return Err(FixtureIntakeError::InvalidWave);
    }
    let tag = read_u16(bytes, 0)?;
    let channels = read_u16(bytes, 2)?;
    let sample_rate_hz = read_u32(bytes, 4)?;
    let byte_rate = read_u32(bytes, 8)?;
    let block_align = read_u16(bytes, 12)?;
    let bits_per_sample = read_u16(bytes, 14)?;
    let bytes_per_sample = match (tag, bits_per_sample) {
        (1, 16) => 2_u16,
        (3, 32) => 4_u16,
        _ => return Err(FixtureIntakeError::InvalidWave),
    };
    if !(1..=2).contains(&channels)
        || !(8_000..=MAX_FIXTURE_SAMPLE_RATE_HZ).contains(&sample_rate_hz)
    {
        return Err(FixtureIntakeError::InvalidWave);
    }
    let expected_align = channels
        .checked_mul(bytes_per_sample)
        .ok_or(FixtureIntakeError::InvalidWave)?;
    let expected_rate = sample_rate_hz
        .checked_mul(u32::from(expected_align))
        .ok_or(FixtureIntakeError::InvalidWave)?;
    if block_align != expected_align || byte_rate != expected_rate {
        return Err(FixtureIntakeError::InvalidWave);
    }
    Ok(WaveFormat {
        encoding: if tag == 1 {
            WaveEncoding::Pcm16
        } else {
            WaveEncoding::Float32
        },
        channels,
        sample_rate_hz,
        block_align,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, FixtureIntakeError> {
    let pair = bytes
        .get(offset..offset + 2)
        .ok_or(FixtureIntakeError::InvalidWave)?;
    Ok(u16::from_le_bytes([pair[0], pair[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, FixtureIntakeError> {
    let word = bytes
        .get(offset..offset + 4)
        .ok_or(FixtureIntakeError::InvalidWave)?;
    Ok(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
}
