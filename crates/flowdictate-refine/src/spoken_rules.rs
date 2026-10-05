use std::str;

use crate::{
    validation::{is_bidi_control, is_noncharacter},
    CleanupConfig, CleanupError,
};

/// Explicit spoken-rule behavior for deterministic refinement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SpokenRulesPolicy {
    /// Preserve filler repetitions and formatting phrases as literal text.
    #[default]
    Literal,
    /// Version-one English rules for repeated fillers and formatting commands.
    EnglishV1,
}

/// Numeric-only measurements from one spoken-rule pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SpokenRulesReport {
    /// Borrowed bytes inspected.
    pub input_bytes: usize,
    /// Output bytes retained by the result owner.
    pub output_bytes: usize,
    /// Consecutive repeated filler tokens collapsed.
    pub repeated_fillers_collapsed: usize,
    /// Explicit formatting phrases interpreted.
    pub formatting_commands: usize,
    /// LF line-break bytes emitted by formatting commands.
    pub line_breaks_emitted: usize,
    /// Bullet markers emitted by formatting commands.
    pub bullets_emitted: usize,
}

impl SpokenRulesReport {
    /// Reports whether the policy changed the input.
    #[must_use]
    pub const fn changed(self) -> bool {
        self.repeated_fillers_collapsed != 0 || self.formatting_commands != 0
    }
}

/// Opaque spoken-rule output whose initialized bytes are wiped on drop.
///
/// This type intentionally implements neither `Clone` nor `Debug`.
pub struct SpokenRulesTranscript {
    bytes: Vec<u8>,
    report: SpokenRulesReport,
}

impl SpokenRulesTranscript {
    /// Borrows the transformed text.
    #[must_use]
    pub fn text(&self) -> &str {
        str::from_utf8(&self.bytes).unwrap_or_default()
    }

    /// Returns numeric-only transformation metadata.
    #[must_use]
    pub const fn report(&self) -> SpokenRulesReport {
        self.report
    }
}

impl Drop for SpokenRulesTranscript {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}

/// Applies an explicit deterministic spoken-rule policy.
///
/// `Literal` is the default and performs no interpretation. `EnglishV1`
/// collapses only consecutive repeats of `uh`, `um`, `erm`, or `hmm`; it never
/// removes the first filler or repetitions such as `very very`. It recognizes
/// only `new line`, `new paragraph`, and `bullet point`, case-insensitively at
/// whole-word boundaries. These phrases remain inert text unless the caller
/// explicitly selects `EnglishV1`.
///
/// # Errors
///
/// Rejects excessive or unsafe input and allocation failure using the existing
/// payload-free cleanup categories.
pub fn apply_spoken_rules(
    input: &str,
    policy: SpokenRulesPolicy,
    config: CleanupConfig,
) -> Result<SpokenRulesTranscript, CleanupError> {
    validate_input(input, config)?;
    let mut report = SpokenRulesReport {
        input_bytes: input.len(),
        ..SpokenRulesReport::default()
    };
    let mut output = bounded_bytes(input.len())?;
    match policy {
        SpokenRulesPolicy::Literal => output.extend_from_slice(input.as_bytes()),
        SpokenRulesPolicy::EnglishV1 => {
            let mut fillers = SensitiveBytes::new(input.len())?;
            collapse_repeated_fillers(input, &mut fillers.bytes, &mut report);
            apply_formatting(fillers.text(), &mut output, &mut report);
        }
    }
    report.output_bytes = output.len();
    Ok(SpokenRulesTranscript {
        bytes: output,
        report,
    })
}

fn validate_input(input: &str, config: CleanupConfig) -> Result<(), CleanupError> {
    if input.len() > config.maximum_bytes() {
        return Err(CleanupError::InputTooLong);
    }
    for character in input.chars() {
        if character.is_control() {
            return Err(CleanupError::InvalidControlCharacter);
        }
        if is_bidi_control(character) {
            return Err(CleanupError::InvalidBidiControl);
        }
        if is_noncharacter(character) {
            return Err(CleanupError::InvalidNoncharacter);
        }
    }
    Ok(())
}

fn bounded_bytes(capacity: usize) -> Result<Vec<u8>, CleanupError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| CleanupError::AllocationFailed)?;
    Ok(bytes)
}

struct SensitiveBytes {
    bytes: Vec<u8>,
}

impl SensitiveBytes {
    fn new(capacity: usize) -> Result<Self, CleanupError> {
        Ok(Self {
            bytes: bounded_bytes(capacity)?,
        })
    }

    fn text(&self) -> &str {
        str::from_utf8(&self.bytes).unwrap_or_default()
    }
}

impl Drop for SensitiveBytes {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}

fn collapse_repeated_fillers(input: &str, output: &mut Vec<u8>, report: &mut SpokenRulesReport) {
    let mut copied_through = 0usize;
    let mut previous_filler = None;
    for (_, end, token) in whitespace_tokens(input) {
        let filler = filler_key(token);
        if filler.is_some() && filler == previous_filler {
            copied_through = end;
            report.repeated_fillers_collapsed = report.repeated_fillers_collapsed.saturating_add(1);
            continue;
        }
        output.extend_from_slice(&input.as_bytes()[copied_through..end]);
        copied_through = end;
        previous_filler = filler;
    }
    output.extend_from_slice(&input.as_bytes()[copied_through..]);
}

fn whitespace_tokens(input: &str) -> impl Iterator<Item = (usize, usize, &str)> {
    let mut start = None;
    input
        .char_indices()
        .chain(std::iter::once((input.len(), ' ')))
        .filter_map(move |(index, character)| {
            if character.is_whitespace() {
                start
                    .take()
                    .map(|token_start| (token_start, index, &input[token_start..index]))
            } else {
                start.get_or_insert(index);
                None
            }
        })
}

fn filler_key(token: &str) -> Option<&'static str> {
    ["uh", "um", "erm", "hmm"]
        .into_iter()
        .find(|filler| token.eq_ignore_ascii_case(filler))
}

fn apply_formatting(input: &str, output: &mut Vec<u8>, report: &mut SpokenRulesReport) {
    let mut index = 0usize;
    let mut capitalize_next = false;
    while index < input.len() {
        if let Some((length, command)) = formatting_at(input, index) {
            trim_ascii_spaces(output);
            command.write(output, report);
            report.formatting_commands = report.formatting_commands.saturating_add(1);
            index = index.saturating_add(length);
            while input.as_bytes().get(index) == Some(&b' ') {
                index = index.saturating_add(1);
            }
            capitalize_next = true;
            continue;
        }

        let Some(character) = input.get(index..).and_then(|tail| tail.chars().next()) else {
            break;
        };
        let emitted = if capitalize_next && character.is_ascii_lowercase() {
            capitalize_next = false;
            character.to_ascii_uppercase()
        } else {
            if !character.is_whitespace() {
                capitalize_next = false;
            }
            character
        };
        let mut encoded = [0_u8; 4];
        output.extend_from_slice(emitted.encode_utf8(&mut encoded).as_bytes());
        index = index.saturating_add(character.len_utf8());
    }
}

#[derive(Clone, Copy)]
enum FormattingCommand {
    NewParagraph,
    BulletPoint,
    NewLine,
}

impl FormattingCommand {
    fn write(self, output: &mut Vec<u8>, report: &mut SpokenRulesReport) {
        match self {
            Self::NewParagraph => {
                output.extend_from_slice(b"\n\n");
                report.line_breaks_emitted = report.line_breaks_emitted.saturating_add(2);
            }
            Self::BulletPoint => {
                if !output.is_empty() && output.last() != Some(&b'\n') {
                    output.push(b'\n');
                    report.line_breaks_emitted = report.line_breaks_emitted.saturating_add(1);
                }
                output.extend_from_slice(b"- ");
                report.bullets_emitted = report.bullets_emitted.saturating_add(1);
            }
            Self::NewLine => {
                output.push(b'\n');
                report.line_breaks_emitted = report.line_breaks_emitted.saturating_add(1);
            }
        }
    }
}

fn formatting_at(input: &str, index: usize) -> Option<(usize, FormattingCommand)> {
    let previous_is_word = input
        .get(..index)
        .and_then(|prefix| prefix.chars().next_back())
        .is_some_and(is_ascii_word_character);
    if previous_is_word {
        return None;
    }
    [
        ("new paragraph", FormattingCommand::NewParagraph),
        ("bullet point", FormattingCommand::BulletPoint),
        ("new line", FormattingCommand::NewLine),
    ]
    .into_iter()
    .find_map(|(phrase, command)| {
        let end = index.checked_add(phrase.len())?;
        let candidate = input.get(index..end)?;
        let next_is_word = input
            .get(end..)
            .and_then(|tail| tail.chars().next())
            .is_some_and(is_ascii_word_character);
        (candidate.eq_ignore_ascii_case(phrase) && !next_is_word).then_some((phrase.len(), command))
    })
}

fn is_ascii_word_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn trim_ascii_spaces(bytes: &mut Vec<u8>) {
    while bytes.last() == Some(&b' ') {
        let Some(last) = bytes.last_mut() else {
            return;
        };
        *last = 0;
        bytes.truncate(bytes.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_policy_never_interprets_dictated_phrases() -> Result<(), CleanupError> {
        let input = "um um new line bullet point";
        let output =
            apply_spoken_rules(input, SpokenRulesPolicy::Literal, CleanupConfig::default())?;

        assert_eq!(output.text(), input);
        assert!(!output.report().changed());
        Ok(())
    }

    #[test]
    fn english_v1_collapses_only_repeated_fillers_and_formats_lines() -> Result<(), CleanupError> {
        let output = apply_spoken_rules(
            "um UM we continue new line next bullet point final",
            SpokenRulesPolicy::EnglishV1,
            CleanupConfig::default(),
        )?;

        assert_eq!(output.text(), "um we continue\nNext\n- Final");
        assert_eq!(output.report().repeated_fillers_collapsed, 1);
        assert_eq!(output.report().formatting_commands, 2);
        assert_eq!(output.report().line_breaks_emitted, 2);
        assert_eq!(output.report().bullets_emitted, 1);
        Ok(())
    }

    #[test]
    fn intentional_repetition_single_fillers_and_other_languages_are_preserved(
    ) -> Result<(), CleanupError> {
        let input = "very very um नमस्ते नमस्ते";
        let output = apply_spoken_rules(
            input,
            SpokenRulesPolicy::EnglishV1,
            CleanupConfig::default(),
        )?;

        assert_eq!(output.text(), input);
        assert!(!output.report().changed());
        Ok(())
    }

    #[test]
    fn unsafe_input_errors_never_echo_payload() -> Result<(), CleanupError> {
        let marker = "private-marker\0";
        let error = apply_spoken_rules(
            marker,
            SpokenRulesPolicy::EnglishV1,
            CleanupConfig::default(),
        )
        .err()
        .ok_or(CleanupError::InvalidConfig)?;

        assert_eq!(error, CleanupError::InvalidControlCharacter);
        assert!(!error.to_string().contains(marker));
        Ok(())
    }
}
