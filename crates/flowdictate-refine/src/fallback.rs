use std::str;

use crate::{
    clean_transcript, push_character,
    validation::{is_bidi_control, is_noncharacter},
    CleanedTranscript, CleanupConfig, CleanupError, CleanupReport,
};

/// Payload-free measurements from raw-transcript sanitization.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SanitizationReport {
    /// Borrowed UTF-8 bytes inspected.
    pub input_bytes: usize,
    /// Sanitized UTF-8 bytes retained by the result owner.
    pub output_bytes: usize,
    /// Whether leading, trailing, duplicate, or non-space whitespace changed.
    pub whitespace_normalized: bool,
    /// Non-whitespace control scalars removed.
    pub control_scalars_removed: usize,
    /// Explicit bidirectional-formatting scalars removed.
    pub bidi_controls_removed: usize,
    /// Unicode noncharacter scalars removed.
    pub noncharacters_removed: usize,
}

impl SanitizationReport {
    /// Reports whether sanitization changed the input.
    #[must_use]
    pub const fn changed(self) -> bool {
        self.whitespace_normalized
            || self.control_scalars_removed != 0
            || self.bidi_controls_removed != 0
            || self.noncharacters_removed != 0
    }
}

/// Opaque sanitized raw transcript whose initialized bytes are wiped on drop.
///
/// This type intentionally implements neither `Clone` nor `Debug`.
pub struct SanitizedTranscript {
    bytes: Vec<u8>,
    report: SanitizationReport,
}

impl SanitizedTranscript {
    /// Borrows sanitized text.
    #[must_use]
    pub fn text(&self) -> &str {
        str::from_utf8(&self.bytes).unwrap_or_default()
    }

    /// Returns numeric and boolean sanitization metadata.
    #[must_use]
    pub const fn report(&self) -> SanitizationReport {
        self.report
    }
}

impl Drop for SanitizedTranscript {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}

/// The model-free path that produced a final transcript.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefinementPath {
    /// Conservative deterministic cleanup succeeded.
    Deterministic,
    /// Unsafe input scalars required sanitized-raw fallback.
    SanitizedRaw,
}

enum RefinementOwner {
    Cleaned(CleanedTranscript),
    Sanitized(SanitizedTranscript),
}

/// Opaque model-free refinement output with one wipe-on-drop text owner.
///
/// This type intentionally implements neither `Clone` nor `Debug`.
pub struct RefinementOutput {
    owner: RefinementOwner,
}

impl RefinementOutput {
    /// Borrows the final model-free text.
    #[must_use]
    pub fn text(&self) -> &str {
        match &self.owner {
            RefinementOwner::Cleaned(output) => output.text(),
            RefinementOwner::Sanitized(output) => output.text(),
        }
    }

    /// Reports which model-free path produced the text.
    #[must_use]
    pub const fn path(&self) -> RefinementPath {
        match self.owner {
            RefinementOwner::Cleaned(_) => RefinementPath::Deterministic,
            RefinementOwner::Sanitized(_) => RefinementPath::SanitizedRaw,
        }
    }

    /// Returns cleanup metadata only when deterministic cleanup succeeded.
    #[must_use]
    pub const fn cleanup_report(&self) -> Option<CleanupReport> {
        match &self.owner {
            RefinementOwner::Cleaned(output) => Some(output.report()),
            RefinementOwner::Sanitized(_) => None,
        }
    }

    /// Returns sanitization metadata only when fallback was required.
    #[must_use]
    pub const fn sanitization_report(&self) -> Option<SanitizationReport> {
        match &self.owner {
            RefinementOwner::Cleaned(_) => None,
            RefinementOwner::Sanitized(output) => Some(output.report()),
        }
    }
}

/// Removes unsafe invisible/control scalars from a bounded raw transcript.
///
/// Visible text is otherwise preserved. Unicode whitespace is collapsed to one
/// ASCII space and leading/trailing whitespace is removed. The result owns one
/// bounded buffer and performs no model, file, logging, context, or network
/// operation.
///
/// # Errors
///
/// Returns a fixed category for excessive input or allocation failure.
pub fn sanitize_raw_transcript(
    input: &str,
    config: CleanupConfig,
) -> Result<SanitizedTranscript, CleanupError> {
    if input.len() > config.maximum_bytes() {
        return Err(CleanupError::InputTooLong);
    }

    let mut output = SanitizedTranscript {
        bytes: Vec::new(),
        report: SanitizationReport {
            input_bytes: input.len(),
            ..SanitizationReport::default()
        },
    };
    output
        .bytes
        .try_reserve_exact(input.len())
        .map_err(|_| CleanupError::AllocationFailed)?;

    let mut pending_space = false;
    for character in input.chars() {
        if character.is_whitespace() {
            if pending_space || character != ' ' {
                output.report.whitespace_normalized = true;
            }
            pending_space = true;
        } else if character.is_control() {
            output.report.control_scalars_removed += 1;
        } else if is_bidi_control(character) {
            output.report.bidi_controls_removed += 1;
        } else if is_noncharacter(character) {
            output.report.noncharacters_removed += 1;
        } else {
            if pending_space {
                if output.bytes.is_empty() {
                    output.report.whitespace_normalized = true;
                } else {
                    output.bytes.push(b' ');
                }
                pending_space = false;
            }
            push_character(&mut output.bytes, character);
        }
    }

    if pending_space {
        output.report.whitespace_normalized = true;
    }
    output.report.output_bytes = output.bytes.len();
    Ok(output)
}

/// Runs deterministic cleanup and falls back only for unsafe input scalars.
///
/// Bounds, configuration, and allocation failures remain errors. This function
/// never invokes a model or a network service.
///
/// # Errors
///
/// Returns a fixed cleanup category when bounded local processing cannot
/// produce an owned result.
pub fn refine_without_model(
    input: &str,
    config: CleanupConfig,
) -> Result<RefinementOutput, CleanupError> {
    let owner = match clean_transcript(input, config) {
        Ok(cleaned) => RefinementOwner::Cleaned(cleaned),
        Err(
            CleanupError::InvalidControlCharacter
            | CleanupError::InvalidBidiControl
            | CleanupError::InvalidNoncharacter,
        ) => RefinementOwner::Sanitized(sanitize_raw_transcript(input, config)?),
        Err(error) => return Err(error),
    };
    Ok(RefinementOutput { owner })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_input_uses_deterministic_cleanup() -> Result<(), CleanupError> {
        let output = refine_without_model("  hello  , world  ", CleanupConfig::default())?;

        assert_eq!(output.text(), "Hello, world");
        assert_eq!(output.path(), RefinementPath::Deterministic);
        assert!(output.cleanup_report().is_some());
        assert!(output.sanitization_report().is_none());
        Ok(())
    }

    #[test]
    fn unsafe_scalars_use_sanitized_raw_fallback() -> Result<(), CleanupError> {
        let input = "  keep\0 visible \u{202e}text\u{fdd0}  ";
        let output = refine_without_model(input, CleanupConfig::default())?;

        assert_eq!(output.text(), "keep visible text");
        assert_eq!(output.path(), RefinementPath::SanitizedRaw);
        assert!(output.cleanup_report().is_none());
        assert_eq!(
            output.sanitization_report(),
            Some(SanitizationReport {
                input_bytes: input.len(),
                output_bytes: 17,
                whitespace_normalized: true,
                control_scalars_removed: 1,
                bidi_controls_removed: 1,
                noncharacters_removed: 1,
            })
        );
        Ok(())
    }

    #[test]
    fn sanitized_raw_preserves_visible_punctuation_and_case() -> Result<(), CleanupError> {
        let output =
            sanitize_raw_transcript("  hello  , WORLD \u{2066} now  ", CleanupConfig::default())?;

        assert_eq!(output.text(), "hello , WORLD now");
        assert_eq!(output.report().bidi_controls_removed, 1);
        assert!(output.report().changed());
        Ok(())
    }

    #[test]
    fn script_joiners_are_not_removed() -> Result<(), CleanupError> {
        let input = "नमस्\u{200d}ते اردو";
        let output = sanitize_raw_transcript(input, CleanupConfig::default())?;

        assert_eq!(output.text(), input);
        assert!(!output.report().changed());
        Ok(())
    }

    #[test]
    fn resource_failures_do_not_masquerade_as_fallback() -> Result<(), CleanupError> {
        let config = CleanupConfig::new(4)?;

        assert!(matches!(
            refine_without_model("12345", config),
            Err(CleanupError::InputTooLong)
        ));
        Ok(())
    }

    #[test]
    fn sanitized_output_never_exceeds_input() -> Result<(), CleanupError> {
        let input = " \0a  b\u{202e} c\u{fdd0} ";
        let output = sanitize_raw_transcript(input, CleanupConfig::default())?;

        assert!(output.report().output_bytes <= input.len());
        Ok(())
    }
}
