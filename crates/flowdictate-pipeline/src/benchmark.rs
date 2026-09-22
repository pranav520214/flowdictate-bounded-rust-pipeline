//! Bounded, payload-minimal metrics for local streaming-ASR evaluation.

use std::{error::Error, fmt, time::Duration};

use flowdictate_asr_ipc::{MAX_INFERENCE_SAMPLES, MAX_TRANSCRIPT_BYTES};

const MAX_BENCHMARK_OBSERVATIONS: usize = 100_000;
const BASIS_POINTS: u128 = 10_000;
const MICROS_PER_SECOND: u128 = 1_000_000;
const ASR_SAMPLES_PER_SECOND: u128 = 16_000;
/// Highest token count accepted on either side of one WER comparison.
pub const MAX_RECOGNITION_TOKENS: usize = 4_096;
/// Highest non-whitespace Unicode-scalar count accepted on either side of one CER comparison.
pub const MAX_RECOGNITION_CHARACTERS: usize = 16_384;
/// Highest dynamic-programming cell count accepted by either edit comparison.
pub const MAX_RECOGNITION_EDIT_CELLS: usize = 4_000_000;

/// Explicit transcript semantics applied before recognition scoring.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RecognitionTextPolicy {
    /// Preserve exact casing, punctuation, and Unicode scalar values.
    #[default]
    Exact,
    /// `LibriSpeech` English evaluation: ASCII lowercase, retain letters,
    /// digits, and apostrophes, map other ASCII punctuation to word breaks,
    /// and collapse whitespace. U+2019 is mapped to an ASCII apostrophe.
    LibriSpeechEnglish,
    /// Google FLEURS Hindi evaluation: lowercase ASCII, preserve printable
    /// Unicode scalars, map a reviewed punctuation set to word breaks,
    /// collapse whitespace, and reject control characters.
    FleursHindi,
}

/// Validated bounds for one deterministic reference/hypothesis comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecognitionBenchmarkConfig {
    tokens: usize,
    characters: usize,
    edit_cells: usize,
    text_policy: RecognitionTextPolicy,
}

impl RecognitionBenchmarkConfig {
    /// Creates a bounded recognition metric policy.
    ///
    /// # Errors
    ///
    /// Rejects zero limits or limits above the compiled work ceilings.
    pub const fn new(
        maximum_tokens: usize,
        maximum_characters: usize,
        maximum_edit_cells: usize,
    ) -> Result<Self, RecognitionBenchmarkError> {
        if maximum_tokens == 0
            || maximum_tokens > MAX_RECOGNITION_TOKENS
            || maximum_characters == 0
            || maximum_characters > MAX_RECOGNITION_CHARACTERS
            || maximum_edit_cells == 0
            || maximum_edit_cells > MAX_RECOGNITION_EDIT_CELLS
        {
            return Err(RecognitionBenchmarkError::InvalidConfig);
        }
        Ok(Self {
            tokens: maximum_tokens,
            characters: maximum_characters,
            edit_cells: maximum_edit_cells,
            text_policy: RecognitionTextPolicy::Exact,
        })
    }

    /// Applies an explicitly reviewed transcript policy without changing work limits.
    #[must_use]
    pub const fn with_text_policy(mut self, text_policy: RecognitionTextPolicy) -> Self {
        self.text_policy = text_policy;
        self
    }

    /// Returns the token ceiling on each side of a WER comparison.
    #[must_use]
    pub const fn maximum_tokens(self) -> usize {
        self.tokens
    }

    /// Returns the non-whitespace Unicode-scalar ceiling on each side of a CER comparison.
    #[must_use]
    pub const fn maximum_characters(self) -> usize {
        self.characters
    }

    /// Returns the dynamic-programming work ceiling for each comparison.
    #[must_use]
    pub const fn maximum_edit_cells(self) -> usize {
        self.edit_cells
    }

    /// Returns the configured transcript comparison semantics.
    #[must_use]
    pub const fn text_policy(self) -> RecognitionTextPolicy {
        self.text_policy
    }
}

impl Default for RecognitionBenchmarkConfig {
    fn default() -> Self {
        Self {
            tokens: MAX_RECOGNITION_TOKENS,
            characters: MAX_RECOGNITION_CHARACTERS,
            edit_cells: MAX_RECOGNITION_EDIT_CELLS,
            text_policy: RecognitionTextPolicy::Exact,
        }
    }
}

/// Numeric-only exact-text recognition result for one reviewed fixture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecognitionBenchmarkSummary {
    /// Whitespace-delimited reference token count.
    pub reference_words: usize,
    /// Whitespace-delimited hypothesis token count.
    pub hypothesis_words: usize,
    /// Word substitutions in the deterministic minimum edit path.
    pub word_substitutions: u32,
    /// Reference words absent from the hypothesis.
    pub word_deletions: u32,
    /// Hypothesis words absent from the reference.
    pub word_insertions: u32,
    /// Total word edits.
    pub word_errors: u32,
    /// Word error rate in basis points, or `None` for an empty reference.
    pub word_error_rate_basis_points: Option<u64>,
    /// Non-whitespace Unicode scalar values in the reference.
    pub reference_characters: usize,
    /// Non-whitespace Unicode scalar values in the hypothesis.
    pub hypothesis_characters: usize,
    /// Reference scalars inside the Devanagari Unicode block.
    pub reference_devanagari_characters: usize,
    /// Hypothesis scalars inside the Devanagari Unicode block.
    pub hypothesis_devanagari_characters: usize,
    /// ASCII Latin letters in the reference.
    pub reference_ascii_latin_characters: usize,
    /// ASCII Latin letters in the hypothesis.
    pub hypothesis_ascii_latin_characters: usize,
    /// Character substitutions in the deterministic minimum edit path.
    pub character_substitutions: u32,
    /// Reference characters absent from the hypothesis.
    pub character_deletions: u32,
    /// Hypothesis characters absent from the reference.
    pub character_insertions: u32,
    /// Total character edits.
    pub character_errors: u32,
    /// Character error rate in basis points, or `None` for an empty reference.
    pub character_error_rate_basis_points: Option<u64>,
}

/// Compares borrowed reference and hypothesis text under strict work limits.
///
/// WER uses exact, case-sensitive, whitespace-delimited tokens. CER uses exact,
/// case-sensitive Unicode scalar values after excluding Unicode whitespace.
/// Dataset-specific punctuation, casing, or Unicode normalization must be a
/// separately reviewed preprocessing step so this function never changes text
/// semantics implicitly. Only numeric counts are returned.
///
/// # Errors
///
/// Returns a payload-free error when either transcript or comparison exceeds
/// its byte, token, character, allocation, arithmetic, or work bound.
pub fn measure_recognition(
    reference: &str,
    hypothesis: &str,
    config: RecognitionBenchmarkConfig,
) -> Result<RecognitionBenchmarkSummary, RecognitionBenchmarkError> {
    if reference.len() > MAX_TRANSCRIPT_BYTES || hypothesis.len() > MAX_TRANSCRIPT_BYTES {
        return Err(RecognitionBenchmarkError::TextTooLong);
    }
    validate_recognition_config(config)?;
    match config.text_policy {
        RecognitionTextPolicy::Exact => measure_exact_recognition(reference, hypothesis, config),
        RecognitionTextPolicy::LibriSpeechEnglish => {
            let normalized_reference = SensitiveNormalizedText::librispeech_english(reference)?;
            let normalized_hypothesis = SensitiveNormalizedText::librispeech_english(hypothesis)?;
            measure_exact_recognition(
                normalized_reference.as_str(),
                normalized_hypothesis.as_str(),
                config,
            )
        }
        RecognitionTextPolicy::FleursHindi => {
            let normalized_reference = SensitiveNormalizedText::fleurs_hindi(reference)?;
            let normalized_hypothesis = SensitiveNormalizedText::fleurs_hindi(hypothesis)?;
            measure_exact_recognition(
                normalized_reference.as_str(),
                normalized_hypothesis.as_str(),
                config,
            )
        }
    }
}

fn measure_exact_recognition(
    reference: &str,
    hypothesis: &str,
    config: RecognitionBenchmarkConfig,
) -> Result<RecognitionBenchmarkSummary, RecognitionBenchmarkError> {
    let reference_words = collect_words(reference, config.tokens)?;
    let hypothesis_words = collect_words(hypothesis, config.tokens)?;
    let word_edits = edit_counts(&reference_words, &hypothesis_words, config.edit_cells)?;

    let reference_characters = SensitiveCharacters::new(reference, config.characters)?;
    let hypothesis_characters = SensitiveCharacters::new(hypothesis, config.characters)?;
    let character_edits = edit_counts(
        &reference_characters.0,
        &hypothesis_characters.0,
        config.edit_cells,
    )?;

    Ok(RecognitionBenchmarkSummary {
        reference_words: reference_words.len(),
        hypothesis_words: hypothesis_words.len(),
        word_substitutions: word_edits.substitutions,
        word_deletions: word_edits.deletions,
        word_insertions: word_edits.insertions,
        word_errors: word_edits.errors,
        word_error_rate_basis_points: error_rate(word_edits.errors, reference_words.len())?,
        reference_characters: reference_characters.0.len(),
        hypothesis_characters: hypothesis_characters.0.len(),
        reference_devanagari_characters: count_devanagari(&reference_characters.0),
        hypothesis_devanagari_characters: count_devanagari(&hypothesis_characters.0),
        reference_ascii_latin_characters: count_ascii_latin(&reference_characters.0),
        hypothesis_ascii_latin_characters: count_ascii_latin(&hypothesis_characters.0),
        character_substitutions: character_edits.substitutions,
        character_deletions: character_edits.deletions,
        character_insertions: character_edits.insertions,
        character_errors: character_edits.errors,
        character_error_rate_basis_points: error_rate(
            character_edits.errors,
            reference_characters.0.len(),
        )?,
    })
}

fn count_devanagari(characters: &[char]) -> usize {
    characters
        .iter()
        .filter(|character| ('\u{0900}'..='\u{097f}').contains(character))
        .count()
}

fn count_ascii_latin(characters: &[char]) -> usize {
    characters
        .iter()
        .filter(|character| character.is_ascii_alphabetic())
        .count()
}

struct SensitiveNormalizedText(Vec<u8>);

impl SensitiveNormalizedText {
    fn librispeech_english(text: &str) -> Result<Self, RecognitionBenchmarkError> {
        let mut normalized = Vec::new();
        normalized
            .try_reserve_exact(text.len())
            .map_err(|_| RecognitionBenchmarkError::AllocationFailed)?;
        let mut separator_pending = false;
        for character in text.chars() {
            if character.is_ascii_alphanumeric() {
                if separator_pending && !normalized.is_empty() {
                    normalized.push(b' ');
                }
                normalized.push(character.to_ascii_lowercase() as u8);
                separator_pending = false;
            } else if matches!(character, '\'' | '\u{2019}') {
                if separator_pending && !normalized.is_empty() {
                    normalized.push(b' ');
                }
                normalized.push(b'\'');
                separator_pending = false;
            } else if character.is_ascii_punctuation() || character.is_whitespace() {
                separator_pending = !normalized.is_empty();
            } else {
                normalized.fill(0);
                return Err(RecognitionBenchmarkError::UnsupportedNormalizationText);
            }
        }
        Ok(Self(normalized))
    }

    fn fleurs_hindi(text: &str) -> Result<Self, RecognitionBenchmarkError> {
        let mut normalized = Vec::new();
        normalized
            .try_reserve_exact(text.len())
            .map_err(|_| RecognitionBenchmarkError::AllocationFailed)?;
        let mut separator_pending = false;
        for character in text.chars() {
            if character.is_whitespace() || is_fleurs_hindi_separator(character) {
                separator_pending = !normalized.is_empty();
                continue;
            }
            if character.is_ascii_alphanumeric() {
                push_normalized_separator(&mut normalized, &mut separator_pending);
                normalized.push(character.to_ascii_lowercase() as u8);
                continue;
            }
            if !character.is_control() {
                push_normalized_separator(&mut normalized, &mut separator_pending);
                let mut encoded = [0_u8; 4];
                normalized.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
                continue;
            }
            normalized.fill(0);
            return Err(RecognitionBenchmarkError::UnsupportedNormalizationText);
        }
        Ok(Self(normalized))
    }

    fn as_str(&self) -> &str {
        // Construction admits ASCII bytes only.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
}

fn push_normalized_separator(normalized: &mut Vec<u8>, separator_pending: &mut bool) {
    if *separator_pending && !normalized.is_empty() {
        normalized.push(b' ');
    }
    *separator_pending = false;
}

fn is_fleurs_hindi_separator(character: char) -> bool {
    character.is_ascii_punctuation()
        || matches!(
            character,
            '\u{0964}'
                | '\u{0965}'
                | '\u{2010}'..='\u{2015}'
                | '\u{2018}'..='\u{201f}'
                | '\u{2026}'
        )
}

impl Drop for SensitiveNormalizedText {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

fn validate_recognition_config(
    config: RecognitionBenchmarkConfig,
) -> Result<(), RecognitionBenchmarkError> {
    RecognitionBenchmarkConfig::new(config.tokens, config.characters, config.edit_cells).map(|_| ())
}

fn collect_words(text: &str, maximum: usize) -> Result<Vec<&str>, RecognitionBenchmarkError> {
    let count = text.split_whitespace().count();
    if count > maximum {
        return Err(RecognitionBenchmarkError::TokenLimitReached);
    }
    let mut words = Vec::new();
    words
        .try_reserve_exact(count)
        .map_err(|_| RecognitionBenchmarkError::AllocationFailed)?;
    words.extend(text.split_whitespace());
    Ok(words)
}

struct SensitiveCharacters(Vec<char>);

impl SensitiveCharacters {
    fn new(text: &str, maximum: usize) -> Result<Self, RecognitionBenchmarkError> {
        let count = text
            .chars()
            .filter(|character| !character.is_whitespace())
            .count();
        if count > maximum {
            return Err(RecognitionBenchmarkError::CharacterLimitReached);
        }
        let mut characters = Vec::new();
        characters
            .try_reserve_exact(count)
            .map_err(|_| RecognitionBenchmarkError::AllocationFailed)?;
        characters.extend(text.chars().filter(|character| !character.is_whitespace()));
        Ok(Self(characters))
    }
}

impl Drop for SensitiveCharacters {
    fn drop(&mut self) {
        self.0.fill('\0');
    }
}

#[derive(Clone, Copy, Default)]
struct EditCounts {
    errors: u32,
    substitutions: u32,
    deletions: u32,
    insertions: u32,
}

impl EditCounts {
    fn substitution(self) -> Result<Self, RecognitionBenchmarkError> {
        Ok(Self {
            errors: self
                .errors
                .checked_add(1)
                .ok_or(RecognitionBenchmarkError::ArithmeticOverflow)?,
            substitutions: self
                .substitutions
                .checked_add(1)
                .ok_or(RecognitionBenchmarkError::ArithmeticOverflow)?,
            ..self
        })
    }

    fn deletion(self) -> Result<Self, RecognitionBenchmarkError> {
        Ok(Self {
            errors: self
                .errors
                .checked_add(1)
                .ok_or(RecognitionBenchmarkError::ArithmeticOverflow)?,
            deletions: self
                .deletions
                .checked_add(1)
                .ok_or(RecognitionBenchmarkError::ArithmeticOverflow)?,
            ..self
        })
    }

    fn insertion(self) -> Result<Self, RecognitionBenchmarkError> {
        Ok(Self {
            errors: self
                .errors
                .checked_add(1)
                .ok_or(RecognitionBenchmarkError::ArithmeticOverflow)?,
            insertions: self
                .insertions
                .checked_add(1)
                .ok_or(RecognitionBenchmarkError::ArithmeticOverflow)?,
            ..self
        })
    }
}

fn edit_counts<T: Eq>(
    reference: &[T],
    hypothesis: &[T],
    maximum_cells: usize,
) -> Result<EditCounts, RecognitionBenchmarkError> {
    let cells = reference
        .len()
        .checked_add(1)
        .and_then(|rows| {
            hypothesis
                .len()
                .checked_add(1)
                .and_then(|columns| rows.checked_mul(columns))
        })
        .ok_or(RecognitionBenchmarkError::ComparisonLimitReached)?;
    if cells > maximum_cells {
        return Err(RecognitionBenchmarkError::ComparisonLimitReached);
    }
    let columns = hypothesis
        .len()
        .checked_add(1)
        .ok_or(RecognitionBenchmarkError::ComparisonLimitReached)?;
    let mut previous = bounded_edit_row(columns)?;
    let mut current = bounded_edit_row(columns)?;
    for (index, cell) in previous.iter_mut().enumerate().skip(1) {
        let insertions =
            u32::try_from(index).map_err(|_| RecognitionBenchmarkError::ArithmeticOverflow)?;
        *cell = EditCounts {
            errors: insertions,
            insertions,
            ..EditCounts::default()
        };
    }

    for (reference_index, reference_item) in reference.iter().enumerate() {
        let deletions = u32::try_from(reference_index.saturating_add(1))
            .map_err(|_| RecognitionBenchmarkError::ArithmeticOverflow)?;
        current[0] = EditCounts {
            errors: deletions,
            deletions,
            ..EditCounts::default()
        };
        for (hypothesis_index, hypothesis_item) in hypothesis.iter().enumerate() {
            let column = hypothesis_index.saturating_add(1);
            current[column] = if reference_item == hypothesis_item {
                previous[column - 1]
            } else {
                choose_edit(
                    previous[column - 1].substitution()?,
                    previous[column].deletion()?,
                    current[column - 1].insertion()?,
                )
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    Ok(previous[hypothesis.len()])
}

fn bounded_edit_row(length: usize) -> Result<Vec<EditCounts>, RecognitionBenchmarkError> {
    let mut row = Vec::new();
    row.try_reserve_exact(length)
        .map_err(|_| RecognitionBenchmarkError::AllocationFailed)?;
    row.resize(length, EditCounts::default());
    Ok(row)
}

const fn choose_edit(
    substitution: EditCounts,
    deletion: EditCounts,
    insertion: EditCounts,
) -> EditCounts {
    if substitution.errors <= deletion.errors && substitution.errors <= insertion.errors {
        substitution
    } else if deletion.errors <= insertion.errors {
        deletion
    } else {
        insertion
    }
}

fn error_rate(
    errors: u32,
    reference_units: usize,
) -> Result<Option<u64>, RecognitionBenchmarkError> {
    if reference_units == 0 {
        return Ok(None);
    }
    let numerator = u128::from(errors)
        .checked_mul(BASIS_POINTS)
        .ok_or(RecognitionBenchmarkError::ArithmeticOverflow)?;
    let denominator = u128::try_from(reference_units)
        .map_err(|_| RecognitionBenchmarkError::ArithmeticOverflow)?;
    Ok(Some(u64::try_from(numerator / denominator).map_err(
        |_| RecognitionBenchmarkError::ArithmeticOverflow,
    )?))
}

/// Payload-free recognition scoring failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecognitionBenchmarkError {
    /// A configured limit is zero or exceeds its compiled ceiling.
    InvalidConfig,
    /// A borrowed transcript exceeded the existing ASR byte limit.
    TextTooLong,
    /// A whitespace-token count exceeded its configured limit.
    TokenLimitReached,
    /// A non-whitespace Unicode-scalar count exceeded its configured limit.
    CharacterLimitReached,
    /// A dynamic-programming comparison exceeded its configured work limit.
    ComparisonLimitReached,
    /// Bounded metric memory could not be reserved.
    AllocationFailed,
    /// Text contains a scalar outside the selected dataset policy.
    UnsupportedNormalizationText,
    /// Numeric metric conversion or accumulation overflowed.
    ArithmeticOverflow,
}

impl fmt::Display for RecognitionBenchmarkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConfig => "invalid recognition benchmark configuration",
            Self::TextTooLong => "recognition benchmark text exceeded its bound",
            Self::TokenLimitReached => "recognition benchmark token limit was reached",
            Self::CharacterLimitReached => "recognition benchmark character limit was reached",
            Self::ComparisonLimitReached => "recognition benchmark work limit was reached",
            Self::AllocationFailed => "recognition benchmark allocation failed",
            Self::UnsupportedNormalizationText => {
                "recognition benchmark text is unsupported by its normalization policy"
            }
            Self::ArithmeticOverflow => "recognition benchmark metric arithmetic overflowed",
        })
    }
}

impl Error for RecognitionBenchmarkError {}

/// Validated memory and observation limits for one benchmark case.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamingBenchmarkConfig {
    maximum_observations: usize,
    maximum_partial_bytes: usize,
}

impl StreamingBenchmarkConfig {
    /// Creates a bounded recorder policy.
    ///
    /// # Errors
    ///
    /// Rejects zero/excessive observations or a zero/transcript-exceeding
    /// partial-text bound.
    pub const fn new(
        maximum_observations: usize,
        maximum_partial_bytes: usize,
    ) -> Result<Self, StreamingBenchmarkError> {
        if maximum_observations == 0
            || maximum_observations > MAX_BENCHMARK_OBSERVATIONS
            || maximum_partial_bytes == 0
            || maximum_partial_bytes > MAX_TRANSCRIPT_BYTES
        {
            return Err(StreamingBenchmarkError::InvalidConfig);
        }
        Ok(Self {
            maximum_observations,
            maximum_partial_bytes,
        })
    }

    /// Returns the maximum partial and inference observations retained.
    #[must_use]
    pub const fn maximum_observations(self) -> usize {
        self.maximum_observations
    }

    /// Returns the maximum UTF-8 bytes retained for one previous partial.
    #[must_use]
    pub const fn maximum_partial_bytes(self) -> usize {
        self.maximum_partial_bytes
    }
}

impl Default for StreamingBenchmarkConfig {
    fn default() -> Self {
        Self {
            maximum_observations: 4_096,
            maximum_partial_bytes: MAX_TRANSCRIPT_BYTES,
        }
    }
}

/// Numeric-only result for one local streaming benchmark case.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamingBenchmarkSummary {
    /// Partial hypothesis observations accepted.
    pub partial_observations: usize,
    /// Observed changes after the first partial.
    pub partial_revisions: usize,
    /// Revision frequency over transitions, in basis points.
    pub partial_revision_rate_basis_points: u32,
    /// Unicode scalar values replaced outside stable prefix/suffix regions.
    pub revised_characters: u64,
    /// Time from case start to the first non-empty partial.
    pub time_to_first_partial: Option<Duration>,
    /// Unicode scalar divergence between the last partial and final text.
    pub final_partial_divergence_characters: u64,
    /// Local inference windows measured.
    pub inference_calls: usize,
    /// Sum of canonical samples presented across inference windows.
    pub inference_samples: u64,
    /// Total wall time spent inside local inference.
    pub inference_elapsed: Duration,
    /// Nearest-rank median local inference time.
    pub inference_p50: Duration,
    /// Nearest-rank p95 local inference time.
    pub inference_p95: Duration,
    /// Local inference real-time factor in basis points (`10_000 == 1.0`).
    pub real_time_factor_basis_points: u64,
    /// Immutable consensus deltas observed.
    pub commits_observed: usize,
    /// Generation/sequence transitions violating monotonic commit ordering.
    pub commit_order_violations: usize,
}

/// Volatile recorder retaining only one bounded previous partial plus numeric
/// inference timings.
///
/// This type intentionally implements neither `Clone` nor `Debug`. Partial and
/// final text are never returned, logged, serialized, or included in errors.
pub struct StreamingBenchmarkRecorder {
    config: StreamingBenchmarkConfig,
    previous_partial: Vec<u8>,
    partial_observations: usize,
    partial_revisions: usize,
    revised_characters: u64,
    first_partial: Option<Duration>,
    last_partial_elapsed: Duration,
    inference_micros: Vec<u64>,
    inference_samples: u64,
    inference_elapsed_micros: u64,
    commit_generation: Option<u64>,
    commit_sequence: u64,
    commits_observed: usize,
    commit_order_violations: usize,
}

impl StreamingBenchmarkRecorder {
    /// Preallocates the complete sensitive-text and numeric-timing bounds.
    ///
    /// # Errors
    ///
    /// Returns a fixed allocation failure without exposing fixture data.
    pub fn new(config: StreamingBenchmarkConfig) -> Result<Self, StreamingBenchmarkError> {
        let mut previous_partial = Vec::new();
        previous_partial
            .try_reserve_exact(config.maximum_partial_bytes)
            .map_err(|_| StreamingBenchmarkError::AllocationFailed)?;
        let mut inference_micros = Vec::new();
        inference_micros
            .try_reserve_exact(config.maximum_observations)
            .map_err(|_| StreamingBenchmarkError::AllocationFailed)?;
        Ok(Self {
            config,
            previous_partial,
            partial_observations: 0,
            partial_revisions: 0,
            revised_characters: 0,
            first_partial: None,
            last_partial_elapsed: Duration::ZERO,
            inference_micros,
            inference_samples: 0,
            inference_elapsed_micros: 0,
            commit_generation: None,
            commit_sequence: 0,
            commits_observed: 0,
            commit_order_violations: 0,
        })
    }

    /// Observes one borrowed pending hypothesis and immediately reduces it to
    /// bounded stability counters plus one overwritten previous-partial buffer.
    ///
    /// # Errors
    ///
    /// Rejects excessive observations/text or a non-monotonic elapsed time.
    pub fn observe_partial(
        &mut self,
        partial: &str,
        elapsed_from_start: Duration,
    ) -> Result<(), StreamingBenchmarkError> {
        if self.partial_observations == self.config.maximum_observations {
            return Err(StreamingBenchmarkError::ObservationLimitReached);
        }
        if partial.len() > self.config.maximum_partial_bytes {
            return Err(StreamingBenchmarkError::TextTooLong);
        }
        if self.partial_observations > 0 && elapsed_from_start < self.last_partial_elapsed {
            return Err(StreamingBenchmarkError::TimeReversed);
        }

        let previous = std::str::from_utf8(&self.previous_partial)
            .map_err(|_| StreamingBenchmarkError::InvalidState)?;
        if self.partial_observations > 0 && previous != partial {
            self.partial_revisions = self.partial_revisions.saturating_add(1);
            let revised = divergent_characters(previous, partial)?;
            self.revised_characters = self
                .revised_characters
                .checked_add(revised)
                .ok_or(StreamingBenchmarkError::ArithmeticOverflow)?;
        }
        if self.first_partial.is_none() && !partial.is_empty() {
            self.first_partial = Some(elapsed_from_start);
        }

        self.previous_partial.fill(0);
        self.previous_partial.clear();
        self.previous_partial.extend_from_slice(partial.as_bytes());
        self.partial_observations = self.partial_observations.saturating_add(1);
        self.last_partial_elapsed = elapsed_from_start;
        Ok(())
    }

    /// Records one local inference window using only its size and wall time.
    ///
    /// # Errors
    ///
    /// Rejects zero/excessive windows, observation exhaustion, or numeric
    /// overflow.
    pub fn observe_inference(
        &mut self,
        inference_samples: usize,
        inference_elapsed: Duration,
    ) -> Result<(), StreamingBenchmarkError> {
        if inference_samples == 0 || inference_samples > MAX_INFERENCE_SAMPLES {
            return Err(StreamingBenchmarkError::InvalidInference);
        }
        if self.inference_micros.len() == self.config.maximum_observations {
            return Err(StreamingBenchmarkError::ObservationLimitReached);
        }
        let micros = u64::try_from(inference_elapsed.as_micros())
            .map_err(|_| StreamingBenchmarkError::ArithmeticOverflow)?;
        let samples = u64::try_from(inference_samples)
            .map_err(|_| StreamingBenchmarkError::ArithmeticOverflow)?;
        let total_samples = self
            .inference_samples
            .checked_add(samples)
            .ok_or(StreamingBenchmarkError::ArithmeticOverflow)?;
        let total_micros = self
            .inference_elapsed_micros
            .checked_add(micros)
            .ok_or(StreamingBenchmarkError::ArithmeticOverflow)?;

        self.inference_micros.push(micros);
        self.inference_samples = total_samples;
        self.inference_elapsed_micros = total_micros;
        Ok(())
    }

    /// Observes numeric metadata for one immutable consensus delta.
    ///
    /// Invalid generation/sequence transitions are counted rather than
    /// accepted as valid ordering evidence. Transcript text is not required.
    pub fn observe_commit(&mut self, generation: u64, sequence: u64) {
        let valid = match self.commit_generation {
            None => sequence == 1,
            Some(current) if generation == current => {
                self.commit_sequence.checked_add(1) == Some(sequence)
            }
            Some(current) if generation > current => sequence == 1,
            Some(_) => false,
        };
        self.commits_observed = self.commits_observed.saturating_add(1);
        if valid {
            self.commit_generation = Some(generation);
            self.commit_sequence = sequence;
        } else {
            self.commit_order_violations = self.commit_order_violations.saturating_add(1);
        }
    }

    /// Reduces the last borrowed final transcript to numeric divergence and
    /// returns a payload-free summary. The retained previous partial is erased.
    ///
    /// # Errors
    ///
    /// Rejects a final transcript exceeding the configured text bound or
    /// metric arithmetic outside the report representation.
    pub fn finish(
        mut self,
        final_text: &str,
    ) -> Result<StreamingBenchmarkSummary, StreamingBenchmarkError> {
        if final_text.len() > self.config.maximum_partial_bytes {
            return Err(StreamingBenchmarkError::TextTooLong);
        }
        let previous = std::str::from_utf8(&self.previous_partial)
            .map_err(|_| StreamingBenchmarkError::InvalidState)?;
        let final_partial_divergence_characters = divergent_characters(previous, final_text)?;
        let transitions = self.partial_observations.saturating_sub(1);
        let revision_rate = if transitions == 0 {
            0
        } else {
            let numerator = u128::try_from(self.partial_revisions)
                .map_err(|_| StreamingBenchmarkError::ArithmeticOverflow)?
                .checked_mul(BASIS_POINTS)
                .ok_or(StreamingBenchmarkError::ArithmeticOverflow)?;
            let denominator = u128::try_from(transitions)
                .map_err(|_| StreamingBenchmarkError::ArithmeticOverflow)?;
            u32::try_from(numerator / denominator)
                .map_err(|_| StreamingBenchmarkError::ArithmeticOverflow)?
        };

        self.inference_micros.sort_unstable();
        let inference_p50 = percentile_duration(&self.inference_micros, 50)?;
        let inference_p95 = percentile_duration(&self.inference_micros, 95)?;
        let rtf =
            real_time_factor_basis_points(self.inference_samples, self.inference_elapsed_micros)?;
        let summary = StreamingBenchmarkSummary {
            partial_observations: self.partial_observations,
            partial_revisions: self.partial_revisions,
            partial_revision_rate_basis_points: revision_rate,
            revised_characters: self.revised_characters,
            time_to_first_partial: self.first_partial,
            final_partial_divergence_characters,
            inference_calls: self.inference_micros.len(),
            inference_samples: self.inference_samples,
            inference_elapsed: Duration::from_micros(self.inference_elapsed_micros),
            inference_p50,
            inference_p95,
            real_time_factor_basis_points: rtf,
            commits_observed: self.commits_observed,
            commit_order_violations: self.commit_order_violations,
        };
        self.erase_partial();
        Ok(summary)
    }

    fn erase_partial(&mut self) {
        self.previous_partial.fill(0);
        self.previous_partial.clear();
    }
}

impl Drop for StreamingBenchmarkRecorder {
    fn drop(&mut self) {
        self.erase_partial();
    }
}

fn divergent_characters(left: &str, right: &str) -> Result<u64, StreamingBenchmarkError> {
    let left_count = left.chars().count();
    let right_count = right.chars().count();
    let prefix = left
        .chars()
        .zip(right.chars())
        .take_while(|(left, right)| left == right)
        .count();
    let suffix_limit = left_count
        .saturating_sub(prefix)
        .min(right_count.saturating_sub(prefix));
    let suffix = left
        .chars()
        .rev()
        .zip(right.chars().rev())
        .take(suffix_limit)
        .take_while(|(left, right)| left == right)
        .count();
    let divergent = left_count
        .saturating_sub(prefix)
        .saturating_sub(suffix)
        .saturating_add(right_count.saturating_sub(prefix).saturating_sub(suffix));
    u64::try_from(divergent).map_err(|_| StreamingBenchmarkError::ArithmeticOverflow)
}

fn percentile_duration(
    sorted_micros: &[u64],
    percentile: usize,
) -> Result<Duration, StreamingBenchmarkError> {
    if sorted_micros.is_empty() {
        return Ok(Duration::ZERO);
    }
    let rank = sorted_micros
        .len()
        .checked_mul(percentile)
        .and_then(|value| value.checked_add(99))
        .ok_or(StreamingBenchmarkError::ArithmeticOverflow)?
        / 100;
    let index = rank.saturating_sub(1).min(sorted_micros.len() - 1);
    Ok(Duration::from_micros(sorted_micros[index]))
}

pub(crate) fn real_time_factor_basis_points(
    inference_samples: u64,
    inference_micros: u64,
) -> Result<u64, StreamingBenchmarkError> {
    if inference_samples == 0 {
        return Ok(0);
    }
    let audio_micros = u128::from(inference_samples)
        .checked_mul(MICROS_PER_SECOND)
        .ok_or(StreamingBenchmarkError::ArithmeticOverflow)?
        / ASR_SAMPLES_PER_SECOND;
    let scaled = u128::from(inference_micros)
        .checked_mul(BASIS_POINTS)
        .ok_or(StreamingBenchmarkError::ArithmeticOverflow)?;
    u64::try_from(scaled / audio_micros).map_err(|_| StreamingBenchmarkError::ArithmeticOverflow)
}

/// Payload-free streaming benchmark failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamingBenchmarkError {
    /// Recorder bounds are zero or exceed compiled limits.
    InvalidConfig,
    /// Recorder preallocation failed.
    AllocationFailed,
    /// The configured partial/inference observation limit was reached.
    ObservationLimitReached,
    /// A partial or final transcript exceeded the configured byte bound.
    TextTooLong,
    /// Partial elapsed time moved backwards.
    TimeReversed,
    /// An inference window was empty or exceeded the ASR hard limit.
    InvalidInference,
    /// Numeric metric conversion or accumulation overflowed.
    ArithmeticOverflow,
    /// Internal sensitive text state was not valid UTF-8.
    InvalidState,
}

impl fmt::Display for StreamingBenchmarkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidConfig => "invalid streaming benchmark configuration",
            Self::AllocationFailed => "streaming benchmark allocation failed",
            Self::ObservationLimitReached => "streaming benchmark observation limit reached",
            Self::TextTooLong => "streaming benchmark text exceeded its bound",
            Self::TimeReversed => "streaming benchmark time moved backwards",
            Self::InvalidInference => "streaming benchmark inference window was invalid",
            Self::ArithmeticOverflow => "streaming benchmark metric arithmetic overflowed",
            Self::InvalidState => "streaming benchmark state was invalid",
        };
        formatter.write_str(message)
    }
}

impl Error for StreamingBenchmarkError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognition_counts_substitutions_deletions_and_insertions(
    ) -> Result<(), RecognitionBenchmarkError> {
        let config = RecognitionBenchmarkConfig::default();
        let substitution = measure_recognition("a b c", "a x c", config)?;
        assert_eq!(substitution.word_substitutions, 1);
        assert_eq!(substitution.word_deletions, 0);
        assert_eq!(substitution.word_insertions, 0);
        assert_eq!(substitution.word_errors, 1);
        assert_eq!(substitution.word_error_rate_basis_points, Some(3_333));
        assert_eq!(substitution.character_substitutions, 1);
        assert_eq!(substitution.character_error_rate_basis_points, Some(3_333));

        let deletion = measure_recognition("a b c", "a c", config)?;
        assert_eq!(deletion.word_deletions, 1);
        assert_eq!(deletion.word_errors, 1);

        let insertion = measure_recognition("a c", "a b c", config)?;
        assert_eq!(insertion.word_insertions, 1);
        assert_eq!(insertion.word_errors, 1);
        Ok(())
    }

    #[test]
    fn recognition_is_unicode_scalar_safe_and_whitespace_stable(
    ) -> Result<(), RecognitionBenchmarkError> {
        let config = RecognitionBenchmarkConfig::default();
        let whitespace = measure_recognition("hello \n world", "hello world", config)?;
        assert_eq!(whitespace.word_errors, 0);
        assert_eq!(whitespace.character_errors, 0);

        let hindi = measure_recognition("नमस्ते दुनिया", "नमस्ते दुनियाँ", config)?;
        assert_eq!(hindi.reference_words, 2);
        assert_eq!(hindi.word_substitutions, 1);
        assert_eq!(hindi.word_error_rate_basis_points, Some(5_000));
        assert!(hindi.character_errors > 0);
        Ok(())
    }

    #[test]
    fn recognition_keeps_case_and_punctuation_semantics_explicit(
    ) -> Result<(), RecognitionBenchmarkError> {
        let summary =
            measure_recognition("Hello.", "hello", RecognitionBenchmarkConfig::default())?;
        assert_eq!(summary.word_substitutions, 1);
        assert_eq!(summary.word_error_rate_basis_points, Some(10_000));
        assert_eq!(summary.character_substitutions, 1);
        assert_eq!(summary.character_deletions, 1);
        assert_eq!(summary.character_errors, 2);
        Ok(())
    }

    #[test]
    fn librispeech_policy_is_explicit_bounded_and_ascii_focused(
    ) -> Result<(), RecognitionBenchmarkError> {
        let config = RecognitionBenchmarkConfig::default()
            .with_text_policy(RecognitionTextPolicy::LibriSpeechEnglish);
        assert_eq!(
            config.text_policy(),
            RecognitionTextPolicy::LibriSpeechEnglish
        );
        let summary = measure_recognition("HE SAID: IT'S READY", "He said, it’s ready.", config)?;
        assert_eq!(summary.word_errors, 0);
        assert_eq!(summary.character_errors, 0);
        assert_eq!(
            measure_recognition("CAFE", "café", config),
            Err(RecognitionBenchmarkError::UnsupportedNormalizationText)
        );
        Ok(())
    }

    #[test]
    fn fleurs_hindi_policy_is_explicit_bounded_and_unicode_safe(
    ) -> Result<(), RecognitionBenchmarkError> {
        let config = RecognitionBenchmarkConfig::default()
            .with_text_policy(RecognitionTextPolicy::FleursHindi);
        assert_eq!(config.text_policy(), RecognitionTextPolicy::FleursHindi);
        let summary = measure_recognition(
            "पुलिस ने कहा कि शव पुराना है",
            "पुलिस  ने कहा—कि शव पुराना है।",
            config,
        )?;
        assert_eq!(summary.word_errors, 0);
        assert_eq!(summary.character_errors, 0);
        assert_eq!(
            summary.reference_devanagari_characters,
            summary.reference_characters
        );
        assert_eq!(
            summary.hypothesis_devanagari_characters,
            summary.hypothesis_characters
        );
        let wrong_script = measure_recognition("नमस्ते", "hello", config)?;
        assert_eq!(wrong_script.reference_devanagari_characters, 6);
        assert_eq!(wrong_script.hypothesis_devanagari_characters, 0);
        assert_eq!(wrong_script.reference_ascii_latin_characters, 0);
        assert_eq!(wrong_script.hypothesis_ascii_latin_characters, 5);
        assert_eq!(
            measure_recognition("नमस्ते", "\0", config),
            Err(RecognitionBenchmarkError::UnsupportedNormalizationText)
        );
        Ok(())
    }

    #[test]
    fn recognition_empty_reference_is_reported_without_dividing_by_zero(
    ) -> Result<(), RecognitionBenchmarkError> {
        let summary =
            measure_recognition("", "extra words", RecognitionBenchmarkConfig::default())?;
        assert_eq!(summary.reference_words, 0);
        assert_eq!(summary.word_insertions, 2);
        assert_eq!(summary.word_error_rate_basis_points, None);
        assert_eq!(summary.reference_characters, 0);
        assert_eq!(summary.character_insertions, 10);
        assert_eq!(summary.character_error_rate_basis_points, None);
        Ok(())
    }

    #[test]
    fn recognition_configuration_and_work_are_strictly_bounded(
    ) -> Result<(), RecognitionBenchmarkError> {
        assert_eq!(
            RecognitionBenchmarkConfig::new(0, 1, 1),
            Err(RecognitionBenchmarkError::InvalidConfig)
        );
        let token_config = RecognitionBenchmarkConfig::new(2, 16, 100)?;
        assert_eq!(
            measure_recognition("a b c", "a", token_config),
            Err(RecognitionBenchmarkError::TokenLimitReached)
        );
        let character_config = RecognitionBenchmarkConfig::new(8, 3, 100)?;
        assert_eq!(
            measure_recognition("abcd", "a", character_config),
            Err(RecognitionBenchmarkError::CharacterLimitReached)
        );
        let cell_config = RecognitionBenchmarkConfig::new(8, 8, 4)?;
        assert_eq!(
            measure_recognition("ab", "ab", cell_config),
            Err(RecognitionBenchmarkError::ComparisonLimitReached)
        );
        Ok(())
    }

    #[test]
    fn recognition_failures_never_echo_fixture_text() -> Result<(), RecognitionBenchmarkError> {
        let marker = "recognition-private-marker";
        let config = RecognitionBenchmarkConfig::new(1, 1, 1)?;
        let error = measure_recognition(marker, marker, config)
            .err()
            .ok_or(RecognitionBenchmarkError::InvalidConfig)?;
        assert!(!error.to_string().contains(marker));
        Ok(())
    }

    #[test]
    fn configuration_enforces_compiled_bounds() {
        assert_eq!(
            StreamingBenchmarkConfig::new(0, 1),
            Err(StreamingBenchmarkError::InvalidConfig)
        );
        assert_eq!(
            StreamingBenchmarkConfig::new(1, MAX_TRANSCRIPT_BYTES + 1),
            Err(StreamingBenchmarkError::InvalidConfig)
        );
        assert!(StreamingBenchmarkConfig::new(32, 1_024).is_ok());
    }

    #[test]
    fn stability_and_latency_reduce_to_numeric_metrics() -> Result<(), StreamingBenchmarkError> {
        let config = StreamingBenchmarkConfig::new(8, 64)?;
        let mut recorder = StreamingBenchmarkRecorder::new(config)?;
        recorder.observe_partial("hello", Duration::from_millis(100))?;
        recorder.observe_partial("hello", Duration::from_millis(200))?;
        recorder.observe_partial("hullo", Duration::from_millis(300))?;
        recorder.observe_inference(16_000, Duration::from_millis(750))?;
        recorder.observe_inference(16_000, Duration::from_millis(250))?;
        recorder.observe_commit(4, 1);
        recorder.observe_commit(4, 2);

        let summary = recorder.finish("hullo!")?;
        assert_eq!(summary.partial_observations, 3);
        assert_eq!(summary.partial_revisions, 1);
        assert_eq!(summary.partial_revision_rate_basis_points, 5_000);
        assert_eq!(summary.revised_characters, 2);
        assert_eq!(
            summary.time_to_first_partial,
            Some(Duration::from_millis(100))
        );
        assert_eq!(summary.final_partial_divergence_characters, 1);
        assert_eq!(summary.inference_calls, 2);
        assert_eq!(summary.inference_samples, 32_000);
        assert_eq!(summary.inference_elapsed, Duration::from_secs(1));
        assert_eq!(summary.inference_p50, Duration::from_millis(250));
        assert_eq!(summary.inference_p95, Duration::from_millis(750));
        assert_eq!(summary.real_time_factor_basis_points, 5_000);
        assert_eq!(summary.commits_observed, 2);
        assert_eq!(summary.commit_order_violations, 0);
        Ok(())
    }

    #[test]
    fn unicode_divergence_counts_characters_not_utf8_bytes() -> Result<(), StreamingBenchmarkError>
    {
        let mut recorder = StreamingBenchmarkRecorder::new(StreamingBenchmarkConfig::new(4, 64)?)?;
        recorder.observe_partial("नमस्ते", Duration::from_millis(1))?;
        recorder.observe_partial("नमस्ते!", Duration::from_millis(2))?;
        let summary = recorder.finish("नमस्ते!")?;
        assert_eq!(summary.revised_characters, 1);
        assert_eq!(summary.final_partial_divergence_characters, 0);
        Ok(())
    }

    #[test]
    fn commit_order_violations_are_counted_without_text() -> Result<(), StreamingBenchmarkError> {
        let mut recorder = StreamingBenchmarkRecorder::new(StreamingBenchmarkConfig::new(4, 8)?)?;
        recorder.observe_commit(3, 1);
        recorder.observe_commit(3, 3);
        recorder.observe_commit(2, 2);
        recorder.observe_commit(4, 1);
        let summary = recorder.finish("")?;
        assert_eq!(summary.commits_observed, 4);
        assert_eq!(summary.commit_order_violations, 2);
        Ok(())
    }

    #[test]
    fn limits_and_errors_do_not_echo_sensitive_markers() -> Result<(), StreamingBenchmarkError> {
        const MARKER: &str = "SENSITIVE_TOKEN_7QX";
        let mut recorder =
            StreamingBenchmarkRecorder::new(StreamingBenchmarkConfig::new(1, MARKER.len())?)?;
        recorder.observe_partial(MARKER, Duration::from_millis(2))?;
        let limit = recorder.observe_partial(MARKER, Duration::from_millis(3));
        assert_eq!(limit, Err(StreamingBenchmarkError::ObservationLimitReached));
        assert!(!format!("{}", StreamingBenchmarkError::TextTooLong).contains(MARKER));
        assert_eq!(
            recorder.observe_inference(0, Duration::ZERO),
            Err(StreamingBenchmarkError::InvalidInference)
        );
        Ok(())
    }
}
