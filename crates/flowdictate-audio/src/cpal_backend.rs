//! Narrow CPAL boundary. Device metadata never enters errors or logs.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, SampleFormat, Stream, StreamConfig, SupportedStreamConfig,
};

use crate::{
    bounded_audio_ring, AudioConsumer, AudioFormat, BufferError, CapturePlan, CaptureSampleFormat,
    CaptureWrite, DEFAULT_RING_DURATION, MAX_SAMPLE_RATE_HZ, MIN_SAMPLE_RATE_HZ,
};

/// User-visible microphone metadata returned only to an explicit settings UI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MicrophoneInfo {
    /// Ephemeral enumeration index; it is not a stable user/device identifier.
    pub ordinal: usize,
    /// Human-readable OS-provided label. It must not be logged.
    pub display_name: String,
    /// Whether the OS currently reports this device as the default input.
    pub is_default: bool,
}

/// Enumerates current input devices without opening or recording from them.
///
/// # Errors
///
/// Returns a payload-free error if the OS backend cannot enumerate inputs.
pub fn enumerate_microphones() -> Result<Vec<MicrophoneInfo>, PlatformCaptureError> {
    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|device| device.id().ok());
    let devices = host
        .input_devices()
        .map_err(|_| PlatformCaptureError::DeviceEnumerationFailed)?;
    let mut microphones = Vec::new();
    for (ordinal, device) in devices.enumerate() {
        let display_name = device.description().map_or_else(
            |_| "Unavailable microphone".to_owned(),
            |description| description.name().to_owned(),
        );
        let is_default = device.id().ok().as_ref() == default_id.as_ref();
        microphones.push(MicrophoneInfo {
            ordinal,
            display_name,
            is_default,
        });
    }
    Ok(microphones)
}

/// Builds a paused stream for the OS default input device.
///
/// The caller must invoke [`CaptureStream::resume`] only in response to the
/// approved listening interaction. Building the stream allocates the complete
/// two-second ring before any callback can run.
///
/// # Errors
///
/// Returns a payload-free platform error and never falls back to a remote or
/// file-based audio source.
pub fn build_default_capture() -> Result<(CaptureStream, AudioConsumer), PlatformCaptureError> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or(PlatformCaptureError::NoDefaultInputDevice)?;
    let supported = select_supported_config(&device)?;
    let sample_format = map_sample_format(supported.sample_format())?;
    let audio_format = AudioFormat::new(supported.sample_rate(), supported.channels())
        .map_err(|_| PlatformCaptureError::UnsupportedDefaultFormat)?;
    let plan = CapturePlan::new(audio_format, sample_format, DEFAULT_RING_DURATION)
        .map_err(|_| PlatformCaptureError::InvalidCapturePlan)?;
    let (producer, consumer) =
        bounded_audio_ring(plan.ring_capacity_samples()).map_err(map_buffer_error)?;
    let health = CaptureHealth::default();
    let config: StreamConfig = supported.into();
    let stream = build_stream(&device, config, sample_format, producer, health.clone())?;
    Ok((
        CaptureStream {
            stream,
            plan,
            health,
        },
        consumer,
    ))
}

fn select_supported_config(device: &Device) -> Result<SupportedStreamConfig, PlatformCaptureError> {
    let configs = device
        .supported_input_configs()
        .map_err(|_| PlatformCaptureError::ConfigurationUnavailable)?;
    let mut best: Option<(u8, SupportedStreamConfig)> = None;
    for range in configs {
        if !(1..=2).contains(&range.channels()) {
            continue;
        }
        let format_score = match range.sample_format() {
            SampleFormat::F32 => 30,
            SampleFormat::I16 => 20,
            SampleFormat::U16 => 10,
            _ => continue,
        };
        let selected_rate = choose_supported_rate(range.min_sample_rate(), range.max_sample_rate());
        let selected = selected_rate.and_then(|rate| range.try_with_sample_rate(rate));
        let Some(config) = selected else {
            continue;
        };
        let channel_score = u8::from(config.channels() == 1);
        let score = format_score + channel_score;
        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score)
        {
            best = Some((score, config));
        }
    }
    best.map(|(_, config)| config)
        .ok_or(PlatformCaptureError::UnsupportedDefaultFormat)
}

/// Chooses a rate that stays inside the product's hard safety bounds.
///
/// Common speech rates are preferred for resampling quality and predictable
/// behavior. If a device advertises a different rate, the lowest rate in the
/// safe intersection is accepted instead of rejecting an otherwise usable
/// microphone.
fn choose_supported_rate(min_rate: u32, max_rate: u32) -> Option<u32> {
    let lower = min_rate.max(MIN_SAMPLE_RATE_HZ);
    let upper = max_rate.min(MAX_SAMPLE_RATE_HZ);
    if lower > upper {
        return None;
    }
    [48_000, 44_100, 16_000, 8_000]
        .into_iter()
        .find(|rate| (*rate >= lower) && (*rate <= upper))
        .or(Some(lower))
}

fn map_sample_format(format: SampleFormat) -> Result<CaptureSampleFormat, PlatformCaptureError> {
    match format {
        SampleFormat::F32 => Ok(CaptureSampleFormat::F32),
        SampleFormat::I16 => Ok(CaptureSampleFormat::I16),
        SampleFormat::U16 => Ok(CaptureSampleFormat::U16),
        _ => Err(PlatformCaptureError::UnsupportedDefaultFormat),
    }
}

fn build_stream(
    device: &Device,
    config: StreamConfig,
    sample_format: CaptureSampleFormat,
    mut producer: crate::CaptureProducer,
    health: CaptureHealth,
) -> Result<Stream, PlatformCaptureError> {
    let error_health = health.clone();
    let stream = match sample_format {
        CaptureSampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _| health.record(producer.try_push_f32(data)),
            move |_| error_health.record_stream_error(),
            None,
        ),
        CaptureSampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _| health.record(producer.try_push_i16(data)),
            move |_| error_health.record_stream_error(),
            None,
        ),
        CaptureSampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _| health.record(producer.try_push_u16(data)),
            move |_| error_health.record_stream_error(),
            None,
        ),
    };
    stream.map_err(|_| PlatformCaptureError::StreamBuildFailed)
}

fn map_buffer_error(error: BufferError) -> PlatformCaptureError {
    match error {
        BufferError::ZeroCapacity => PlatformCaptureError::InvalidCapturePlan,
    }
}

/// A paused/playing platform stream and its non-sensitive health handle.
pub struct CaptureStream {
    stream: Stream,
    plan: CapturePlan,
    health: CaptureHealth,
}

impl CaptureStream {
    /// Returns the prevalidated allocation/format plan.
    #[must_use]
    pub const fn plan(&self) -> CapturePlan {
        self.plan
    }

    /// Returns a cloneable, payload-free health counter handle.
    #[must_use]
    pub fn health(&self) -> CaptureHealth {
        self.health.clone()
    }

    /// Starts/resumes microphone callbacks.
    ///
    /// # Errors
    ///
    /// Returns a payload-free error if the OS denies or loses the stream.
    pub fn resume(&self) -> Result<(), PlatformCaptureError> {
        self.stream
            .play()
            .map_err(|_| PlatformCaptureError::StreamStartFailed)
    }

    /// Pauses microphone callbacks without persisting buffered audio.
    ///
    /// # Errors
    ///
    /// Returns a payload-free error if the OS cannot pause the stream.
    pub fn pause(&self) -> Result<(), PlatformCaptureError> {
        self.stream
            .pause()
            .map_err(|_| PlatformCaptureError::StreamPauseFailed)
    }
}

/// Cloneable counters that contain no audio or device metadata.
#[derive(Clone, Default)]
pub struct CaptureHealth {
    counters: Arc<CaptureCounters>,
}

impl CaptureHealth {
    fn record(&self, outcome: CaptureWrite) {
        self.counters
            .callback_batches
            .fetch_add(1, Ordering::Relaxed);
        match outcome {
            CaptureWrite::Written {
                samples,
                sanitized_samples,
                ..
            } => {
                self.counters
                    .written_samples
                    .fetch_add(saturating_u64(samples), Ordering::Relaxed);
                self.counters
                    .sanitized_samples
                    .fetch_add(saturating_u64(sanitized_samples), Ordering::Relaxed);
            }
            CaptureWrite::Dropped { samples, .. } => {
                self.counters
                    .dropped_batches
                    .fetch_add(1, Ordering::Relaxed);
                self.counters
                    .dropped_samples
                    .fetch_add(saturating_u64(samples), Ordering::Relaxed);
            }
        }
    }

    fn record_stream_error(&self) {
        self.counters.stream_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Takes an atomic, payload-free operational snapshot.
    #[must_use]
    pub fn snapshot(&self) -> CaptureHealthSnapshot {
        CaptureHealthSnapshot {
            callback_batches: self.counters.callback_batches.load(Ordering::Relaxed),
            written_samples: self.counters.written_samples.load(Ordering::Relaxed),
            sanitized_samples: self.counters.sanitized_samples.load(Ordering::Relaxed),
            dropped_batches: self.counters.dropped_batches.load(Ordering::Relaxed),
            dropped_samples: self.counters.dropped_samples.load(Ordering::Relaxed),
            stream_errors: self.counters.stream_errors.load(Ordering::Relaxed),
        }
    }
}

#[derive(Default)]
struct CaptureCounters {
    callback_batches: AtomicU64,
    written_samples: AtomicU64,
    sanitized_samples: AtomicU64,
    dropped_batches: AtomicU64,
    dropped_samples: AtomicU64,
    stream_errors: AtomicU64,
}

/// Non-sensitive local capture metrics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureHealthSnapshot {
    /// Callback batches observed.
    pub callback_batches: u64,
    /// Interleaved samples committed to the bounded ring.
    pub written_samples: u64,
    /// Non-finite/out-of-range samples replaced or clamped.
    pub sanitized_samples: u64,
    /// Complete callback batches dropped on overflow.
    pub dropped_batches: u64,
    /// Interleaved samples dropped on overflow.
    pub dropped_samples: u64,
    /// Payload-free CPAL stream errors.
    pub stream_errors: u64,
}

/// Payload-free platform microphone failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformCaptureError {
    /// The audio host could not enumerate input devices.
    DeviceEnumerationFailed,
    /// The OS reports no default input device.
    NoDefaultInputDevice,
    /// Supported configurations could not be queried.
    ConfigurationUnavailable,
    /// No safe mono/stereo F32/I16/U16 configuration was found.
    UnsupportedDefaultFormat,
    /// A bounded ring plan could not be created.
    InvalidCapturePlan,
    /// CPAL could not build the paused input stream.
    StreamBuildFailed,
    /// The OS refused to start/resume the stream.
    StreamStartFailed,
    /// The OS refused to pause the stream.
    StreamPauseFailed,
}

impl std::fmt::Display for PlatformCaptureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::DeviceEnumerationFailed => "microphone enumeration failed",
            Self::NoDefaultInputDevice => "no default microphone is available",
            Self::ConfigurationUnavailable => "microphone configuration is unavailable",
            Self::UnsupportedDefaultFormat => "microphone has no supported safe format",
            Self::InvalidCapturePlan => "microphone capture plan is invalid",
            Self::StreamBuildFailed => "microphone stream build failed",
            Self::StreamStartFailed => "microphone stream start failed",
            Self::StreamPauseFailed => "microphone stream pause failed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for PlatformCaptureError {}

fn saturating_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::choose_supported_rate;

    #[test]
    fn prefers_common_rate_inside_device_range() {
        assert_eq!(choose_supported_rate(32_000, 48_000), Some(48_000));
    }

    #[test]
    fn falls_back_to_safe_nonstandard_rate() {
        assert_eq!(choose_supported_rate(32_000, 32_000), Some(32_000));
    }

    #[test]
    fn rejects_ranges_outside_hard_bounds() {
        assert_eq!(choose_supported_rate(1_000, 7_999), None);
        assert_eq!(choose_supported_rate(192_001, 384_000), None);
    }
}
