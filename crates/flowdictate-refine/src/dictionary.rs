use std::{error::Error, fmt, str};

use crate::{
    validation::{is_bidi_control, is_noncharacter},
    CleanupConfig,
};

/// Maximum explicit entries accepted by one dictionary pass.
pub const MAX_DICTIONARY_ENTRIES: usize = 256;
/// Maximum UTF-8 bytes in one spoken or written dictionary field.
pub const MAX_DICTIONARY_FIELD_BYTES: usize = 256;

/// One borrowed, explicitly entered exact-token or phrase replacement.
///
/// This type intentionally omits `Debug` so dictionary content is not
/// accidentally formatted into logs or errors.
#[derive(Clone, Copy)]
pub struct DictionaryEntry<'a> {
    spoken: &'a str,
    written: &'a str,
}

impl<'a> DictionaryEntry<'a> {
    /// Validates one case-sensitive token and its visible replacement.
    ///
    /// Spoken forms contain one or more tokens separated by one ASCII space
    /// under the compiled punctuation delimiters. Matching is case-sensitive;
    /// case folding is excluded until its language semantics are versioned.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversized, unsafe, or multi-token fields without
    /// including their content in the error.
    pub fn new(spoken: &'a str, written: &'a str) -> Result<Self, DictionaryError> {
        if spoken.is_empty()
            || written.is_empty()
            || spoken.len() > MAX_DICTIONARY_FIELD_BYTES
            || written.len() > MAX_DICTIONARY_FIELD_BYTES
            || !is_valid_spoken(spoken)
            || !written.chars().all(is_safe_written_character)
        {
            return Err(DictionaryError::InvalidEntry);
        }
        Ok(Self { spoken, written })
    }

    /// Borrows the exact case-sensitive spoken token or phrase.
    #[must_use]
    pub const fn spoken(&self) -> &'a str {
        self.spoken
    }

    /// Borrows the exact user-preferred written form.
    #[must_use]
    pub const fn written(&self) -> &'a str {
        self.written
    }
}

/// Numeric-only measurements from one dictionary pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DictionaryReport {
    /// Borrowed transcript bytes inspected.
    pub input_bytes: usize,
    /// Output bytes retained by the result owner.
    pub output_bytes: usize,
    /// Explicit dictionary entries considered.
    pub entries: usize,
    /// Whole-token replacements applied.
    pub replacements: usize,
}

/// Opaque dictionary-substituted transcript, overwritten on drop.
///
/// This type intentionally implements neither `Clone` nor `Debug`.
pub struct DictionaryTranscript {
    bytes: Vec<u8>,
    report: DictionaryReport,
}

impl DictionaryTranscript {
    /// Borrows the substituted text.
    #[must_use]
    pub fn text(&self) -> &str {
        str::from_utf8(&self.bytes).unwrap_or_default()
    }

    /// Returns numeric-only replacement metadata.
    #[must_use]
    pub const fn report(&self) -> DictionaryReport {
        self.report
    }
}

impl Drop for DictionaryTranscript {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}

/// Applies explicit case-sensitive whole-token or phrase replacements.
///
/// Entries are borrowed from a caller-controlled local store and are never
/// retained, logged, serialized, learned, or transmitted. Punctuation and
/// whitespace outside matched tokens are copied exactly. An empty entry slice
/// is the personalization-off behavior and copies the bounded input unchanged.
///
/// # Errors
///
/// Rejects excessive input/output, too many or conflicting entries, invalid
/// fields, and allocation failure using payload-free categories.
pub fn apply_user_dictionary(
    input: &str,
    entries: &[DictionaryEntry<'_>],
    config: CleanupConfig,
) -> Result<DictionaryTranscript, DictionaryError> {
    if input.len() > config.maximum_bytes() {
        return Err(DictionaryError::InputTooLong);
    }
    validate_entries(entries)?;
    let (output_bytes, replacements) = measure_output(input, entries, config.maximum_bytes())?;
    let mut output = DictionaryTranscript {
        bytes: Vec::new(),
        report: DictionaryReport {
            input_bytes: input.len(),
            output_bytes,
            entries: entries.len(),
            replacements,
        },
    };
    output
        .bytes
        .try_reserve_exact(output_bytes)
        .map_err(|_| DictionaryError::AllocationFailed)?;
    write_output(input, entries, &mut output.bytes);
    Ok(output)
}

fn validate_entries(entries: &[DictionaryEntry<'_>]) -> Result<(), DictionaryError> {
    if entries.len() > MAX_DICTIONARY_ENTRIES {
        return Err(DictionaryError::TooManyEntries);
    }
    for (index, entry) in entries.iter().enumerate() {
        if DictionaryEntry::new(entry.spoken, entry.written).is_err() {
            return Err(DictionaryError::InvalidEntry);
        }
        if entries[..index]
            .iter()
            .any(|previous| previous.spoken == entry.spoken)
        {
            return Err(DictionaryError::ConflictingEntry);
        }
    }
    Ok(())
}

fn measure_output(
    input: &str,
    entries: &[DictionaryEntry<'_>],
    maximum_bytes: usize,
) -> Result<(usize, usize), DictionaryError> {
    let mut output_bytes = input.len();
    let mut replacements = 0usize;
    let mut matched_through = 0usize;
    for (start, _, _) in tokens(input) {
        if start < matched_through {
            continue;
        }
        if let Some((entry, end)) = find_entry_at(entries, input, start) {
            let matched_bytes = end.saturating_sub(start);
            output_bytes = output_bytes
                .checked_sub(matched_bytes)
                .and_then(|length| length.checked_add(entry.written.len()))
                .ok_or(DictionaryError::OutputTooLong)?;
            if output_bytes > maximum_bytes {
                return Err(DictionaryError::OutputTooLong);
            }
            replacements = replacements.saturating_add(1);
            matched_through = end;
        }
    }
    Ok((output_bytes, replacements))
}

fn write_output(input: &str, entries: &[DictionaryEntry<'_>], output: &mut Vec<u8>) {
    let mut copied_through = 0usize;
    for (start, _, _) in tokens(input) {
        if start < copied_through {
            continue;
        }
        let Some((entry, end)) = find_entry_at(entries, input, start) else {
            continue;
        };
        output.extend_from_slice(&input.as_bytes()[copied_through..start]);
        output.extend_from_slice(entry.written.as_bytes());
        copied_through = end;
    }
    output.extend_from_slice(&input.as_bytes()[copied_through..]);
}

fn tokens(input: &str) -> impl Iterator<Item = (usize, usize, &str)> {
    let mut start = None;
    input
        .char_indices()
        .chain(std::iter::once((input.len(), ' ')))
        .filter_map(move |(index, character)| {
            if is_token_character(character) {
                start.get_or_insert(index);
                None
            } else {
                start
                    .take()
                    .map(|token_start| (token_start, index, &input[token_start..index]))
            }
        })
}

fn find_entry_at<'a>(
    entries: &'a [DictionaryEntry<'a>],
    input: &str,
    start: usize,
) -> Option<(&'a DictionaryEntry<'a>, usize)> {
    let tail = input.get(start..)?;
    entries
        .iter()
        .filter_map(|entry| {
            let remaining = tail.strip_prefix(entry.spoken)?;
            let valid_end = remaining
                .chars()
                .next()
                .is_none_or(|character| !is_token_character(character));
            valid_end.then_some((entry, start.saturating_add(entry.spoken.len())))
        })
        .max_by_key(|(entry, _)| entry.spoken.len())
}

fn is_valid_spoken(spoken: &str) -> bool {
    let mut previous_space = false;
    for character in spoken.chars() {
        if character == ' ' {
            if previous_space {
                return false;
            }
            previous_space = true;
        } else if !is_token_character(character) || !is_safe_written_character(character) {
            return false;
        } else {
            previous_space = false;
        }
    }
    !spoken.starts_with(' ') && !previous_space
}

fn is_token_character(character: char) -> bool {
    !character.is_whitespace() && !is_token_separator(character)
}

fn is_token_separator(character: char) -> bool {
    (character.is_ascii_punctuation() && character != '_')
        || matches!(
            character,
            '।' | '॥'
                | '،'
                | '؛'
                | '؟'
                | '。'
                | '、'
                | '！'
                | '？'
                | '…'
                | '–'
                | '—'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '«'
                | '»'
                | '（'
                | '）'
                | '【'
                | '】'
        )
}

fn is_safe_written_character(character: char) -> bool {
    !character.is_control() && !is_bidi_control(character) && !is_noncharacter(character)
}

/// Payload-free dictionary failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DictionaryError {
    /// More than the compiled maximum entry count was supplied.
    TooManyEntries,
    /// A spoken or written field violated its size or character policy.
    InvalidEntry,
    /// Two entries use the same exact spoken token or phrase.
    ConflictingEntry,
    /// The borrowed transcript exceeds its configured byte ceiling.
    InputTooLong,
    /// Replacement would exceed the configured byte ceiling.
    OutputTooLong,
    /// Bounded output storage could not be reserved.
    AllocationFailed,
}

impl fmt::Display for DictionaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TooManyEntries => "dictionary contains too many entries",
            Self::InvalidEntry => "dictionary entry is invalid",
            Self::ConflictingEntry => "dictionary contains a conflicting entry",
            Self::InputTooLong => "dictionary input exceeds its bound",
            Self::OutputTooLong => "dictionary output exceeds its bound",
            Self::AllocationFailed => "dictionary output allocation failed",
        })
    }
}

impl Error for DictionaryError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refine_without_model;

    #[test]
    fn exact_phrases_replace_longest_without_touching_substrings_or_case(
    ) -> Result<(), DictionaryError> {
        let entries = [
            DictionaryEntry::new("open", "OPEN")?,
            DictionaryEntry::new("open ai", "OpenAI")?,
        ];
        let output = apply_user_dictionary(
            "open ai, openaix and Open AI",
            &entries,
            CleanupConfig::default(),
        )?;

        assert_eq!(output.text(), "OpenAI, openaix and Open AI");
        assert_eq!(output.report().replacements, 1);
        Ok(())
    }

    #[test]
    fn unicode_names_and_programming_identifiers_are_supported() -> Result<(), DictionaryError> {
        let entries = [
            DictionaryEntry::new("प्रणव", "प्रणव")?,
            DictionaryEntry::new("flow_dictate", "FlowDictate")?,
        ];
        let output = apply_user_dictionary(
            "प्रणव। uses flow_dictate.",
            &entries,
            CleanupConfig::default(),
        )?;

        assert_eq!(output.text(), "प्रणव। uses FlowDictate.");
        assert_eq!(output.report().replacements, 2);
        Ok(())
    }

    #[test]
    fn substituted_text_composes_before_cleanup() -> Result<(), Box<dyn Error>> {
        let entries = [DictionaryEntry::new("cpp", "C++")?];
        let substituted =
            apply_user_dictionary("  cpp   is useful . ", &entries, CleanupConfig::default())?;
        let refined = refine_without_model(substituted.text(), CleanupConfig::default())?;

        assert_eq!(refined.text(), "C++ is useful.");
        Ok(())
    }

    #[test]
    fn invalid_conflicting_and_expanding_entries_fail_closed() -> Result<(), DictionaryError> {
        assert!(matches!(
            DictionaryEntry::new("two  words", "value"),
            Err(DictionaryError::InvalidEntry)
        ));
        let marker = "private-marker";
        let entries = [
            DictionaryEntry::new("x", "first")?,
            DictionaryEntry::new("x", marker)?,
        ];
        let error = apply_user_dictionary("x", &entries, CleanupConfig::default())
            .err()
            .ok_or(DictionaryError::InvalidEntry)?;
        assert_eq!(error, DictionaryError::ConflictingEntry);
        assert!(!error.to_string().contains(marker));

        let expanding = [DictionaryEntry::new("x", "expanded")?];
        let small = CleanupConfig::new(4).map_err(|_| DictionaryError::InvalidEntry)?;
        assert!(matches!(
            apply_user_dictionary("x", &expanding, small),
            Err(DictionaryError::OutputTooLong)
        ));
        Ok(())
    }
}
