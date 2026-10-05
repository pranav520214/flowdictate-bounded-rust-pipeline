//! Application-facing selection policy for local ASR backends.
//!
//! This module deliberately owns neither microphone consent nor capture. It
//! gives a future UI a versioned disclosure and requires an opaque, explicit
//! acknowledgement before the experimental backend can be selected.

use std::{error::Error, fmt};

/// Current version of the facts a user must see before enabling Nemotron.
pub const EXPERIMENTAL_NEMOTRON_DISCLOSURE_VERSION: u16 = 1;

/// Exact local model size disclosed for the pinned experimental artifact.
pub const EXPERIMENTAL_NEMOTRON_MODEL_BYTES: u64 = 741_548_352;

/// Peak process working set measured by the reviewed feasibility probe.
pub const EXPERIMENTAL_NEMOTRON_MEASURED_PEAK_BYTES: u64 = 976_093_184;

/// A local ASR backend choice understood by the future application shell.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocalAsrBackendSelection {
    /// The existing supported local Whisper path.
    #[default]
    ProductionWhisper,
    /// The pinned Hindi-only native Nemotron feasibility path.
    ExperimentalNemotronHindi,
}

impl LocalAsrBackendSelection {
    /// Returns a stable, non-sensitive identifier suitable for local settings.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ProductionWhisper => "production-whisper",
            Self::ExperimentalNemotronHindi => "experimental-nemotron-hi-in",
        }
    }

    /// Reports whether this choice is outside the supported production path.
    #[must_use]
    pub const fn is_experimental(self) -> bool {
        matches!(self, Self::ExperimentalNemotronHindi)
    }
}

/// Versioned, user-visible facts required by the experimental opt-in UI.
///
/// The facts contain no identity, activity, transcript, or acknowledgement
/// timestamp. A product shell may render them but must not imply that accepting
/// them grants microphone/listening consent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExperimentalNemotronDisclosure {
    version: u16,
}

impl ExperimentalNemotronDisclosure {
    /// Returns the current immutable disclosure contract.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            version: EXPERIMENTAL_NEMOTRON_DISCLOSURE_VERSION,
        }
    }

    /// Returns the disclosure version that must be acknowledged.
    #[must_use]
    pub const fn version(self) -> u16 {
        self.version
    }

    /// Returns the experimental model's pinned language scope.
    #[must_use]
    pub const fn language(self) -> &'static str {
        "hi-IN"
    }

    /// Returns the exact pinned model size in bytes.
    #[must_use]
    pub const fn model_bytes(self) -> u64 {
        EXPERIMENTAL_NEMOTRON_MODEL_BYTES
    }

    /// Returns the measured peak process working set in bytes.
    #[must_use]
    pub const fn measured_peak_working_set_bytes(self) -> u64 {
        EXPERIMENTAL_NEMOTRON_MEASURED_PEAK_BYTES
    }

    /// Reports that inference stays on the local device.
    #[must_use]
    pub const fn is_local_only(self) -> bool {
        true
    }

    /// Reports whether the reviewed platform gate is available in this build.
    #[must_use]
    pub const fn is_available_on_this_platform(self) -> bool {
        cfg!(windows)
    }

    /// Reports that accepting this disclosure never authorizes listening.
    #[must_use]
    pub const fn authorizes_microphone(self) -> bool {
        false
    }
}

/// Unforgeable proof that the current experimental disclosure was accepted.
///
/// This value is intentionally not cloneable and carries no user identifier or
/// sensitive event metadata. It is consumed when the selection is enabled.
pub struct ExperimentalNemotronOptIn {
    _private: (),
}

/// Records an explicit response to the current disclosure in volatile memory.
///
/// # Errors
///
/// Returns [`BackendSelectionError::DisclosureDeclined`] unless `accepted` is
/// true, or [`BackendSelectionError::StaleDisclosure`] when a caller presents
/// any version other than the current one.
pub const fn acknowledge_experimental_nemotron(
    disclosure_version: u16,
    accepted: bool,
) -> Result<ExperimentalNemotronOptIn, BackendSelectionError> {
    if disclosure_version != EXPERIMENTAL_NEMOTRON_DISCLOSURE_VERSION {
        return Err(BackendSelectionError::StaleDisclosure);
    }
    if !accepted {
        return Err(BackendSelectionError::DisclosureDeclined);
    }
    Ok(ExperimentalNemotronOptIn { _private: () })
}

/// Volatile application-composition policy for local ASR selection.
///
/// The default is always production Whisper. This type stores no durable
/// consent record and exposes no capture or microphone capability.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct LocalAsrSelectionPolicy {
    selected: LocalAsrBackendSelection,
}

impl LocalAsrSelectionPolicy {
    /// Returns the currently selected backend.
    #[must_use]
    pub const fn selected(&self) -> LocalAsrBackendSelection {
        self.selected
    }

    /// Reports that a separate listening consent is still required.
    #[must_use]
    pub const fn requires_listening_consent(&self) -> bool {
        true
    }

    /// Consumes a current opt-in and selects the experimental backend.
    ///
    /// # Errors
    ///
    /// Fails closed on platforms without the reviewed immutable model-opening
    /// boundary. The selection remains unchanged on failure.
    pub fn enable_experimental_nemotron(
        &mut self,
        _opt_in: ExperimentalNemotronOptIn,
    ) -> Result<BackendSelectionChange, BackendSelectionError> {
        if !cfg!(windows) {
            return Err(BackendSelectionError::UnsupportedPlatform);
        }
        Ok(self.replace(LocalAsrBackendSelection::ExperimentalNemotronHindi))
    }

    /// Withdraws experimental selection and immediately returns to Whisper.
    pub fn withdraw_experimental(&mut self) -> BackendSelectionChange {
        self.replace(LocalAsrBackendSelection::ProductionWhisper)
    }

    fn replace(&mut self, selected: LocalAsrBackendSelection) -> BackendSelectionChange {
        let changed = self.selected != selected;
        self.selected = selected;
        BackendSelectionChange { selected, changed }
    }
}

/// Non-sensitive result of a local backend selection change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackendSelectionChange {
    selected: LocalAsrBackendSelection,
    changed: bool,
}

impl BackendSelectionChange {
    /// Returns the selection after the change.
    #[must_use]
    pub const fn selected(self) -> LocalAsrBackendSelection {
        self.selected
    }

    /// Returns whether the selection actually changed.
    #[must_use]
    pub const fn changed(self) -> bool {
        self.changed
    }
}

/// Payload-free selection policy failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendSelectionError {
    /// The user did not accept the experimental disclosure.
    DisclosureDeclined,
    /// The acknowledgement did not reference the current disclosure version.
    StaleDisclosure,
    /// This build lacks the reviewed immutable model-opening boundary.
    UnsupportedPlatform,
}

impl fmt::Display for BackendSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::DisclosureDeclined => "experimental backend disclosure was declined",
            Self::StaleDisclosure => "experimental backend disclosure is stale",
            Self::UnsupportedPlatform => "experimental backend is unsupported on this platform",
        };
        formatter.write_str(message)
    }
}

impl Error for BackendSelectionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_production_whisper_and_still_requires_listening_consent() {
        let policy = LocalAsrSelectionPolicy::default();

        assert_eq!(
            policy.selected(),
            LocalAsrBackendSelection::ProductionWhisper
        );
        assert_eq!(policy.selected().code(), "production-whisper");
        assert!(!policy.selected().is_experimental());
        assert!(policy.requires_listening_consent());
    }

    #[test]
    fn disclosure_exposes_only_fixed_product_facts() {
        let disclosure = ExperimentalNemotronDisclosure::current();

        assert_eq!(disclosure.version(), 1);
        assert_eq!(disclosure.language(), "hi-IN");
        assert_eq!(disclosure.model_bytes(), 741_548_352);
        assert_eq!(disclosure.measured_peak_working_set_bytes(), 976_093_184);
        assert!(disclosure.is_local_only());
        assert_eq!(disclosure.is_available_on_this_platform(), cfg!(windows));
        assert!(!disclosure.authorizes_microphone());
    }

    #[test]
    fn denial_and_stale_versions_cannot_create_opt_in() {
        assert!(matches!(
            acknowledge_experimental_nemotron(EXPERIMENTAL_NEMOTRON_DISCLOSURE_VERSION, false),
            Err(BackendSelectionError::DisclosureDeclined)
        ));
        assert!(matches!(
            acknowledge_experimental_nemotron(
                EXPERIMENTAL_NEMOTRON_DISCLOSURE_VERSION.saturating_add(1),
                true
            ),
            Err(BackendSelectionError::StaleDisclosure)
        ));
    }

    #[test]
    fn current_opt_in_selects_only_on_reviewed_platform() -> Result<(), BackendSelectionError> {
        let disclosure = ExperimentalNemotronDisclosure::current();
        let opt_in = acknowledge_experimental_nemotron(disclosure.version(), true)?;
        let mut policy = LocalAsrSelectionPolicy::default();
        let result = policy.enable_experimental_nemotron(opt_in);

        if cfg!(windows) {
            let change = result?;
            assert!(change.changed());
            assert_eq!(
                change.selected(),
                LocalAsrBackendSelection::ExperimentalNemotronHindi
            );
            assert!(policy.selected().is_experimental());
            assert!(policy.requires_listening_consent());
        } else {
            assert!(matches!(
                result,
                Err(BackendSelectionError::UnsupportedPlatform)
            ));
            assert_eq!(
                policy.selected(),
                LocalAsrBackendSelection::ProductionWhisper
            );
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn withdrawal_is_immediate_idempotent_and_returns_to_whisper(
    ) -> Result<(), BackendSelectionError> {
        let opt_in =
            acknowledge_experimental_nemotron(EXPERIMENTAL_NEMOTRON_DISCLOSURE_VERSION, true)?;
        let mut policy = LocalAsrSelectionPolicy::default();
        policy.enable_experimental_nemotron(opt_in)?;

        let first = policy.withdraw_experimental();
        let second = policy.withdraw_experimental();

        assert!(first.changed());
        assert_eq!(
            first.selected(),
            LocalAsrBackendSelection::ProductionWhisper
        );
        assert!(!second.changed());
        assert_eq!(
            policy.selected(),
            LocalAsrBackendSelection::ProductionWhisper
        );
        Ok(())
    }
}
