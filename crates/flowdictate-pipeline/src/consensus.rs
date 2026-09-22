//! Monotonic, bounded consensus over rolling local ASR hypotheses.

use std::{error::Error, fmt, mem};

use flowdictate_asr_ipc::{WorkerTranscript, MAX_TRANSCRIPT_BYTES, MAX_TRANSCRIPT_SEGMENTS};

const MAX_STABILITY_OBSERVATIONS: u8 = 8;
const MAX_TIME_MARGIN_MS: u32 = 10_000;

/// One borrowed transcript segment presented to the consensus engine.
#[derive(Clone, Copy)]
pub struct HypothesisSegment<'a> {
    /// Exact display text emitted by the local model.
    pub text: &'a str,
    /// Segment start relative to the current inference window.
    pub start_ms: u32,
    /// Segment end relative to the current inference window.
    pub end_ms: u32,
}

/// Narrow read-only view over one bounded local ASR hypothesis.
pub trait TranscriptHypothesis {
    /// Returns the number of transcript segments.
    fn segment_count(&self) -> usize;

    /// Borrows one exact segment, or returns `None` for an invalid index/view.
    fn segment(&self, index: usize) -> Option<HypothesisSegment<'_>>;
}

impl TranscriptHypothesis for WorkerTranscript {
    fn segment_count(&self) -> usize {
        self.segments().len()
    }

    fn segment(&self, index: usize) -> Option<HypothesisSegment<'_>> {
        let segment = self.segments().get(index)?;
        let text = self.text().get(segment.byte_start..segment.byte_end)?;
        Some(HypothesisSegment {
            text,
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
        })
    }
}

/// Validated policy for conservative stable-prefix commitment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsensusConfig {
    required_observations: u8,
    unstable_tail_ms: u32,
    retained_overlap_ms: u32,
}

impl ConsensusConfig {
    /// Creates a bounded consensus policy.
    ///
    /// # Errors
    ///
    /// Rejects fewer than two or more than eight observations, time margins
    /// above ten seconds, or overlap larger than the unstable trailing margin.
    pub const fn new(
        required_observations: u8,
        unstable_tail_ms: u32,
        retained_overlap_ms: u32,
    ) -> Result<Self, ConsensusError> {
        if required_observations < 2 || required_observations > MAX_STABILITY_OBSERVATIONS {
            return Err(ConsensusError::InvalidConfig);
        }
        if unstable_tail_ms > MAX_TIME_MARGIN_MS || retained_overlap_ms > unstable_tail_ms {
            return Err(ConsensusError::InvalidConfig);
        }
        Ok(Self {
            required_observations,
            unstable_tail_ms,
            retained_overlap_ms,
        })
    }
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            required_observations: 2,
            unstable_tail_ms: 800,
            retained_overlap_ms: 320,
        }
    }
}

/// One newly committed, immutable text delta.
///
/// This type intentionally implements neither `Clone` nor `Debug`. Its owned
/// UTF-8 bytes are overwritten on drop.
pub struct ConsensusCommit {
    text: Vec<u8>,
    generation: u64,
    sequence: u64,
    start_ms: u64,
    end_ms: u64,
    segments: usize,
}

impl ConsensusCommit {
    /// Returns the exact local-model display text committed in this delta.
    #[must_use]
    pub fn text(&self) -> &str {
        std::str::from_utf8(&self.text).unwrap_or_default()
    }

    /// Returns the reset generation containing this commit.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the monotonic sequence within the current generation.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Returns the absolute start timestamp of the delta.
    #[must_use]
    pub const fn start_ms(&self) -> u64 {
        self.start_ms
    }

    /// Returns the absolute exclusive end timestamp of the delta.
    #[must_use]
    pub const fn end_ms(&self) -> u64 {
        self.end_ms
    }

    /// Returns the number of complete ASR segments in the delta.
    #[must_use]
    pub const fn segments(&self) -> usize {
        self.segments
    }
}

impl Drop for ConsensusCommit {
    fn drop(&mut self) {
        self.text.fill(0);
    }
}

/// Payload-free counters from one rolling hypothesis observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsensusReport {
    /// Eligible, uncommitted segments retained after this observation.
    pub pending_segments: usize,
    /// Exact display bytes retained in the pending suffix.
    pub pending_bytes: usize,
    /// Whether a new immutable commit delta was appended.
    pub commit_written: bool,
    /// Absolute audio timestamp before which PCM may be discarded.
    pub discard_audio_before_ms: u64,
}

#[derive(Clone, Copy)]
struct CandidateSegment {
    byte_start: usize,
    byte_end: usize,
    start_ms: u64,
    end_ms: u64,
    observations: u8,
}

/// Stateful owner of only the bounded, uncommitted hypothesis suffix.
pub struct ConsensusCommitter {
    config: ConsensusConfig,
    generation: u64,
    next_sequence: u64,
    committed_through_ms: u64,
    previous_text: Vec<u8>,
    previous: Vec<CandidateSegment>,
    next_text: Vec<u8>,
    next: Vec<CandidateSegment>,
}

impl ConsensusCommitter {
    /// Preallocates both bounded comparison generations.
    ///
    /// # Errors
    ///
    /// Returns a fixed allocation failure without retaining partial state.
    pub fn new(config: ConsensusConfig) -> Result<Self, ConsensusError> {
        let mut previous_text = Vec::new();
        previous_text
            .try_reserve_exact(MAX_TRANSCRIPT_BYTES)
            .map_err(|_| ConsensusError::AllocationFailed)?;
        let mut previous = Vec::new();
        previous
            .try_reserve_exact(MAX_TRANSCRIPT_SEGMENTS)
            .map_err(|_| ConsensusError::AllocationFailed)?;
        let mut next_text = Vec::new();
        next_text
            .try_reserve_exact(MAX_TRANSCRIPT_BYTES)
            .map_err(|_| ConsensusError::AllocationFailed)?;
        let mut next = Vec::new();
        next.try_reserve_exact(MAX_TRANSCRIPT_SEGMENTS)
            .map_err(|_| ConsensusError::AllocationFailed)?;
        Ok(Self {
            config,
            generation: 0,
            next_sequence: 0,
            committed_through_ms: 0,
            previous_text,
            previous,
            next_text,
            next,
        })
    }

    /// Borrows the exact currently uncommitted display suffix.
    #[must_use]
    pub fn pending_text(&self) -> &str {
        std::str::from_utf8(&self.previous_text).unwrap_or_default()
    }

    /// Returns the current reset generation.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the absolute PCM timestamp safe to discard while retaining the
    /// configured overlap before the latest commit boundary.
    #[must_use]
    pub const fn discard_audio_before_ms(&self) -> u64 {
        self.committed_through_ms
            .saturating_sub(self.config.retained_overlap_ms as u64)
    }

    /// Observes one rolling local hypothesis and commits only a repeated stable
    /// prefix outside the configured unstable trailing margin.
    ///
    /// `outputs` must have one spare slot. Invalid input and allocation failure
    /// leave the prior pending suffix and caller output unchanged.
    ///
    /// # Errors
    ///
    /// Rejects insufficient output capacity, malformed/oversized hypotheses,
    /// timestamp overflow/regression, committed-boundary overlap, or allocation
    /// failure.
    pub fn observe<H: TranscriptHypothesis>(
        &mut self,
        window_start_ms: u64,
        observed_audio_end_ms: u64,
        hypothesis: &H,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<ConsensusReport, ConsensusError> {
        if outputs.capacity() == outputs.len() {
            return Err(ConsensusError::OutputCapacityTooSmall);
        }
        let eligible_end =
            observed_audio_end_ms.saturating_sub(u64::from(self.config.unstable_tail_ms));
        self.stage_hypothesis(
            window_start_ms,
            observed_audio_end_ms,
            eligible_end,
            hypothesis,
        )?;
        self.update_observations();
        let stable = self
            .next
            .iter()
            .take_while(|segment| segment.observations >= self.config.required_observations)
            .count();
        let commit = self.make_commit(stable)?;
        if let Some(commit) = commit {
            outputs.push(commit);
            self.remove_stable_prefix(stable);
        }
        self.install_next();
        Ok(ConsensusReport {
            pending_segments: self.previous.len(),
            pending_bytes: self.previous_text.len(),
            commit_written: stable > 0,
            discard_audio_before_ms: self.discard_audio_before_ms(),
        })
    }

    /// Commits the final hypothesis suffix without allowing it to revise any
    /// already committed boundary.
    ///
    /// # Errors
    ///
    /// Applies the same capacity, size, timestamp, boundary, and allocation
    /// checks as [`Self::observe`].
    pub fn finalize<H: TranscriptHypothesis>(
        &mut self,
        window_start_ms: u64,
        observed_audio_end_ms: u64,
        hypothesis: &H,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<ConsensusReport, ConsensusError> {
        if outputs.capacity() == outputs.len() {
            return Err(ConsensusError::OutputCapacityTooSmall);
        }
        self.stage_hypothesis(
            window_start_ms,
            observed_audio_end_ms,
            observed_audio_end_ms,
            hypothesis,
        )?;
        let count = self.next.len();
        let commit = self.make_commit(count)?;
        if let Some(commit) = commit {
            outputs.push(commit);
        }
        self.clear_pending();
        Ok(ConsensusReport {
            pending_segments: 0,
            pending_bytes: 0,
            commit_written: count > 0,
            discard_audio_before_ms: self.discard_audio_before_ms(),
        })
    }

    /// Erases uncommitted text and starts a new monotonic generation.
    pub fn reset(&mut self) {
        self.clear_pending();
        self.committed_through_ms = 0;
        self.next_sequence = 0;
        self.generation = self.generation.saturating_add(1);
    }

    fn stage_hypothesis<H: TranscriptHypothesis>(
        &mut self,
        window_start_ms: u64,
        observed_audio_end_ms: u64,
        eligible_end_ms: u64,
        hypothesis: &H,
    ) -> Result<(), ConsensusError> {
        self.clear_next();
        let result = self.stage_hypothesis_inner(
            window_start_ms,
            observed_audio_end_ms,
            eligible_end_ms,
            hypothesis,
        );
        if result.is_err() {
            self.clear_next();
        }
        result
    }

    fn stage_hypothesis_inner<H: TranscriptHypothesis>(
        &mut self,
        window_start_ms: u64,
        observed_audio_end_ms: u64,
        eligible_end_ms: u64,
        hypothesis: &H,
    ) -> Result<(), ConsensusError> {
        let count = hypothesis.segment_count();
        if count > MAX_TRANSCRIPT_SEGMENTS {
            return Err(ConsensusError::HypothesisTooLarge);
        }
        let mut total_bytes = 0usize;
        let mut previous_end = window_start_ms;
        for index in 0..count {
            let Some(segment) = hypothesis.segment(index) else {
                self.clear_next();
                return Err(ConsensusError::InvalidHypothesis);
            };
            total_bytes = total_bytes
                .checked_add(segment.text.len())
                .ok_or(ConsensusError::HypothesisTooLarge)?;
            if total_bytes > MAX_TRANSCRIPT_BYTES || segment.text.is_empty() {
                self.clear_next();
                return Err(ConsensusError::HypothesisTooLarge);
            }
            let start_ms = window_start_ms
                .checked_add(u64::from(segment.start_ms))
                .ok_or(ConsensusError::InvalidTimestamp)?;
            let end_ms = window_start_ms
                .checked_add(u64::from(segment.end_ms))
                .ok_or(ConsensusError::InvalidTimestamp)?;
            if start_ms > end_ms || start_ms < previous_end || end_ms > observed_audio_end_ms {
                self.clear_next();
                return Err(ConsensusError::InvalidTimestamp);
            }
            previous_end = end_ms;
            if end_ms <= self.committed_through_ms {
                continue;
            }
            if start_ms < self.committed_through_ms {
                self.clear_next();
                return Err(ConsensusError::CommittedBoundaryOverlap);
            }
            if end_ms > eligible_end_ms {
                continue;
            }
            let byte_start = self.next_text.len();
            self.next_text.extend_from_slice(segment.text.as_bytes());
            self.next.push(CandidateSegment {
                byte_start,
                byte_end: self.next_text.len(),
                start_ms,
                end_ms,
                observations: 1,
            });
        }
        Ok(())
    }

    fn update_observations(&mut self) {
        let mut matching_prefix = true;
        for (index, current) in self.next.iter_mut().enumerate() {
            let Some(previous) = self.previous.get(index) else {
                matching_prefix = false;
                continue;
            };
            let same = matching_prefix
                && current.start_ms == previous.start_ms
                && current.end_ms == previous.end_ms
                && equivalent_whitespace(
                    &self.next_text[current.byte_start..current.byte_end],
                    &self.previous_text[previous.byte_start..previous.byte_end],
                );
            if same {
                current.observations = previous.observations.saturating_add(1);
            } else {
                matching_prefix = false;
            }
        }
    }

    fn make_commit(&mut self, count: usize) -> Result<Option<ConsensusCommit>, ConsensusError> {
        if count == 0 {
            return Ok(None);
        }
        let Some(first) = self.next.first().copied() else {
            return Err(ConsensusError::InvalidHypothesis);
        };
        let Some(last) = self.next.get(count - 1).copied() else {
            return Err(ConsensusError::InvalidHypothesis);
        };
        let mut text = Vec::new();
        let length = last.byte_end.saturating_sub(first.byte_start);
        text.try_reserve_exact(length)
            .map_err(|_| ConsensusError::AllocationFailed)?;
        text.extend_from_slice(&self.next_text[first.byte_start..last.byte_end]);
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.committed_through_ms = last.end_ms;
        Ok(Some(ConsensusCommit {
            text,
            generation: self.generation,
            sequence: self.next_sequence,
            start_ms: first.start_ms,
            end_ms: last.end_ms,
            segments: count,
        }))
    }

    fn remove_stable_prefix(&mut self, count: usize) {
        let removed_bytes = self.next[count - 1].byte_end;
        let retained_bytes = self.next_text.len().saturating_sub(removed_bytes);
        self.next_text.copy_within(removed_bytes.., 0);
        self.next_text[retained_bytes..].fill(0);
        self.next_text.truncate(retained_bytes);
        self.next.drain(..count);
        for segment in &mut self.next {
            segment.byte_start = segment.byte_start.saturating_sub(removed_bytes);
            segment.byte_end = segment.byte_end.saturating_sub(removed_bytes);
        }
    }

    fn install_next(&mut self) {
        self.clear_previous();
        mem::swap(&mut self.previous_text, &mut self.next_text);
        mem::swap(&mut self.previous, &mut self.next);
    }

    fn clear_pending(&mut self) {
        self.clear_previous();
        self.clear_next();
    }

    fn clear_previous(&mut self) {
        self.previous_text.fill(0);
        self.previous_text.clear();
        self.previous.clear();
    }

    fn clear_next(&mut self) {
        self.next_text.fill(0);
        self.next_text.clear();
        self.next.clear();
    }
}

impl Drop for ConsensusCommitter {
    fn drop(&mut self) {
        self.clear_pending();
    }
}

fn equivalent_whitespace(left: &[u8], right: &[u8]) -> bool {
    let Ok(left) = std::str::from_utf8(left) else {
        return false;
    };
    let Ok(right) = std::str::from_utf8(right) else {
        return false;
    };
    left.split_whitespace().eq(right.split_whitespace())
}

/// Payload-free consensus failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsensusError {
    /// The stability or time-margin policy is outside its hard bounds.
    InvalidConfig,
    /// A bounded construction or commit allocation failed.
    AllocationFailed,
    /// The caller output has no spare slot for a possible commit.
    OutputCapacityTooSmall,
    /// The hypothesis has too many segments/bytes or an empty segment.
    HypothesisTooLarge,
    /// The hypothesis view did not return all declared segments.
    InvalidHypothesis,
    /// Segment timestamps overflow, regress, overlap, or exceed observed audio.
    InvalidTimestamp,
    /// A segment straddles an immutable committed timestamp boundary.
    CommittedBoundaryOverlap,
}

impl fmt::Display for ConsensusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidConfig => "invalid consensus configuration",
            Self::AllocationFailed => "consensus allocation failed",
            Self::OutputCapacityTooSmall => "consensus output capacity is too small",
            Self::HypothesisTooLarge => "consensus hypothesis exceeds its bound",
            Self::InvalidHypothesis => "consensus hypothesis is invalid",
            Self::InvalidTimestamp => "consensus hypothesis timestamp is invalid",
            Self::CommittedBoundaryOverlap => "consensus hypothesis overlaps committed text",
        };
        formatter.write_str(message)
    }
}

impl Error for ConsensusError {}

#[cfg(test)]
mod tests {
    use super::*;

    struct Hypothesis<'a>(&'a [HypothesisSegment<'a>]);

    impl TranscriptHypothesis for Hypothesis<'_> {
        fn segment_count(&self) -> usize {
            self.0.len()
        }

        fn segment(&self, index: usize) -> Option<HypothesisSegment<'_>> {
            self.0.get(index).copied()
        }
    }

    fn segment(text: &str, start_ms: u32, end_ms: u32) -> HypothesisSegment<'_> {
        HypothesisSegment {
            text,
            start_ms,
            end_ms,
        }
    }

    fn committer() -> Result<ConsensusCommitter, ConsensusError> {
        ConsensusCommitter::new(ConsensusConfig::new(2, 100, 40)?)
    }

    #[test]
    fn config_rejects_unsafe_observation_and_margin_bounds() {
        assert_eq!(
            ConsensusConfig::new(1, 100, 40),
            Err(ConsensusError::InvalidConfig)
        );
        assert_eq!(
            ConsensusConfig::new(9, 100, 40),
            Err(ConsensusError::InvalidConfig)
        );
        assert_eq!(
            ConsensusConfig::new(2, 100, 101),
            Err(ConsensusError::InvalidConfig)
        );
    }

    #[test]
    fn repeated_stable_prefix_commits_once_and_never_rolls_back() -> Result<(), ConsensusError> {
        let mut engine = committer()?;
        let hypothesis = Hypothesis(&[segment("hello ", 0, 300), segment("world", 300, 600)]);
        let mut outputs = Vec::with_capacity(4);
        assert!(
            !engine
                .observe(0, 1_000, &hypothesis, &mut outputs)?
                .commit_written
        );
        assert!(
            engine
                .observe(0, 1_000, &hypothesis, &mut outputs)?
                .commit_written
        );
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].text(), "hello world");
        assert_eq!(outputs[0].sequence(), 1);
        assert_eq!(outputs[0].segments(), 2);
        assert_eq!(engine.pending_text(), "");

        assert!(
            !engine
                .observe(0, 1_000, &hypothesis, &mut outputs)?
                .commit_written
        );
        assert_eq!(outputs.len(), 1);
        assert_eq!(engine.discard_audio_before_ms(), 560);
        Ok(())
    }

    #[test]
    fn changed_suffix_does_not_reset_the_stable_leading_segment() -> Result<(), ConsensusError> {
        let mut engine = committer()?;
        let first = Hypothesis(&[segment("stable ", 0, 300), segment("draft", 300, 600)]);
        let changed = Hypothesis(&[segment("stable ", 0, 300), segment("revised", 300, 650)]);
        let mut outputs = Vec::with_capacity(2);
        engine.observe(0, 1_000, &first, &mut outputs)?;
        let report = engine.observe(0, 1_000, &changed, &mut outputs)?;
        assert!(report.commit_written);
        assert_eq!(outputs[0].text(), "stable ");
        assert_eq!(engine.pending_text(), "revised");
        Ok(())
    }

    #[test]
    fn unstable_trailing_margin_delays_commit() -> Result<(), ConsensusError> {
        let mut engine = committer()?;
        let current = Hypothesis(&[segment("near tail", 0, 950)]);
        let mut outputs = Vec::with_capacity(2);
        engine.observe(0, 1_000, &current, &mut outputs)?;
        engine.observe(0, 1_000, &current, &mut outputs)?;
        assert!(outputs.is_empty());
        engine.observe(0, 1_100, &current, &mut outputs)?;
        assert!(outputs.is_empty());
        engine.observe(0, 1_100, &current, &mut outputs)?;
        assert_eq!(outputs[0].text(), "near tail");
        Ok(())
    }

    #[test]
    fn comparison_normalizes_whitespace_but_commit_preserves_latest_display(
    ) -> Result<(), ConsensusError> {
        let mut engine = committer()?;
        let first = Hypothesis(&[segment("hello   world", 0, 300)]);
        let second = Hypothesis(&[segment("hello world", 0, 300)]);
        let mut outputs = Vec::with_capacity(1);
        engine.observe(0, 500, &first, &mut outputs)?;
        engine.observe(0, 500, &second, &mut outputs)?;
        assert_eq!(outputs[0].text(), "hello world");
        Ok(())
    }

    #[test]
    fn final_hypothesis_revises_only_the_uncommitted_suffix() -> Result<(), ConsensusError> {
        let mut engine = committer()?;
        let stable = Hypothesis(&[segment("fixed ", 0, 300)]);
        let mut outputs = Vec::with_capacity(3);
        engine.observe(0, 500, &stable, &mut outputs)?;
        engine.observe(0, 500, &stable, &mut outputs)?;
        let final_hypothesis = Hypothesis(&[segment("fixed ", 0, 300), segment("final", 300, 480)]);
        let report = engine.finalize(0, 500, &final_hypothesis, &mut outputs)?;
        assert!(report.commit_written);
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0].text(), "fixed ");
        assert_eq!(outputs[1].text(), "final");
        assert_eq!(outputs[1].sequence(), 2);
        Ok(())
    }

    #[test]
    fn invalid_boundary_overlap_is_atomic() -> Result<(), ConsensusError> {
        let mut engine = committer()?;
        let stable = Hypothesis(&[segment("fixed", 0, 300)]);
        let mut outputs = Vec::with_capacity(3);
        engine.observe(0, 500, &stable, &mut outputs)?;
        engine.observe(0, 500, &stable, &mut outputs)?;
        let overlap = Hypothesis(&[segment("crosses", 250, 400)]);
        assert_eq!(
            engine.observe(0, 500, &overlap, &mut outputs),
            Err(ConsensusError::CommittedBoundaryOverlap)
        );
        assert_eq!(outputs.len(), 1);
        assert_eq!(engine.pending_text(), "");
        Ok(())
    }

    #[test]
    fn reset_erases_pending_text_and_changes_generation() -> Result<(), ConsensusError> {
        let mut engine = committer()?;
        let pending = Hypothesis(&[segment("sensitive pending", 0, 300)]);
        let mut outputs = Vec::with_capacity(1);
        engine.observe(0, 500, &pending, &mut outputs)?;
        assert_eq!(engine.pending_text(), "sensitive pending");
        engine.reset();
        assert_eq!(engine.pending_text(), "");
        assert_eq!(engine.generation(), 1);
        assert!(engine.previous_text.is_empty());
        assert!(engine.next_text.is_empty());
        Ok(())
    }

    #[test]
    fn invalid_timestamps_and_capacity_fail_without_consuming_state() -> Result<(), ConsensusError>
    {
        let mut engine = committer()?;
        let pending = Hypothesis(&[segment("pending", 0, 300)]);
        let mut outputs = Vec::with_capacity(1);
        engine.observe(0, 500, &pending, &mut outputs)?;
        let no_capacity = Vec::new();
        let mut no_capacity = no_capacity;
        assert_eq!(
            engine.observe(0, 500, &pending, &mut no_capacity),
            Err(ConsensusError::OutputCapacityTooSmall)
        );
        let invalid = Hypothesis(&[segment("bad", 400, 300)]);
        assert_eq!(
            engine.observe(0, 500, &invalid, &mut outputs),
            Err(ConsensusError::InvalidTimestamp)
        );
        assert_eq!(engine.pending_text(), "pending");
        Ok(())
    }

    #[test]
    fn segment_and_text_bounds_fail_atomically() -> Result<(), ConsensusError> {
        let mut engine = committer()?;
        let pending = Hypothesis(&[segment("pending", 0, 300)]);
        let mut outputs = Vec::with_capacity(1);
        engine.observe(0, 500, &pending, &mut outputs)?;

        let too_many_segments = vec![segment("x", 0, 0); MAX_TRANSCRIPT_SEGMENTS + 1];
        assert_eq!(
            engine.observe(0, 500, &Hypothesis(&too_many_segments), &mut outputs),
            Err(ConsensusError::HypothesisTooLarge)
        );
        let oversized_text = "x".repeat(MAX_TRANSCRIPT_BYTES + 1);
        let oversized = Hypothesis(&[segment(&oversized_text, 0, 300)]);
        assert_eq!(
            engine.observe(0, 500, &oversized, &mut outputs),
            Err(ConsensusError::HypothesisTooLarge)
        );
        assert_eq!(engine.pending_text(), "pending");
        assert!(engine.next_text.is_empty());
        Ok(())
    }
}
