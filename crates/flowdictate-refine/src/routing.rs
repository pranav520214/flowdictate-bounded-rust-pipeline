/// Non-sensitive upstream signals that may justify optional local editing.
///
/// The router deliberately accepts only metadata. Transcript content remains
/// outside the routing boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefinementNeed {
    /// Meaning must change rather than only punctuation or spacing.
    SemanticRewrite,
    /// A self-correction cannot be resolved deterministically.
    AmbiguousSelfCorrection,
    /// Requested formatting depends on surrounding meaning.
    ContextualFormatting,
    /// Deterministic rules did not meet their configured confidence policy.
    LowDeterministicConfidence,
}

/// Compact set of non-sensitive routing conditions.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RefinementSignals(u8);

impl RefinementSignals {
    /// Creates a signal set with no need for semantic editing.
    #[must_use]
    pub const fn none() -> Self {
        Self(0)
    }

    /// Creates a signal set containing one named routing condition.
    #[must_use]
    pub const fn from_need(need: RefinementNeed) -> Self {
        Self(need.mask())
    }

    /// Adds a named routing condition without allocating.
    #[must_use]
    pub const fn with(self, need: RefinementNeed) -> Self {
        Self(self.0 | need.mask())
    }

    /// Reports whether at least one condition requires semantic editing.
    #[must_use]
    pub const fn requires_editor(self) -> bool {
        self.0 != 0
    }
}

impl RefinementNeed {
    const fn mask(self) -> u8 {
        1 << self as u8
    }
}

/// Explicit policy and readiness gate for an optional local editor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocalEditorGate {
    /// Local model use is not permitted by current policy.
    #[default]
    Disabled,
    /// Local editing is permitted but the runtime is not ready.
    Unavailable,
    /// Local editing is permitted and its runtime is ready.
    Ready,
}

/// Selected refinement path. Every path returns to the validated model-free
/// pipeline before output can be inserted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefinementRoute {
    /// Use deterministic cleanup and sanitized-raw fallback only.
    ModelFree,
    /// Try the ready local editor, then validate or use model-free fallback.
    LocalEditorThenModelFree,
}

/// Selects the least-privileged refinement route from metadata alone.
#[must_use]
pub const fn select_refinement_route(
    signals: RefinementSignals,
    editor: LocalEditorGate,
) -> RefinementRoute {
    if signals.requires_editor() && matches!(editor, LocalEditorGate::Ready) {
        RefinementRoute::LocalEditorThenModelFree
    } else {
        RefinementRoute::ModelFree
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_model_free() {
        assert_eq!(RefinementSignals::default(), RefinementSignals::none());
        assert_eq!(LocalEditorGate::default(), LocalEditorGate::Disabled);
        assert_eq!(
            select_refinement_route(RefinementSignals::default(), LocalEditorGate::default()),
            RefinementRoute::ModelFree
        );
    }

    #[test]
    fn a_ready_editor_without_need_stays_model_free() {
        assert_eq!(
            select_refinement_route(RefinementSignals::none(), LocalEditorGate::Ready),
            RefinementRoute::ModelFree
        );
    }

    #[test]
    fn each_documented_condition_can_select_a_ready_editor() {
        let needs = [
            RefinementNeed::SemanticRewrite,
            RefinementNeed::AmbiguousSelfCorrection,
            RefinementNeed::ContextualFormatting,
            RefinementNeed::LowDeterministicConfidence,
        ];

        for need in needs {
            let signal = RefinementSignals::from_need(need);
            assert!(signal.requires_editor());
            assert_eq!(
                select_refinement_route(signal, LocalEditorGate::Ready),
                RefinementRoute::LocalEditorThenModelFree
            );
        }
    }

    #[test]
    fn disabled_or_unavailable_editors_fail_closed() {
        let signals = RefinementSignals::none()
            .with(RefinementNeed::SemanticRewrite)
            .with(RefinementNeed::AmbiguousSelfCorrection)
            .with(RefinementNeed::ContextualFormatting)
            .with(RefinementNeed::LowDeterministicConfidence);

        for editor in [LocalEditorGate::Disabled, LocalEditorGate::Unavailable] {
            assert_eq!(
                select_refinement_route(signals, editor),
                RefinementRoute::ModelFree
            );
        }
    }

    #[test]
    fn empty_signals_do_not_require_an_editor() {
        assert!(!RefinementSignals::none().requires_editor());
    }
}
