//! Consent-first human microphone probe. It never prints device labels or audio.

use std::{
    env,
    error::Error,
    fmt,
    io::{self, Read, Write},
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};

use flowdictate_audio::{build_default_capture, AudioConsumer, PlatformCaptureError};
use flowdictate_capture_probe::{
    ConsentError, ConsentToken, ProbeDuration, ProbeDurationError, ProbeReport, CONSENT_NOTICE,
};

const DEFAULT_PROBE_SECONDS: u64 = 10;
const CONSENT_BUFFER_BYTES: usize = 32;
const DRAIN_BUFFER_SAMPLES: usize = 16_384;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = writeln!(io::stderr().lock(), "probe_error={error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), ProbeRunError> {
    let duration = parse_duration()?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{CONSENT_NOTICE}").map_err(|_| ProbeRunError::OutputUnavailable)?;
    write!(stdout, "consent> ").map_err(|_| ProbeRunError::OutputUnavailable)?;
    stdout
        .flush()
        .map_err(|_| ProbeRunError::OutputUnavailable)?;

    let mut stdin = io::stdin().lock();
    let _consent = read_consent(&mut stdin)?;
    let (stream, mut consumer) = build_default_capture().map_err(ProbeRunError::CaptureBuild)?;
    let plan = stream.plan();
    let health = stream.health();
    let mut drain_buffer = vec![0.0; DRAIN_BUFFER_SAMPLES].into_boxed_slice();
    stream.resume().map_err(ProbeRunError::CaptureStart)?;

    let (drained_samples, discontinuity_epoch) =
        drain_for(duration, &mut consumer, &mut drain_buffer);
    stream.pause().map_err(ProbeRunError::CapturePause)?;
    let (final_drained, final_epoch) = drain_available(
        &mut consumer,
        &mut drain_buffer,
        drained_samples,
        discontinuity_epoch,
    );
    let report = ProbeReport::new(
        duration,
        plan,
        health.snapshot(),
        final_drained,
        final_epoch,
    );
    writeln!(stdout, "{report}").map_err(|_| ProbeRunError::OutputUnavailable)
}

fn parse_duration() -> Result<ProbeDuration, ProbeRunError> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let seconds = match arguments.next() {
        None => DEFAULT_PROBE_SECONDS,
        Some(value) => value
            .to_str()
            .and_then(|text| text.parse::<u64>().ok())
            .ok_or(ProbeRunError::InvalidArguments)?,
    };
    if arguments.next().is_some() {
        return Err(ProbeRunError::InvalidArguments);
    }
    ProbeDuration::new(seconds).map_err(ProbeRunError::InvalidDuration)
}

fn read_consent(reader: &mut impl Read) -> Result<ConsentToken, ProbeRunError> {
    let mut input = [0_u8; CONSENT_BUFFER_BYTES];
    let mut length = 0;
    loop {
        let mut byte = [0_u8; 1];
        let read = reader
            .read(&mut byte)
            .map_err(|_| ProbeRunError::InputUnavailable)?;
        if read == 0 || byte[0] == b'\n' {
            break;
        }
        if length == input.len() {
            return Err(ProbeRunError::ConsentDenied(ConsentError));
        }
        input[length] = byte[0];
        length += 1;
    }
    ConsentToken::parse(&input[..length]).map_err(ProbeRunError::ConsentDenied)
}

fn drain_for(
    duration: ProbeDuration,
    consumer: &mut AudioConsumer,
    buffer: &mut [f32],
) -> (u64, u64) {
    let deadline = Instant::now() + duration.as_duration();
    let mut drained_samples = 0_u64;
    let mut discontinuity_epoch = 0_u64;
    while Instant::now() < deadline {
        let report = consumer.read(buffer);
        drained_samples = drained_samples.saturating_add(saturating_u64(report.samples_read));
        discontinuity_epoch = report.discontinuity_epoch;
        buffer[..report.samples_read].fill(0.0);
        if report.samples_read == 0 {
            thread::sleep(Duration::from_millis(2));
        }
    }
    (drained_samples, discontinuity_epoch)
}

fn drain_available(
    consumer: &mut AudioConsumer,
    buffer: &mut [f32],
    mut drained_samples: u64,
    mut discontinuity_epoch: u64,
) -> (u64, u64) {
    loop {
        let report = consumer.read(buffer);
        if report.samples_read == 0 {
            return (drained_samples, discontinuity_epoch);
        }
        drained_samples = drained_samples.saturating_add(saturating_u64(report.samples_read));
        discontinuity_epoch = report.discontinuity_epoch;
        buffer[..report.samples_read].fill(0.0);
    }
}

fn saturating_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[derive(Debug)]
enum ProbeRunError {
    InvalidArguments,
    InvalidDuration(ProbeDurationError),
    InputUnavailable,
    ConsentDenied(ConsentError),
    CaptureBuild(PlatformCaptureError),
    CaptureStart(PlatformCaptureError),
    CapturePause(PlatformCaptureError),
    OutputUnavailable,
}

impl fmt::Display for ProbeRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments => formatter.write_str("invalid_arguments"),
            Self::InvalidDuration(_) => formatter.write_str("invalid_duration"),
            Self::InputUnavailable => formatter.write_str("input_unavailable"),
            Self::ConsentDenied(_) => formatter.write_str("consent_denied"),
            Self::CaptureBuild(error) => write!(formatter, "capture_build:{error}"),
            Self::CaptureStart(error) => write!(formatter, "capture_start:{error}"),
            Self::CapturePause(error) => write!(formatter, "capture_pause:{error}"),
            Self::OutputUnavailable => formatter.write_str("output_unavailable"),
        }
    }
}

impl Error for ProbeRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidDuration(error) => Some(error),
            Self::ConsentDenied(error) => Some(error),
            Self::CaptureBuild(error) | Self::CaptureStart(error) | Self::CapturePause(error) => {
                Some(error)
            }
            Self::InvalidArguments | Self::InputUnavailable | Self::OutputUnavailable => None,
        }
    }
}
