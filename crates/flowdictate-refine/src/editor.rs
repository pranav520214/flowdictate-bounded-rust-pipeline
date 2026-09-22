use std::{error::Error, fmt, str};

use crate::{
    refine_without_model, select_refinement_route, validate_refined_output,
    verify_semantic_candidate, CleanupConfig, CleanupError, LocalEditorGate,
    OutputValidationConfig, OutputValidationError, RefinementOutput, RefinementRoute,
    RefinementSignals, SemanticVerificationConfig, SemanticVerificationError, MAX_CLEANUP_BYTES,
};

/// Fixed, payload-free failures from a local editor runtime or bounded sink.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalEditorError {
    /// The selected local runtime is not available.
    Unavailable,
    /// The bounded local request exceeded its deadline.
    Timeout,
    /// The runtime could not reserve required memory.
    OutOfMemory,
    /// The runtime exhausted its configured context.
    ContextExhausted,
    /// Local decoding failed.
    DecoderFailed,
    /// The runtime attempted to exceed the output byte ceiling.
    OutputTooLong,
    /// Bounded output storage could not be reserved.
    AllocationFailed,
}

impl fmt::Display for LocalEditorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "local editor is unavailable",
            Self::Timeout => "local editor timed out",
            Self::OutOfMemory => "local editor ran out of memory",
            Self::ContextExhausted => "local editor context is exhausted",
            Self::DecoderFailed => "local editor decoding failed",
            Self::OutputTooLong => "local editor output exceeds its bound",
            Self::AllocationFailed => "local editor output allocation failed",
        })
    }
}

impl Error for LocalEditorError {}

/// Bounded, wipe-on-drop destination for one local editor attempt.
///
/// A runtime receives a fresh empty sink and can append UTF-8 only through
/// [`Self::write_str`]. This type intentionally implements neither `Clone` nor
/// `Debug`.
pub struct LocalEditorOutput {
    bytes: Vec<u8>,
    maximum_bytes: usize,
}

impl LocalEditorOutput {
    fn new(maximum_bytes: usize) -> Self {
        debug_assert!(maximum_bytes > 0 && maximum_bytes <= MAX_CLEANUP_BYTES);
        Self {
            bytes: Vec::new(),
            maximum_bytes,
        }
    }

    /// Appends one candidate fragment without crossing the configured bound.
    ///
    /// # Errors
    ///
    /// Returns a fixed category for excessive output or allocation failure.
    pub fn write_str(&mut self, fragment: &str) -> Result<(), LocalEditorError> {
        let next_len = self
            .bytes
            .len()
            .checked_add(fragment.len())
            .ok_or(LocalEditorError::OutputTooLong)?;
        if next_len > self.maximum_bytes {
            return Err(LocalEditorError::OutputTooLong);
        }
        self.bytes
            .try_reserve_exact(fragment.len())
            .map_err(|_| LocalEditorError::AllocationFailed)?;
        self.bytes.extend_from_slice(fragment.as_bytes());
        Ok(())
    }

    /// Overwrites and empties any candidate written so far.
    pub fn clear(&mut self) {
        self.bytes.fill(0);
        self.bytes.clear();
    }

    /// Borrows the candidate accumulated by the local runtime.
    #[must_use]
    pub fn text(&self) -> &str {
        str::from_utf8(&self.bytes).unwrap_or_default()
    }
}

impl Drop for LocalEditorOutput {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}

/// Narrow synchronous capability implemented by an audited local-only runtime.
///
/// Implementations must not persist or transmit `source`, must honor their
/// bounded deadline/context policy, and must write candidate text only through
/// `output`. Runtime/process supervision remains outside this dependency-free
/// crate.
pub trait LocalEditor {
    /// Attempts one bounded local rewrite.
    ///
    /// # Errors
    ///
    /// Returns a fixed category without source or candidate content.
    fn refine(
        &mut self,
        source: &str,
        output: &mut LocalEditorOutput,
    ) -> Result<(), LocalEditorError>;
}

/// Outcome of the optional local-editor attempt without transcript content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalEditorAttempt {
    /// Metadata or policy selected the model-free route.
    NotAttempted,
    /// The local candidate passed the explicit output policy.
    Accepted,
    /// The runtime failed and the model-free fallback was used.
    Failed(LocalEditorError),
    /// A non-empty source produced an empty candidate, so fallback was used.
    EmptyOutput,
    /// The candidate failed validation and the model-free fallback was used.
    Rejected(OutputValidationError),
    /// The candidate failed deterministic semantic verification and fallback was used.
    RejectedSemantic(SemanticVerificationError),
}

/// Final path that produced one optional-refinement result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionalRefinementPath {
    /// No local-editor attempt was justified or permitted.
    ModelFree,
    /// A bounded local-editor candidate was accepted.
    LocalEditor,
    /// A local-editor attempt failed closed to the model-free path.
    LocalEditorFallback,
}

/// Payload-free route and fallback evidence for one successful result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OptionalRefinementReport {
    route: RefinementRoute,
    path: OptionalRefinementPath,
    attempt: LocalEditorAttempt,
}

impl OptionalRefinementReport {
    /// Returns the metadata-selected route.
    #[must_use]
    pub const fn route(self) -> RefinementRoute {
        self.route
    }

    /// Returns the path that produced the accepted text.
    #[must_use]
    pub const fn path(self) -> OptionalRefinementPath {
        self.path
    }

    /// Returns the content-free local attempt outcome.
    #[must_use]
    pub const fn attempt(self) -> LocalEditorAttempt {
        self.attempt
    }
}

enum OptionalRefinementOwner {
    Local(LocalEditorOutput),
    ModelFree(RefinementOutput),
}

/// Opaque accepted local-editor or model-free result.
///
/// This type intentionally implements neither `Clone` nor `Debug`. Its owned
/// text is wiped by the selected inner owner on drop.
pub struct OptionalRefinementOutput {
    owner: OptionalRefinementOwner,
    report: OptionalRefinementReport,
}

impl OptionalRefinementOutput {
    /// Borrows the final accepted text.
    #[must_use]
    pub fn text(&self) -> &str {
        match &self.owner {
            OptionalRefinementOwner::Local(output) => output.text(),
            OptionalRefinementOwner::ModelFree(output) => output.text(),
        }
    }

    /// Returns route and fallback metadata without transcript content.
    #[must_use]
    pub const fn report(&self) -> OptionalRefinementReport {
        self.report
    }
}

/// Fixed, payload-free failure after the complete fallback hierarchy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionalRefinementError {
    /// Deterministic cleanup and sanitized-raw fallback could not finish.
    Cleanup(CleanupError),
    /// The model-free result unexpectedly failed its strict output policy.
    Validation(OutputValidationError),
}

impl fmt::Display for OptionalRefinementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cleanup(_) => "model-free refinement fallback failed",
            Self::Validation(_) => "model-free refinement fallback validation failed",
        })
    }
}

impl Error for OptionalRefinementError {}

/// Runs the metadata-selected local editor and complete offline fallback chain.
///
/// A local candidate is accepted only after explicit output validation. Runtime
/// failure, empty output, or rejected output destroys that candidate and falls
/// back to deterministic cleanup, which itself falls back to sanitized raw
/// text for unsafe input scalars. No cloud or persistence path exists here.
///
/// # Errors
///
/// Returns a fixed category only when the model-free fallback cannot produce
/// and validate a bounded result.
pub fn refine_with_optional_editor<E: LocalEditor>(
    source: &str,
    signals: RefinementSignals,
    gate: LocalEditorGate,
    editor: &mut E,
    cleanup: CleanupConfig,
    editor_validation: OutputValidationConfig,
) -> Result<OptionalRefinementOutput, OptionalRefinementError> {
    let route = select_refinement_route(signals, gate);
    if route == RefinementRoute::LocalEditorThenModelFree {
        if source.len() > editor_validation.maximum_bytes() {
            return model_free_result(
                source,
                cleanup,
                route,
                LocalEditorAttempt::Rejected(OutputValidationError::SourceTooLong),
            );
        }

        let mut candidate = LocalEditorOutput::new(editor_validation.maximum_bytes());
        match editor.refine(source, &mut candidate) {
            Ok(()) if !source.is_empty() && candidate.text().is_empty() => {
                drop(candidate);
                return model_free_result(source, cleanup, route, LocalEditorAttempt::EmptyOutput);
            }
            Ok(()) => match validate_refined_output(source, candidate.text(), editor_validation) {
                Ok(_) => match verify_semantic_candidate(
                    source,
                    candidate.text(),
                    SemanticVerificationConfig::default(),
                ) {
                    Ok(_) => {
                        return Ok(OptionalRefinementOutput {
                            owner: OptionalRefinementOwner::Local(candidate),
                            report: OptionalRefinementReport {
                                route,
                                path: OptionalRefinementPath::LocalEditor,
                                attempt: LocalEditorAttempt::Accepted,
                            },
                        });
                    }
                    Err(error) => {
                        drop(candidate);
                        return model_free_result(
                            source,
                            cleanup,
                            route,
                            LocalEditorAttempt::RejectedSemantic(error),
                        );
                    }
                },
                Err(error) => {
                    drop(candidate);
                    return model_free_result(
                        source,
                        cleanup,
                        route,
                        LocalEditorAttempt::Rejected(error),
                    );
                }
            },
            Err(error) => {
                drop(candidate);
                return model_free_result(
                    source,
                    cleanup,
                    route,
                    LocalEditorAttempt::Failed(error),
                );
            }
        }
    }

    model_free_result(source, cleanup, route, LocalEditorAttempt::NotAttempted)
}

fn model_free_result(
    source: &str,
    cleanup: CleanupConfig,
    route: RefinementRoute,
    attempt: LocalEditorAttempt,
) -> Result<OptionalRefinementOutput, OptionalRefinementError> {
    let fallback =
        refine_without_model(source, cleanup).map_err(OptionalRefinementError::Cleanup)?;
    let validation = OutputValidationConfig::deterministic(cleanup.maximum_bytes())
        .map_err(OptionalRefinementError::Validation)?;
    validate_refined_output(source, fallback.text(), validation)
        .map_err(OptionalRefinementError::Validation)?;

    let path = if attempt == LocalEditorAttempt::NotAttempted {
        OptionalRefinementPath::ModelFree
    } else {
        OptionalRefinementPath::LocalEditorFallback
    };
    Ok(OptionalRefinementOutput {
        owner: OptionalRefinementOwner::ModelFree(fallback),
        report: OptionalRefinementReport {
            route,
            path,
            attempt,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RefinementNeed;

    struct TestEditor {
        result: Result<&'static str, LocalEditorError>,
        calls: usize,
    }

    impl LocalEditor for TestEditor {
        fn refine(
            &mut self,
            _source: &str,
            output: &mut LocalEditorOutput,
        ) -> Result<(), LocalEditorError> {
            self.calls += 1;
            output.write_str(self.result?)
        }
    }

    fn signals() -> RefinementSignals {
        RefinementSignals::from_need(RefinementNeed::SemanticRewrite)
    }

    fn validation() -> Result<OutputValidationConfig, OutputValidationError> {
        OutputValidationConfig::new(128, 32, 20_000)
    }

    #[test]
    fn model_free_route_never_calls_the_editor() -> Result<(), Box<dyn Error>> {
        let mut editor = TestEditor {
            result: Ok("private candidate"),
            calls: 0,
        };
        let output = refine_with_optional_editor(
            "  hello  world ",
            RefinementSignals::none(),
            LocalEditorGate::Ready,
            &mut editor,
            CleanupConfig::default(),
            validation()?,
        )?;

        assert_eq!(output.text(), "Hello world");
        assert_eq!(output.report().path(), OptionalRefinementPath::ModelFree);
        assert_eq!(output.report().attempt(), LocalEditorAttempt::NotAttempted);
        assert_eq!(editor.calls, 0);
        Ok(())
    }

    #[test]
    fn valid_local_candidate_is_accepted() -> Result<(), Box<dyn Error>> {
        let mut editor = TestEditor {
            result: Ok("Deploy Friday morning."),
            calls: 0,
        };
        let output = refine_with_optional_editor(
            "deploy tomorrow actually no friday morning",
            signals(),
            LocalEditorGate::Ready,
            &mut editor,
            CleanupConfig::default(),
            validation()?,
        )?;

        assert_eq!(output.text(), "Deploy Friday morning.");
        assert_eq!(output.report().path(), OptionalRefinementPath::LocalEditor);
        assert_eq!(output.report().attempt(), LocalEditorAttempt::Accepted);
        assert_eq!(editor.calls, 1);
        Ok(())
    }

    #[test]
    fn runtime_failure_falls_back_to_deterministic_cleanup() -> Result<(), Box<dyn Error>> {
        let mut editor = TestEditor {
            result: Err(LocalEditorError::Timeout),
            calls: 0,
        };
        let output = refine_with_optional_editor(
            "  keep   literal  ",
            signals(),
            LocalEditorGate::Ready,
            &mut editor,
            CleanupConfig::default(),
            validation()?,
        )?;

        assert_eq!(output.text(), "Keep literal");
        assert_eq!(
            output.report().path(),
            OptionalRefinementPath::LocalEditorFallback
        );
        assert_eq!(
            output.report().attempt(),
            LocalEditorAttempt::Failed(LocalEditorError::Timeout)
        );
        Ok(())
    }

    #[test]
    fn invalid_and_empty_candidates_fail_closed() -> Result<(), Box<dyn Error>> {
        for (candidate, expected) in [
            (
                "unsafe\0candidate",
                LocalEditorAttempt::Rejected(OutputValidationError::DisallowedControl),
            ),
            ("", LocalEditorAttempt::EmptyOutput),
        ] {
            let mut editor = TestEditor {
                result: Ok(candidate),
                calls: 0,
            };
            let output = refine_with_optional_editor(
                "keep source",
                signals(),
                LocalEditorGate::Ready,
                &mut editor,
                CleanupConfig::default(),
                validation()?,
            )?;

            assert_eq!(output.text(), "Keep source");
            assert_eq!(output.report().attempt(), expected);
        }
        Ok(())
    }

    #[test]
    fn semantic_mutation_falls_back_before_acceptance() -> Result<(), Box<dyn Error>> {
        let mut editor = TestEditor {
            result: Ok("ship v3"),
            calls: 0,
        };
        let output = refine_with_optional_editor(
            "ship v2",
            signals(),
            LocalEditorGate::Ready,
            &mut editor,
            CleanupConfig::default(),
            validation()?,
        )?;

        assert_eq!(output.text(), "Ship v2");
        assert_eq!(
            output.report().attempt(),
            LocalEditorAttempt::RejectedSemantic(SemanticVerificationError::ProtectedTokenChanged)
        );
        Ok(())
    }

    #[test]
    fn editor_failure_can_reach_sanitized_raw_fallback() -> Result<(), Box<dyn Error>> {
        let marker = "keep\0 visible \u{202e}text";
        let mut editor = TestEditor {
            result: Err(LocalEditorError::DecoderFailed),
            calls: 0,
        };
        let output = refine_with_optional_editor(
            marker,
            signals(),
            LocalEditorGate::Ready,
            &mut editor,
            CleanupConfig::default(),
            validation()?,
        )?;

        assert_eq!(output.text(), "keep visible text");
        assert!(!format!("{:?}", output.report()).contains(marker));
        Ok(())
    }

    #[test]
    fn sink_bounds_and_errors_never_echo_payloads() -> Result<(), Box<dyn Error>> {
        let marker = "private-marker";
        let mut sink = LocalEditorOutput::new(4);
        let Err(error) = sink.write_str(marker) else {
            return Err("marker unexpectedly fit the bounded sink".into());
        };

        assert_eq!(error, LocalEditorError::OutputTooLong);
        assert!(!error.to_string().contains(marker));
        assert_eq!(sink.text(), "");
        sink.write_str("safe")?;
        sink.clear();
        assert_eq!(sink.text(), "");
        Ok(())
    }
}
