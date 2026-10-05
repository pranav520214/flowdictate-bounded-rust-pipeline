use std::{error::Error, fmt};

/// Maximum token count inspected by the deterministic semantic verifier.
pub const MAX_SEMANTIC_TOKENS: usize = 512;
/// Maximum protected-token count retained in one verification pass.
pub const MAX_PROTECTED_TOKENS: usize = 256;

/// Bounded structural policy for one local-editor candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticVerificationConfig {
    maximum_tokens: usize,
    maximum_added_tokens: usize,
}

impl SemanticVerificationConfig {
    /// Creates a bounded token policy.
    ///
    /// # Errors
    ///
    /// Rejects zero or excessive token limits.
    pub const fn new(
        maximum_tokens: usize,
        maximum_added_tokens: usize,
    ) -> Result<Self, SemanticVerificationError> {
        if maximum_tokens == 0
            || maximum_tokens > MAX_SEMANTIC_TOKENS
            || maximum_added_tokens > maximum_tokens
        {
            Err(SemanticVerificationError::InvalidConfig)
        } else {
            Ok(Self {
                maximum_tokens,
                maximum_added_tokens,
            })
        }
    }

    /// Returns the maximum source or candidate token count.
    #[must_use]
    pub const fn maximum_tokens(self) -> usize {
        self.maximum_tokens
    }

    /// Returns the maximum candidate token growth.
    #[must_use]
    pub const fn maximum_added_tokens(self) -> usize {
        self.maximum_added_tokens
    }
}

impl Default for SemanticVerificationConfig {
    fn default() -> Self {
        Self {
            maximum_tokens: MAX_SEMANTIC_TOKENS,
            maximum_added_tokens: 8,
        }
    }
}

/// Numeric-only evidence from one accepted semantic candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticVerificationReport {
    /// Source token count.
    pub source_tokens: usize,
    /// Candidate token count.
    pub candidate_tokens: usize,
    /// Protected source-token count checked for preservation.
    pub protected_tokens: usize,
    /// Candidate tokens beyond the source count.
    pub added_tokens: usize,
}

/// A candidate that passed deterministic semantic checks without owning text.
///
/// This type intentionally implements neither `Clone` nor `Debug`.
pub struct SemanticallyValidated<'a> {
    text: &'a str,
    report: SemanticVerificationReport,
}

impl<'a> SemanticallyValidated<'a> {
    /// Borrows the unchanged candidate.
    #[must_use]
    pub const fn text(&self) -> &'a str {
        self.text
    }

    /// Returns numeric-only verification evidence.
    #[must_use]
    pub const fn report(&self) -> SemanticVerificationReport {
        self.report
    }
}

/// Fixed, payload-free semantic-verification failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticVerificationError {
    /// The token policy is outside its compiled bound.
    InvalidConfig,
    /// Source or candidate has too many tokens.
    TooManyTokens,
    /// Meaningful source text was replaced by an empty candidate.
    EmptyCandidate,
    /// A protected token was deleted or changed.
    ProtectedTokenChanged,
    /// Candidate adds more tokens than the explicit policy permits.
    ExcessiveInsertion,
}

impl fmt::Display for SemanticVerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConfig => "semantic verification configuration is invalid",
            Self::TooManyTokens => "semantic verification token limit exceeded",
            Self::EmptyCandidate => "semantic candidate is empty",
            Self::ProtectedTokenChanged => "semantic candidate changes a protected token",
            Self::ExcessiveInsertion => "semantic candidate adds too many tokens",
        })
    }
}

impl Error for SemanticVerificationError {}

/// Verifies a local-editor candidate without interpreting it as instructions.
///
/// The verifier checks bounded token growth and preserves numbers, dates,
/// URLs, email-like values, paths, flags, technical identifiers, quoted values,
/// and title-cased tokens. It performs no normalization, model call, logging,
/// persistence, context access, or network operation. The candidate remains
/// borrowed and is never copied.
///
/// # Errors
///
/// Returns a fixed category when a bound is exceeded, meaningful text is
/// erased, a protected token changes, or token insertion is excessive.
pub fn verify_semantic_candidate<'a>(
    source: &str,
    candidate: &'a str,
    config: SemanticVerificationConfig,
) -> Result<SemanticallyValidated<'a>, SemanticVerificationError> {
    let (source_tokens, source_protected) = collect_protected(source, config)?;
    let (candidate_tokens, _) = collect_protected(candidate, config)?;
    if !source.is_empty() && candidate_tokens == 0 {
        return Err(SemanticVerificationError::EmptyCandidate);
    }
    let added_tokens = candidate_tokens.saturating_sub(source_tokens);
    if added_tokens > config.maximum_added_tokens {
        return Err(SemanticVerificationError::ExcessiveInsertion);
    }

    for source_token in source_protected
        .items
        .iter()
        .take(source_protected.len)
        .flatten()
    {
        if count_token(source_token, source) != count_token(source_token, candidate) {
            return Err(SemanticVerificationError::ProtectedTokenChanged);
        }
    }

    Ok(SemanticallyValidated {
        text: candidate,
        report: SemanticVerificationReport {
            source_tokens,
            candidate_tokens,
            protected_tokens: source_protected.len,
            added_tokens,
        },
    })
}

struct ProtectedTokens<'a> {
    items: [Option<&'a str>; MAX_PROTECTED_TOKENS],
    len: usize,
}

fn collect_protected(
    text: &str,
    config: SemanticVerificationConfig,
) -> Result<(usize, ProtectedTokens<'_>), SemanticVerificationError> {
    let mut tokens = ProtectedTokens {
        items: [None; MAX_PROTECTED_TOKENS],
        len: 0,
    };
    let mut token_count = 0;
    for (index, raw) in text.split_whitespace().enumerate() {
        token_count = index.saturating_add(1);
        if token_count > config.maximum_tokens {
            return Err(SemanticVerificationError::TooManyTokens);
        }
        if is_protected(raw) {
            if tokens.len == MAX_PROTECTED_TOKENS {
                return Err(SemanticVerificationError::TooManyTokens);
            }
            tokens.items[tokens.len] = Some(normalize_token(raw));
            tokens.len += 1;
        }
    }
    Ok((token_count, tokens))
}

fn count_token(source_token: &str, text: &str) -> usize {
    text.split_whitespace()
        .map(normalize_token)
        .filter(|candidate_token| *candidate_token == source_token)
        .count()
}

fn normalize_token(token: &str) -> &str {
    token.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\''
                | '`'
                | ','
                | '.'
                | '!'
                | '?'
                | ';'
                | ':'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
        )
    })
}

fn is_protected(token: &str) -> bool {
    let normalized = normalize_token(token);
    if normalized.is_empty() {
        return false;
    }
    normalized.contains("http://")
        || normalized.contains("https://")
        || (normalized.contains('@') && normalized.contains('.'))
        || normalized.contains('/')
        || normalized.contains('\\')
        || normalized.contains('_')
        || normalized.starts_with("--")
        || normalized.contains('=')
        || normalized.contains('$')
        || normalized.contains('%')
        || normalized.chars().any(char::is_numeric)
        || normalized.chars().next().is_some_and(char::is_uppercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_numbers_urls_emails_paths_flags_and_identifiers(
    ) -> Result<(), SemanticVerificationError> {
        let source = "Deploy v2 on 2026-09-04 at https://example.com user@example.com C:\\Work\\app --dry-run snake_case";
        let candidate = "Deploy v2 on Friday 2026-09-04 at https://example.com user@example.com C:\\Work\\app --dry-run snake_case";
        let verified =
            verify_semantic_candidate(source, candidate, SemanticVerificationConfig::default())?;

        assert_eq!(verified.text(), candidate);
        assert!(verified.report().protected_tokens >= 8);
        Ok(())
    }

    #[test]
    fn rejects_number_url_email_name_and_identifier_mutation() {
        let cases = [
            ("ship v2", "ship v3"),
            ("visit https://example.com", "visit https://evil.example"),
            ("email user@example.com", "email other@example.com"),
            ("Alice met Bob", "Alice met Carol"),
            ("run snake_case", "run snake-case"),
        ];
        for (source, candidate) in cases {
            assert!(matches!(
                verify_semantic_candidate(source, candidate, SemanticVerificationConfig::default()),
                Err(SemanticVerificationError::ProtectedTokenChanged)
            ));
        }
    }

    #[test]
    fn allows_cleanup_and_small_semantic_rewrite_without_copying_candidate(
    ) -> Result<(), SemanticVerificationError> {
        let source = "deploy tomorrow actually no friday morning";
        let candidate = "Deploy Friday morning.";
        let verified =
            verify_semantic_candidate(source, candidate, SemanticVerificationConfig::default())?;

        assert_eq!(verified.text().as_ptr(), candidate.as_ptr());
        Ok(())
    }

    #[test]
    fn rejects_empty_and_excessive_candidates_without_payload(
    ) -> Result<(), SemanticVerificationError> {
        let config = SemanticVerificationConfig::new(16, 1)?;
        assert!(matches!(
            verify_semantic_candidate("meaningful source", "", config),
            Err(SemanticVerificationError::EmptyCandidate)
        ));
        assert!(matches!(
            verify_semantic_candidate("one", "one two three", config),
            Err(SemanticVerificationError::ExcessiveInsertion)
        ));
        assert!(!SemanticVerificationError::ProtectedTokenChanged
            .to_string()
            .contains("meaningful"));
        Ok(())
    }

    #[test]
    fn bounds_are_fixed_and_payload_free() {
        assert_eq!(
            SemanticVerificationConfig::new(0, 0),
            Err(SemanticVerificationError::InvalidConfig)
        );
        assert_eq!(
            SemanticVerificationConfig::new(MAX_SEMANTIC_TOKENS + 1, 0),
            Err(SemanticVerificationError::InvalidConfig)
        );
    }
}
