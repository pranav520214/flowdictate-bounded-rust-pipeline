//! Bounded, deterministic transcript cleanup for `FlowDictate`.
//!
//! This crate performs no model inference, persistence, logging, context access,
//! or network work. Input is borrowed; cleaned UTF-8 is owned by an opaque value
//! that overwrites its initialized bytes on drop.

use std::{error::Error, fmt, str};

mod dictionary;
mod editor;
mod fallback;
mod routing;
mod semantic;
mod spoken_rules;
mod validation;

pub use dictionary::{
    apply_user_dictionary, DictionaryEntry, DictionaryError, DictionaryReport,
    DictionaryTranscript, MAX_DICTIONARY_ENTRIES, MAX_DICTIONARY_FIELD_BYTES,
};
pub use editor::{
    refine_with_optional_editor, LocalEditor, LocalEditorAttempt, LocalEditorError,
    LocalEditorOutput, OptionalRefinementError, OptionalRefinementOutput, OptionalRefinementPath,
    OptionalRefinementReport,
};
pub use fallback::{
    refine_without_model, sanitize_raw_transcript, RefinementOutput, RefinementPath,
    SanitizationReport, SanitizedTranscript,
};
pub use routing::{
    select_refinement_route, LocalEditorGate, RefinementNeed, RefinementRoute, RefinementSignals,
};
pub use semantic::{
    verify_semantic_candidate, SemanticVerificationConfig, SemanticVerificationError,
    SemanticVerificationReport, SemanticallyValidated, MAX_PROTECTED_TOKENS, MAX_SEMANTIC_TOKENS,
};
pub use spoken_rules::{
    apply_spoken_rules, SpokenRulesPolicy, SpokenRulesReport, SpokenRulesTranscript,
};
pub use validation::{
    validate_refined_output, LineBreakPolicy, OutputValidationConfig, OutputValidationError,
    OutputValidationReport, ValidatedOutput, MAX_OUTPUT_EXPANSION_BASIS_POINTS,
};

/// Hard ceiling for one raw or cleaned transcript.
pub const MAX_CLEANUP_BYTES: usize = 65_536;

/// Validated bounds for deterministic cleanup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CleanupConfig {
    maximum_bytes: usize,
}

impl CleanupConfig {
    /// Creates a cleanup configuration within the compiled hard limit.
    ///
    /// # Errors
    ///
    /// Rejects zero or more than [`MAX_CLEANUP_BYTES`] bytes.
    pub const fn new(maximum_bytes: usize) -> Result<Self, CleanupError> {
        if maximum_bytes == 0 || maximum_bytes > MAX_CLEANUP_BYTES {
            Err(CleanupError::InvalidConfig)
        } else {
            Ok(Self { maximum_bytes })
        }
    }

    /// Returns the maximum accepted input and output size.
    #[must_use]
    pub const fn maximum_bytes(self) -> usize {
        self.maximum_bytes
    }
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            maximum_bytes: MAX_CLEANUP_BYTES,
        }
    }
}

/// Non-sensitive counters describing one cleanup pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CleanupReport {
    /// Borrowed UTF-8 bytes inspected.
    pub input_bytes: usize,
    /// Cleaned UTF-8 bytes retained by the result owner.
    pub output_bytes: usize,
    /// Whether leading, trailing, duplicate, or non-space whitespace changed.
    pub whitespace_normalized: bool,
    /// Whether whitespace adjacent to punctuation changed.
    pub punctuation_spacing_normalized: bool,
    /// ASCII sentence-initial letters capitalized.
    pub ascii_capitalizations: usize,
}

impl CleanupReport {
    /// Reports whether deterministic cleanup changed the input.
    #[must_use]
    pub const fn changed(self) -> bool {
        self.whitespace_normalized
            || self.punctuation_spacing_normalized
            || self.ascii_capitalizations != 0
    }
}

/// Opaque cleaned transcript whose initialized UTF-8 bytes are wiped on drop.
///
/// This type intentionally implements neither `Clone` nor `Debug`.
pub struct CleanedTranscript {
    bytes: Vec<u8>,
    report: CleanupReport,
}

impl CleanedTranscript {
    /// Borrows the validated cleaned text.
    #[must_use]
    pub fn text(&self) -> &str {
        str::from_utf8(&self.bytes).unwrap_or_default()
    }

    /// Returns numeric and boolean cleanup metadata without transcript content.
    #[must_use]
    pub const fn report(&self) -> CleanupReport {
        self.report
    }
}

impl Drop for CleanedTranscript {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}

/// Fixed, payload-free deterministic-cleanup failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupError {
    /// The configured bound is zero or exceeds the compiled hard limit.
    InvalidConfig,
    /// The borrowed input exceeds its configured byte bound.
    InputTooLong,
    /// The input contains a non-whitespace control character.
    InvalidControlCharacter,
    /// The input contains an explicit bidirectional-formatting scalar.
    InvalidBidiControl,
    /// The input contains a Unicode noncharacter scalar.
    InvalidNoncharacter,
    /// Bounded output storage could not be reserved.
    AllocationFailed,
}

impl fmt::Display for CleanupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConfig => "cleanup configuration is invalid",
            Self::InputTooLong => "cleanup input exceeds its bound",
            Self::InvalidControlCharacter => "cleanup input contains a control character",
            Self::InvalidBidiControl => "cleanup input contains a bidirectional control character",
            Self::InvalidNoncharacter => "cleanup input contains a Unicode noncharacter",
            Self::AllocationFailed => "cleanup output allocation failed",
        })
    }
}

impl Error for CleanupError {}

/// Applies conservative deterministic cleanup in one bounded pass.
///
/// The pass collapses whitespace, removes whitespace before closing punctuation
/// and after opening punctuation, and capitalizes ASCII letters at the start or
/// after an observed sentence-ending punctuation-plus-whitespace boundary. It
/// never invents missing whitespace, which preserves URLs, decimal numbers,
/// abbreviations, and other punctuation-delimited tokens.
///
/// # Errors
///
/// Returns a fixed category for invalid configuration, excessive input,
/// non-whitespace control characters, or allocation failure. Errors never
/// include transcript content.
pub fn clean_transcript(
    input: &str,
    config: CleanupConfig,
) -> Result<CleanedTranscript, CleanupError> {
    if config.maximum_bytes == 0 || config.maximum_bytes > MAX_CLEANUP_BYTES {
        return Err(CleanupError::InvalidConfig);
    }
    if input.len() > config.maximum_bytes {
        return Err(CleanupError::InputTooLong);
    }

    let mut cleaned = CleanedTranscript {
        bytes: Vec::new(),
        report: CleanupReport {
            input_bytes: input.len(),
            ..CleanupReport::default()
        },
    };
    cleaned
        .bytes
        .try_reserve_exact(input.len())
        .map_err(|_| CleanupError::AllocationFailed)?;

    let mut pending_space = false;
    let mut capitalize_ascii = true;
    let mut sentence_boundary_pending = false;
    let mut current_token_has_period = false;
    let mut last_emitted = None;

    for character in input.chars() {
        if character.is_whitespace() {
            if pending_space || character != ' ' {
                cleaned.report.whitespace_normalized = true;
            }
            pending_space = true;
            if sentence_boundary_pending {
                capitalize_ascii = true;
                sentence_boundary_pending = false;
            }
            continue;
        }
        if character.is_control() {
            return Err(CleanupError::InvalidControlCharacter);
        }
        if validation::is_bidi_control(character) {
            return Err(CleanupError::InvalidBidiControl);
        }
        if validation::is_noncharacter(character) {
            return Err(CleanupError::InvalidNoncharacter);
        }

        if is_closing_punctuation(character) {
            if pending_space {
                cleaned.report.punctuation_spacing_normalized = true;
                pending_space = false;
            }
            let abbreviation_period = character == '.' && current_token_has_period;
            push_character(&mut cleaned.bytes, character);
            last_emitted = Some(character);
            current_token_has_period |= character == '.';
            if is_sentence_terminal(character) && !abbreviation_period {
                sentence_boundary_pending = true;
            }
            continue;
        }

        if pending_space {
            if cleaned.bytes.is_empty() {
                cleaned.report.whitespace_normalized = true;
            } else if last_emitted.is_some_and(is_opening_punctuation) {
                cleaned.report.punctuation_spacing_normalized = true;
            } else {
                cleaned.bytes.push(b' ');
                current_token_has_period = false;
            }
            pending_space = false;
        }

        if sentence_boundary_pending && character.is_alphanumeric() {
            sentence_boundary_pending = false;
        }
        let emitted = if capitalize_ascii && character.is_ascii_lowercase() {
            cleaned.report.ascii_capitalizations += 1;
            character.to_ascii_uppercase()
        } else {
            character
        };
        push_character(&mut cleaned.bytes, emitted);
        last_emitted = Some(emitted);
        if character.is_alphanumeric() {
            capitalize_ascii = false;
        }
    }

    if pending_space {
        cleaned.report.whitespace_normalized = true;
    }
    cleaned.report.output_bytes = cleaned.bytes.len();
    Ok(cleaned)
}

fn push_character(output: &mut Vec<u8>, character: char) {
    let mut encoded = [0_u8; 4];
    output.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
}

const fn is_opening_punctuation(character: char) -> bool {
    matches!(character, '(' | '[' | '{')
}

const fn is_closing_punctuation(character: char) -> bool {
    matches!(
        character,
        ',' | '.' | '!' | '?' | ';' | ':' | '%' | ')' | ']' | '}' | '।' | '॥'
    )
}

const fn is_sentence_terminal(character: char) -> bool {
    matches!(character, '.' | '!' | '?' | '।' | '॥')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_punctuation_and_sentence_case_are_cleaned() -> Result<(), CleanupError> {
        let cleaned = clean_transcript("  hello   ,  world  .  next  ", CleanupConfig::default())?;

        assert_eq!(cleaned.text(), "Hello, world. Next");
        assert_eq!(
            cleaned.report(),
            CleanupReport {
                input_bytes: 29,
                output_bytes: 18,
                whitespace_normalized: true,
                punctuation_spacing_normalized: true,
                ascii_capitalizations: 2,
            }
        );
        assert!(cleaned.report().changed());
        Ok(())
    }

    #[test]
    fn hindi_text_is_spacing_safe_without_case_rewrites() -> Result<(), CleanupError> {
        let cleaned = clean_transcript("  नमस्ते   दुनिया  ।  फिर   मिलेंगे ", CleanupConfig::default())?;

        assert_eq!(cleaned.text(), "नमस्ते दुनिया। फिर मिलेंगे");
        assert_eq!(cleaned.report().ascii_capitalizations, 0);
        Ok(())
    }

    #[test]
    fn absent_spaces_preserve_urls_decimals_and_abbreviations() -> Result<(), CleanupError> {
        let cleaned = clean_transcript(
            "visit https://example.com with version 3.14 e.g. today",
            CleanupConfig::default(),
        )?;

        assert_eq!(
            cleaned.text(),
            "Visit https://example.com with version 3.14 e.g. today"
        );
        Ok(())
    }

    #[test]
    fn numeric_prefix_does_not_capitalize_inside_a_token() -> Result<(), CleanupError> {
        let cleaned = clean_transcript("v2 engine. 3d model", CleanupConfig::default())?;

        assert_eq!(cleaned.text(), "V2 engine. 3d model");
        Ok(())
    }

    #[test]
    fn opening_and_closing_punctuation_drop_inner_spaces() -> Result<(), CleanupError> {
        let cleaned = clean_transcript("say ( hello ) now", CleanupConfig::default())?;

        assert_eq!(cleaned.text(), "Say (hello) now");
        assert!(cleaned.report().punctuation_spacing_normalized);
        Ok(())
    }

    #[test]
    fn empty_input_is_valid_and_unchanged() -> Result<(), CleanupError> {
        let cleaned = clean_transcript("", CleanupConfig::default())?;

        assert_eq!(cleaned.text(), "");
        assert!(!cleaned.report().changed());
        Ok(())
    }

    #[test]
    fn configuration_and_input_bounds_fail_closed() -> Result<(), CleanupError> {
        assert_eq!(CleanupConfig::new(0), Err(CleanupError::InvalidConfig));
        assert_eq!(
            CleanupConfig::new(MAX_CLEANUP_BYTES + 1),
            Err(CleanupError::InvalidConfig)
        );
        let config = CleanupConfig::new(4)?;
        assert!(matches!(
            clean_transcript("12345", config),
            Err(CleanupError::InputTooLong)
        ));
        Ok(())
    }

    #[test]
    fn non_whitespace_controls_fail_without_payload_errors() {
        let marker = "private-marker\0tail";
        let error = clean_transcript(marker, CleanupConfig::default());

        assert!(matches!(error, Err(CleanupError::InvalidControlCharacter)));
        assert!(!CleanupError::InvalidControlCharacter
            .to_string()
            .contains(marker));
    }

    #[test]
    fn bidi_controls_and_noncharacters_fail_strict_cleanup() {
        assert!(matches!(
            clean_transcript("safe\u{202e}text", CleanupConfig::default()),
            Err(CleanupError::InvalidBidiControl)
        ));
        assert!(matches!(
            clean_transcript("safe\u{fdd0}text", CleanupConfig::default()),
            Err(CleanupError::InvalidNoncharacter)
        ));
    }

    #[test]
    fn output_never_exceeds_borrowed_input_size() -> Result<(), CleanupError> {
        let input = "a  compact  sentence . next";
        let cleaned = clean_transcript(input, CleanupConfig::default())?;

        assert!(cleaned.report().output_bytes <= input.len());
        assert_eq!(cleaned.report().input_bytes, input.len());
        Ok(())
    }
}
