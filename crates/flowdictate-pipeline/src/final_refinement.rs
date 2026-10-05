use std::{error::Error, fmt};

use flowdictate_asr_ipc::WorkerTranscript;
use flowdictate_refine::{
    apply_spoken_rules, apply_user_dictionary, refine_with_optional_editor as run_optional_editor,
    refine_without_model, validate_refined_output, DictionaryEntry, DictionaryError, LocalEditor,
    LocalEditorGate, OptionalRefinementError, OptionalRefinementOutput, OutputValidationConfig,
    RefinementSignals, SpokenRulesPolicy, SpokenRulesTranscript,
};

use crate::{
    CleanupConfig, CleanupError, ConsensusCommit, NativeStreamingTranscript, OutputValidationError,
    PipelineOutput, RefinementOutput,
};

/// Read-only text contract for finalized backend transcript types.
pub trait FinalTranscript {
    /// Borrows the exact stable transcript text.
    fn final_text(&self) -> &str;
}

impl FinalTranscript for WorkerTranscript {
    fn final_text(&self) -> &str {
        self.text()
    }
}

/// Fixed, payload-free failure from final transcript refinement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinalRefinementError {
    /// An experimental native hypothesis was not marked final.
    NotFinal,
    /// Streaming commits did not exactly match a completed session boundary.
    InvalidCommitSet,
    /// Explicit local dictionary substitution failed.
    Dictionary(DictionaryError),
    /// Bounded deterministic refinement failed.
    Cleanup(CleanupError),
    /// The complete optional-editor fallback hierarchy failed.
    OptionalEditor(OptionalRefinementError),
    /// The refined result failed the deterministic output policy.
    Validation(OutputValidationError),
}

impl fmt::Display for FinalRefinementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotFinal => "native transcript is not final",
            Self::InvalidCommitSet => "streaming commit set is invalid",
            Self::Dictionary(_) => "final transcript dictionary substitution failed",
            Self::Cleanup(_) => "final transcript cleanup failed",
            Self::OptionalEditor(_) => "final transcript optional-editor fallback failed",
            Self::Validation(_) => "final transcript validation failed",
        })
    }
}

impl Error for FinalRefinementError {}

/// Proof that a streaming session reached a user-driven final boundary.
///
/// Values can only be issued by a successful streaming stop or hotkey release.
/// This type intentionally implements neither `Clone` nor `Copy`, so one
/// completed-session boundary authorizes at most one final refinement attempt.
#[derive(Debug, Eq, PartialEq)]
pub struct StreamingFinalBoundary {
    expected_commits: usize,
}

impl StreamingFinalBoundary {
    pub(crate) const fn new(expected_commits: usize) -> Self {
        Self { expected_commits }
    }

    const fn consume(self) -> usize {
        self.expected_commits
    }
}

impl<T: FinalTranscript> PipelineOutput<T> {
    /// Refines this finalized transcript without invoking a model.
    ///
    /// The output is returned only after the strict deterministic validation
    /// profile accepts it. The source transcript remains owned by this final
    /// pipeline output and is never logged, persisted, or sent to a network.
    ///
    /// # Errors
    ///
    /// Returns a fixed cleanup or validation category without transcript text.
    pub fn refine_model_free(
        &self,
        config: CleanupConfig,
    ) -> Result<RefinementOutput, FinalRefinementError> {
        refine_validated(self.transcript().final_text(), config)
    }

    /// Applies an explicit local dictionary before final model-free refinement.
    ///
    /// The caller selects an output-growth policy because a user-entered
    /// written form may be longer than its spoken token. Entries are borrowed
    /// and never retained by the refinement pipeline.
    ///
    /// # Errors
    ///
    /// Returns a fixed dictionary, cleanup, or validation category.
    pub fn refine_model_free_with_dictionary(
        &self,
        entries: &[DictionaryEntry<'_>],
        config: CleanupConfig,
        validation: OutputValidationConfig,
    ) -> Result<RefinementOutput, FinalRefinementError> {
        refine_dictionary_validated(self.transcript().final_text(), entries, config, validation)
    }

    /// Applies explicit filler/formatting rules to this final transcript.
    ///
    /// # Errors
    ///
    /// Returns a fixed cleanup or validation category.
    pub fn refine_model_free_with_spoken_rules(
        &self,
        policy: SpokenRulesPolicy,
        config: CleanupConfig,
        validation: OutputValidationConfig,
    ) -> Result<SpokenRulesTranscript, FinalRefinementError> {
        refine_spoken_rules_validated(self.transcript().final_text(), policy, config, validation)
    }

    /// Runs an explicitly gated local editor with the complete offline fallback.
    ///
    /// Routing consumes metadata only. Transcript text reaches `editor` only
    /// when the route is justified and the local gate is ready; every accepted
    /// candidate passes the caller's explicit bounded output policy.
    ///
    /// # Errors
    ///
    /// Returns a fixed category only when the model-free fallback also fails.
    pub fn refine_with_optional_editor<E: LocalEditor>(
        &self,
        signals: RefinementSignals,
        gate: LocalEditorGate,
        editor: &mut E,
        config: CleanupConfig,
        validation: OutputValidationConfig,
    ) -> Result<OptionalRefinementOutput, FinalRefinementError> {
        refine_optional_validated(
            self.transcript().final_text(),
            signals,
            gate,
            editor,
            config,
            validation,
        )
    }
}

/// Refines one explicitly final experimental-native transcript.
///
/// The finality flag is checked before transcript text is borrowed. A partial
/// hypothesis therefore cannot reach cleanup, validation, logging, or output.
///
/// # Errors
///
/// Returns [`FinalRefinementError::NotFinal`] for a partial hypothesis, or a
/// fixed cleanup/validation category for a final transcript.
pub fn refine_native_final<T: NativeStreamingTranscript>(
    transcript: &T,
    config: CleanupConfig,
) -> Result<RefinementOutput, FinalRefinementError> {
    if !transcript.is_final() {
        return Err(FinalRefinementError::NotFinal);
    }
    refine_validated(transcript.text(), config)
}

/// Applies an explicit local dictionary to one confirmed native final.
///
/// # Errors
///
/// Rejects partials before borrowing text, then returns only fixed dictionary,
/// cleanup, or validation categories.
pub fn refine_native_final_with_dictionary<T: NativeStreamingTranscript>(
    transcript: &T,
    entries: &[DictionaryEntry<'_>],
    config: CleanupConfig,
    validation: OutputValidationConfig,
) -> Result<RefinementOutput, FinalRefinementError> {
    if !transcript.is_final() {
        return Err(FinalRefinementError::NotFinal);
    }
    refine_dictionary_validated(transcript.text(), entries, config, validation)
}

/// Applies explicit filler/formatting rules to one confirmed native final.
///
/// # Errors
///
/// Rejects partials before borrowing text, then returns a fixed cleanup or
/// validation category.
pub fn refine_native_final_with_spoken_rules<T: NativeStreamingTranscript>(
    transcript: &T,
    policy: SpokenRulesPolicy,
    config: CleanupConfig,
    validation: OutputValidationConfig,
) -> Result<SpokenRulesTranscript, FinalRefinementError> {
    if !transcript.is_final() {
        return Err(FinalRefinementError::NotFinal);
    }
    refine_spoken_rules_validated(transcript.text(), policy, config, validation)
}

/// Runs the optional local-editor hierarchy for one confirmed native final.
///
/// # Errors
///
/// Rejects partials before borrowing text or calling the editor, then returns a
/// fixed category only when the complete model-free fallback fails.
pub fn refine_native_final_with_optional_editor<T: NativeStreamingTranscript, E: LocalEditor>(
    transcript: &T,
    signals: RefinementSignals,
    gate: LocalEditorGate,
    editor: &mut E,
    config: CleanupConfig,
    validation: OutputValidationConfig,
) -> Result<OptionalRefinementOutput, FinalRefinementError> {
    if !transcript.is_final() {
        return Err(FinalRefinementError::NotFinal);
    }
    refine_optional_validated(transcript.text(), signals, gate, editor, config, validation)
}

/// Refines the exact commit set from one successfully completed live session.
///
/// `commits` must be the fresh caller-owned buffer used for that session. The
/// boundary verifies its exact length, monotonic generations, sequence numbers,
/// and timestamps before text is assembled. Both the consumed commit chunks and
/// the temporary assembled source are overwritten on drop.
///
/// # Errors
///
/// Returns [`FinalRefinementError::InvalidCommitSet`] for an incomplete,
/// reordered, mixed, or reused buffer, or a fixed cleanup/validation category.
pub fn refine_streaming_final(
    boundary: StreamingFinalBoundary,
    commits: Vec<ConsensusCommit>,
    config: CleanupConfig,
) -> Result<RefinementOutput, FinalRefinementError> {
    let (source, commits) = assemble_streaming_source(boundary, commits, config)?;
    let result = refine_validated(source.text(), config);
    drop(source);
    drop(commits);
    result
}

/// Applies an explicit local dictionary to one completed streaming session.
///
/// # Errors
///
/// Returns a fixed commit-set, dictionary, cleanup, or validation category.
pub fn refine_streaming_final_with_dictionary(
    boundary: StreamingFinalBoundary,
    commits: Vec<ConsensusCommit>,
    entries: &[DictionaryEntry<'_>],
    config: CleanupConfig,
    validation: OutputValidationConfig,
) -> Result<RefinementOutput, FinalRefinementError> {
    let (source, commits) = assemble_streaming_source(boundary, commits, config)?;
    let result = refine_dictionary_validated(source.text(), entries, config, validation);
    drop(source);
    drop(commits);
    result
}

/// Applies explicit filler/formatting rules to one completed streaming session.
///
/// # Errors
///
/// Returns a fixed commit-set, cleanup, or validation category.
pub fn refine_streaming_final_with_spoken_rules(
    boundary: StreamingFinalBoundary,
    commits: Vec<ConsensusCommit>,
    policy: SpokenRulesPolicy,
    config: CleanupConfig,
    validation: OutputValidationConfig,
) -> Result<SpokenRulesTranscript, FinalRefinementError> {
    let (source, commits) = assemble_streaming_source(boundary, commits, config)?;
    let result = refine_spoken_rules_validated(source.text(), policy, config, validation);
    drop(source);
    drop(commits);
    result
}

/// Runs the optional local-editor hierarchy for one completed streaming session.
///
/// # Errors
///
/// Returns a fixed commit-set category or a fixed error only when the complete
/// model-free fallback cannot finish.
pub fn refine_streaming_final_with_optional_editor<E: LocalEditor>(
    boundary: StreamingFinalBoundary,
    commits: Vec<ConsensusCommit>,
    signals: RefinementSignals,
    gate: LocalEditorGate,
    editor: &mut E,
    config: CleanupConfig,
    validation: OutputValidationConfig,
) -> Result<OptionalRefinementOutput, FinalRefinementError> {
    let (source, commits) = assemble_streaming_source(boundary, commits, config)?;
    let result =
        refine_optional_validated(source.text(), signals, gate, editor, config, validation);
    drop(source);
    drop(commits);
    result
}

fn assemble_streaming_source(
    boundary: StreamingFinalBoundary,
    commits: Vec<ConsensusCommit>,
    config: CleanupConfig,
) -> Result<(SensitiveTranscript, Vec<ConsensusCommit>), FinalRefinementError> {
    let expected_commits = boundary.consume();
    if commits.len() != expected_commits {
        return Err(FinalRefinementError::InvalidCommitSet);
    }

    let mut total_bytes = 0usize;
    let mut previous: Option<(u64, u64, u64)> = None;
    for commit in &commits {
        if commit.text().is_empty() {
            return Err(FinalRefinementError::InvalidCommitSet);
        }
        if let Some((generation, sequence, end_ms)) = previous {
            let valid_sequence = if commit.generation() == generation {
                commit.sequence() == sequence.saturating_add(1)
            } else {
                commit.generation() > generation && commit.sequence() == 1
            };
            if !valid_sequence || commit.start_ms() < end_ms {
                return Err(FinalRefinementError::InvalidCommitSet);
            }
        } else if commit.sequence() != 1 {
            return Err(FinalRefinementError::InvalidCommitSet);
        }
        total_bytes = total_bytes
            .checked_add(commit.text().len())
            .ok_or(FinalRefinementError::Cleanup(CleanupError::InputTooLong))?;
        previous = Some((commit.generation(), commit.sequence(), commit.end_ms()));
    }
    if total_bytes > config.maximum_bytes() {
        return Err(FinalRefinementError::Cleanup(CleanupError::InputTooLong));
    }

    let mut source = SensitiveTranscript::new(total_bytes)?;
    for commit in &commits {
        source.bytes.extend_from_slice(commit.text().as_bytes());
    }
    Ok((source, commits))
}

struct SensitiveTranscript {
    bytes: Vec<u8>,
}

impl SensitiveTranscript {
    fn new(capacity: usize) -> Result<Self, FinalRefinementError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| FinalRefinementError::Cleanup(CleanupError::AllocationFailed))?;
        Ok(Self { bytes })
    }

    fn text(&self) -> &str {
        std::str::from_utf8(&self.bytes).unwrap_or_default()
    }
}

impl Drop for SensitiveTranscript {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}

fn refine_validated(
    source: &str,
    config: CleanupConfig,
) -> Result<RefinementOutput, FinalRefinementError> {
    let refined = refine_without_model(source, config).map_err(FinalRefinementError::Cleanup)?;
    let policy = OutputValidationConfig::deterministic(config.maximum_bytes())
        .map_err(FinalRefinementError::Validation)?;
    validate_refined_output(source, refined.text(), policy)
        .map_err(FinalRefinementError::Validation)?;
    Ok(refined)
}

fn refine_dictionary_validated(
    source: &str,
    entries: &[DictionaryEntry<'_>],
    config: CleanupConfig,
    validation: OutputValidationConfig,
) -> Result<RefinementOutput, FinalRefinementError> {
    let substituted =
        apply_user_dictionary(source, entries, config).map_err(FinalRefinementError::Dictionary)?;
    let refined =
        refine_without_model(substituted.text(), config).map_err(FinalRefinementError::Cleanup)?;
    validate_refined_output(source, refined.text(), validation)
        .map_err(FinalRefinementError::Validation)?;
    drop(substituted);
    Ok(refined)
}

fn refine_spoken_rules_validated(
    source: &str,
    policy: SpokenRulesPolicy,
    config: CleanupConfig,
    validation: OutputValidationConfig,
) -> Result<SpokenRulesTranscript, FinalRefinementError> {
    let cleaned = refine_without_model(source, config).map_err(FinalRefinementError::Cleanup)?;
    let refined = apply_spoken_rules(cleaned.text(), policy, config)
        .map_err(FinalRefinementError::Cleanup)?;
    validate_refined_output(source, refined.text(), validation)
        .map_err(FinalRefinementError::Validation)?;
    drop(cleaned);
    Ok(refined)
}

fn refine_optional_validated<E: LocalEditor>(
    source: &str,
    signals: RefinementSignals,
    gate: LocalEditorGate,
    editor: &mut E,
    config: CleanupConfig,
    validation: OutputValidationConfig,
) -> Result<OptionalRefinementOutput, FinalRefinementError> {
    run_optional_editor(source, signals, gate, editor, config, validation)
        .map_err(FinalRefinementError::OptionalEditor)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use flowdictate_audio::FinalizeReason;
    use flowdictate_refine::{
        DictionaryEntry, LineBreakPolicy, LocalEditorAttempt, LocalEditorError, LocalEditorOutput,
        OptionalRefinementPath, RefinementNeed, RefinementPath, SpokenRulesPolicy,
    };

    use super::*;
    use crate::{ConsensusCommitter, ConsensusConfig, HypothesisSegment, TranscriptHypothesis};

    struct TestFinal(&'static str);

    impl FinalTranscript for TestFinal {
        fn final_text(&self) -> &str {
            self.0
        }
    }

    fn output(text: &'static str) -> PipelineOutput<TestFinal> {
        PipelineOutput {
            transcript: TestFinal(text),
            reason: FinalizeReason::HotkeyReleased,
            canonical_samples: 16_000,
        }
    }

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

    fn editor_signals() -> RefinementSignals {
        RefinementSignals::from_need(RefinementNeed::SemanticRewrite)
    }

    fn editor_validation() -> Result<OutputValidationConfig, OutputValidationError> {
        OutputValidationConfig::new(128, 32, 20_000)
    }

    #[test]
    fn finalized_output_crosses_cleanup_and_validation() -> Result<(), FinalRefinementError> {
        let output = output("  hello   , world  ");
        let refined = output.refine_model_free(CleanupConfig::default())?;

        assert_eq!(refined.text(), "Hello, world");
        assert_eq!(refined.path(), RefinementPath::Deterministic);
        assert_eq!(output.transcript().final_text(), "  hello   , world  ");
        Ok(())
    }

    #[test]
    fn unsafe_final_text_uses_validated_sanitized_fallback() -> Result<(), FinalRefinementError> {
        let output = output(" keep\0 visible \u{202e}text ");
        let refined = output.refine_model_free(CleanupConfig::default())?;

        assert_eq!(refined.text(), "keep visible text");
        assert_eq!(refined.path(), RefinementPath::SanitizedRaw);
        Ok(())
    }

    #[test]
    fn finalized_output_applies_explicit_dictionary_with_bounded_growth(
    ) -> Result<(), Box<dyn Error>> {
        let output = output("cpp is useful");
        let entries = [DictionaryEntry::new("cpp", "C++")?];
        let validation = OutputValidationConfig::new(64, 2, 20_000)?;
        let refined = output.refine_model_free_with_dictionary(
            &entries,
            CleanupConfig::default(),
            validation,
        )?;

        assert_eq!(refined.text(), "C++ is useful");
        Ok(())
    }

    #[test]
    fn finalized_output_accepts_or_falls_back_from_local_editor() -> Result<(), Box<dyn Error>> {
        let mut accepted = TestEditor {
            result: Ok("Deploy Friday morning."),
            calls: 0,
        };
        let refined = output("deploy tomorrow actually no friday morning")
            .refine_with_optional_editor(
                editor_signals(),
                LocalEditorGate::Ready,
                &mut accepted,
                CleanupConfig::default(),
                editor_validation()?,
            )?;
        assert_eq!(refined.text(), "Deploy Friday morning.");
        assert_eq!(refined.report().path(), OptionalRefinementPath::LocalEditor);

        let mut failed = TestEditor {
            result: Err(LocalEditorError::Timeout),
            calls: 0,
        };
        let fallback = output("  retain   source  ").refine_with_optional_editor(
            editor_signals(),
            LocalEditorGate::Ready,
            &mut failed,
            CleanupConfig::default(),
            editor_validation()?,
        )?;
        assert_eq!(fallback.text(), "Retain source");
        assert_eq!(
            fallback.report().attempt(),
            LocalEditorAttempt::Failed(LocalEditorError::Timeout)
        );
        Ok(())
    }

    #[test]
    fn spoken_formatting_requires_explicit_policy_and_line_validation() -> Result<(), Box<dyn Error>>
    {
        let output = output("first new line second");
        let validation =
            OutputValidationConfig::deterministic(64)?.with_line_breaks(LineBreakPolicy::LfOnly);
        let refined = output.refine_model_free_with_spoken_rules(
            SpokenRulesPolicy::EnglishV1,
            CleanupConfig::default(),
            validation,
        )?;

        assert_eq!(refined.text(), "First\nSecond");
        Ok(())
    }

    #[test]
    fn bounds_fail_without_echoing_final_text() -> Result<(), CleanupError> {
        let marker = "private-marker";
        let Err(error) = output(marker).refine_model_free(CleanupConfig::new(4)?) else {
            return Err(CleanupError::InvalidConfig);
        };

        assert_eq!(
            error,
            FinalRefinementError::Cleanup(CleanupError::InputTooLong)
        );
        assert!(!error.to_string().contains(marker));
        Ok(())
    }

    #[test]
    fn worker_transcript_is_an_allowed_final_type() {
        const fn assert_final<T: FinalTranscript>() {}
        assert_final::<WorkerTranscript>();
    }

    struct TestNative {
        text: &'static str,
        final_result: bool,
        text_reads: Cell<usize>,
    }

    impl NativeStreamingTranscript for TestNative {
        fn text(&self) -> &str {
            self.text_reads.set(self.text_reads.get().saturating_add(1));
            self.text
        }

        fn is_final(&self) -> bool {
            self.final_result
        }
    }

    #[test]
    fn final_native_text_crosses_the_same_validated_path() -> Result<(), FinalRefinementError> {
        let transcript = TestNative {
            text: "  native   final  ",
            final_result: true,
            text_reads: Cell::new(0),
        };
        let refined = refine_native_final(&transcript, CleanupConfig::default())?;

        assert_eq!(refined.text(), "Native final");
        assert_eq!(transcript.text_reads.get(), 1);
        Ok(())
    }

    #[test]
    fn native_partial_is_rejected_before_text_is_borrowed() -> Result<(), CleanupError> {
        let marker = "private-partial-marker";
        let transcript = TestNative {
            text: marker,
            final_result: false,
            text_reads: Cell::new(0),
        };
        let Err(error) = refine_native_final(&transcript, CleanupConfig::new(4)?) else {
            return Err(CleanupError::InvalidConfig);
        };

        assert_eq!(error, FinalRefinementError::NotFinal);
        assert_eq!(transcript.text_reads.get(), 0);
        assert!(!error.to_string().contains(marker));
        Ok(())
    }

    #[test]
    fn native_partial_never_calls_the_optional_editor() -> Result<(), Box<dyn Error>> {
        let transcript = TestNative {
            text: "private partial",
            final_result: false,
            text_reads: Cell::new(0),
        };
        let mut editor = TestEditor {
            result: Ok("must not be used"),
            calls: 0,
        };
        let result = refine_native_final_with_optional_editor(
            &transcript,
            editor_signals(),
            LocalEditorGate::Ready,
            &mut editor,
            CleanupConfig::default(),
            editor_validation()?,
        );

        assert!(matches!(result, Err(FinalRefinementError::NotFinal)));
        assert_eq!(transcript.text_reads.get(), 0);
        assert_eq!(editor.calls, 0);
        Ok(())
    }

    struct StreamHypothesis<'a>(&'a [HypothesisSegment<'a>]);

    impl TranscriptHypothesis for StreamHypothesis<'_> {
        fn segment_count(&self) -> usize {
            self.0.len()
        }

        fn segment(&self, index: usize) -> Option<HypothesisSegment<'_>> {
            self.0.get(index).copied()
        }
    }

    fn stream_segment(text: &str, start_ms: u32, end_ms: u32) -> HypothesisSegment<'_> {
        HypothesisSegment {
            text,
            start_ms,
            end_ms,
        }
    }

    fn streaming_commits() -> Result<Vec<ConsensusCommit>, Box<dyn Error>> {
        let mut committer = ConsensusCommitter::new(ConsensusConfig::new(2, 0, 0)?)?;
        let mut commits = Vec::with_capacity(2);
        let first = StreamHypothesis(&[stream_segment("hello  ", 0, 100)]);
        committer.observe(0, 100, &first, &mut commits)?;
        committer.observe(0, 100, &first, &mut commits)?;
        let second = StreamHypothesis(&[stream_segment(" world", 0, 100)]);
        committer.observe(100, 200, &second, &mut commits)?;
        committer.observe(100, 200, &second, &mut commits)?;
        Ok(commits)
    }

    #[test]
    fn completed_streaming_commits_are_assembled_once_then_refined() -> Result<(), Box<dyn Error>> {
        let commits = streaming_commits()?;
        let refined = refine_streaming_final(
            StreamingFinalBoundary::new(2),
            commits,
            CleanupConfig::default(),
        )?;

        assert_eq!(refined.text(), "Hello world");
        assert_eq!(refined.path(), RefinementPath::Deterministic);
        Ok(())
    }

    #[test]
    fn streaming_refinement_rejects_missing_or_reused_commit_buffers() -> Result<(), Box<dyn Error>>
    {
        let mut missing = streaming_commits()?;
        drop(missing.remove(0));
        assert!(matches!(
            refine_streaming_final(
                StreamingFinalBoundary::new(1),
                missing,
                CleanupConfig::default()
            ),
            Err(FinalRefinementError::InvalidCommitSet)
        ));

        assert!(matches!(
            refine_streaming_final(
                StreamingFinalBoundary::new(1),
                streaming_commits()?,
                CleanupConfig::default()
            ),
            Err(FinalRefinementError::InvalidCommitSet)
        ));
        Ok(())
    }

    #[test]
    fn streaming_refinement_checks_total_bytes_before_assembly() -> Result<(), Box<dyn Error>> {
        let marker = "hello";
        let error = refine_streaming_final(
            StreamingFinalBoundary::new(2),
            streaming_commits()?,
            CleanupConfig::new(4)?,
        )
        .err()
        .ok_or(CleanupError::InvalidConfig)?;

        assert_eq!(
            error,
            FinalRefinementError::Cleanup(CleanupError::InputTooLong)
        );
        assert!(!error.to_string().contains(marker));
        Ok(())
    }

    #[test]
    fn streaming_dictionary_path_uses_the_same_one_shot_boundary() -> Result<(), Box<dyn Error>> {
        let entries = [DictionaryEntry::new("hello", "FlowDictate")?];
        let validation = OutputValidationConfig::new(64, 8, 20_000)?;
        let refined = refine_streaming_final_with_dictionary(
            StreamingFinalBoundary::new(2),
            streaming_commits()?,
            &entries,
            CleanupConfig::default(),
            validation,
        )?;

        assert_eq!(refined.text(), "FlowDictate world");
        Ok(())
    }

    #[test]
    fn completed_streaming_boundary_can_authorize_local_editing() -> Result<(), Box<dyn Error>> {
        let mut editor = TestEditor {
            result: Ok("Hello, world."),
            calls: 0,
        };
        let refined = refine_streaming_final_with_optional_editor(
            StreamingFinalBoundary::new(2),
            streaming_commits()?,
            editor_signals(),
            LocalEditorGate::Ready,
            &mut editor,
            CleanupConfig::default(),
            editor_validation()?,
        )?;

        assert_eq!(refined.text(), "Hello, world.");
        assert_eq!(refined.report().path(), OptionalRefinementPath::LocalEditor);
        assert_eq!(editor.calls, 1);
        Ok(())
    }
}
