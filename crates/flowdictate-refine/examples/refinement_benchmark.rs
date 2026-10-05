//! Numeric-only release benchmark for bounded deterministic refinement.
#![allow(clippy::print_stdout)]

use std::{hint::black_box, process::ExitCode, time::Instant};

use flowdictate_refine::{
    refine_without_model, validate_refined_output, CleanupConfig, OutputValidationConfig,
    MAX_CLEANUP_BYTES,
};

const WARMUP_ITERATIONS: usize = 128;
const MEASURED_ITERATIONS: usize = 2_048;
const PHRASE: &str = "benchmark  sentence , with   bounded punctuation . ";

fn main() -> ExitCode {
    if run().is_ok() {
        ExitCode::SUCCESS
    } else {
        println!("status=error");
        ExitCode::FAILURE
    }
}

fn run() -> Result<(), ()> {
    let source = build_source()?;
    let config = CleanupConfig::default();
    for _ in 0..WARMUP_ITERATIONS {
        let output = refine_without_model(black_box(&source), config).map_err(|_| ())?;
        black_box(output.text().len());
    }

    let mut cleanup_ns = durations()?;
    let mut observed_output_bytes = 0usize;
    for sample in &mut cleanup_ns {
        let started = Instant::now();
        let output = refine_without_model(black_box(&source), config).map_err(|_| ())?;
        *sample = started.elapsed().as_nanos();
        observed_output_bytes = black_box(output.text().len());
    }

    let candidate = refine_without_model(&source, config).map_err(|_| ())?;
    let validation = OutputValidationConfig::deterministic(MAX_CLEANUP_BYTES).map_err(|_| ())?;
    let mut validation_ns = durations()?;
    for sample in &mut validation_ns {
        let started = Instant::now();
        let checked =
            validate_refined_output(&source, candidate.text(), validation).map_err(|_| ())?;
        *sample = started.elapsed().as_nanos();
        black_box(checked.text().len());
    }

    let cleanup = summarize(&mut cleanup_ns);
    let validation = summarize(&mut validation_ns);
    println!("status=ok");
    println!("iterations={MEASURED_ITERATIONS}");
    println!("input_bytes={}", source.len());
    println!("output_bytes={observed_output_bytes}");
    println!("cleanup_p50_ns={}", cleanup.p50);
    println!("cleanup_p95_ns={}", cleanup.p95);
    println!("cleanup_max_ns={}", cleanup.maximum);
    println!("validation_p50_ns={}", validation.p50);
    println!("validation_p95_ns={}", validation.p95);
    println!("validation_max_ns={}", validation.maximum);
    println!("no_model_rate_basis_points=10000");
    Ok(())
}

fn build_source() -> Result<String, ()> {
    let mut source = String::new();
    source
        .try_reserve_exact(MAX_CLEANUP_BYTES)
        .map_err(|_| ())?;
    while source.len().saturating_add(PHRASE.len()) <= MAX_CLEANUP_BYTES {
        source.push_str(PHRASE);
    }
    Ok(source)
}

fn durations() -> Result<Vec<u128>, ()> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(MEASURED_ITERATIONS)
        .map_err(|_| ())?;
    values.resize(MEASURED_ITERATIONS, 0);
    Ok(values)
}

struct TimingSummary {
    p50: u128,
    p95: u128,
    maximum: u128,
}

fn summarize(values: &mut [u128]) -> TimingSummary {
    values.sort_unstable();
    TimingSummary {
        p50: values[percentile_index(values.len(), 50)],
        p95: values[percentile_index(values.len(), 95)],
        maximum: values[values.len() - 1],
    }
}

const fn percentile_index(length: usize, percentile: usize) -> usize {
    length
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1)
}
