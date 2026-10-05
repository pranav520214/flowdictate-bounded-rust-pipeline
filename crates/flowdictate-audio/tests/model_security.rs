//! Public-seam tests for the offline model integrity gate.
#![allow(clippy::expect_used)]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use flowdictate_audio::{
    verify_compiled_model_file, verify_model_file, ModelCompatibility, ModelManifest,
    ModelManifestEntry, ModelVerificationError, COMPILED_MODEL_MANIFEST,
    MODEL_MANIFEST_SCHEMA_VERSION,
};
use sha2::{Digest, Sha256};

struct TestRoot(PathBuf);

impl TestRoot {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "flowdictate-model-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("test root should be creatable");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn entry_for(bytes: &[u8]) -> ModelManifestEntry {
    ModelManifestEntry {
        id: "fixture-asr",
        file_name: "fixture.bin",
        purpose: "asr",
        architecture: "fixture",
        quantization: "qtest",
        runtime: "fixture-runtime",
        source_repository: "https://example.invalid/fixture",
        source_revision: "fixture-revision",
        license_id: "MIT",
        languages: &["fixture-language"],
        size_bytes: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    }
}

#[test]
fn compiled_manifest_contains_the_qwen_refiner_identity() {
    let entry = COMPILED_MODEL_MANIFEST
        .entries
        .iter()
        .find(|entry| entry.id == "refiner-qwen3-0.6b-q4_k_m")
        .copied()
        .expect("compiled Qwen refiner entry");

    assert_eq!(entry.purpose, "refinement");
    assert_eq!(entry.runtime, "llama.cpp");
    assert_eq!(entry.architecture, "qwen3");
    assert_eq!(entry.quantization, "q4_k_m");
    assert_eq!(entry.size_bytes, 396_704_416);
}

#[test]
#[ignore = "requires the separately downloaded reviewed Qwen refiner artifact"]
fn downloaded_qwen_refiner_matches_the_compiled_manifest() {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let approved_root = workspace_root.join("models/qwen3-0.6b-q4_k_m");
    let model_path = approved_root.join("Qwen3-0.6B-Q4_K_M.gguf");

    verify_compiled_model_file(
        &model_path,
        &approved_root,
        "refiner-qwen3-0.6b-q4_k_m",
        ModelCompatibility {
            purpose: "refinement",
            runtime: "llama.cpp",
            architecture: "qwen3",
            quantization: "q4_k_m",
        },
    )
    .expect("downloaded Qwen refiner should pass the compiled manifest");
}

fn manifest(entry: ModelManifestEntry) -> ModelManifest<'static> {
    ModelManifest {
        schema_version: MODEL_MANIFEST_SCHEMA_VERSION,
        entries: Box::leak(Box::new([entry])),
    }
}

#[test]
fn known_good_model_returns_the_hashed_handle() {
    let root = TestRoot::new("good");
    let model_path = root.path().join("fixture.bin");
    let bytes = b"reviewed model fixture";
    fs::write(&model_path, bytes).expect("fixture should be writable");

    let verified = verify_model_file(
        &model_path,
        root.path(),
        "fixture-asr",
        ModelCompatibility {
            purpose: "asr",
            runtime: "fixture-runtime",
            architecture: "fixture",
            quantization: "qtest",
        },
        manifest(entry_for(bytes)),
    )
    .expect("known-good fixture should verify");

    assert_eq!(verified.entry().id, "fixture-asr");
    assert_eq!(
        verified
            .canonical_path()
            .file_name()
            .and_then(|name| name.to_str()),
        Some("fixture.bin")
    );
    assert_eq!(
        verified
            .into_file()
            .metadata()
            .expect("handle metadata")
            .len(),
        bytes.len() as u64
    );
}

#[cfg(windows)]
#[test]
fn immutable_path_lease_blocks_mutation_until_drop() {
    let root = TestRoot::new("immutable-path");
    let model_path = root.path().join("fixture.bin");
    let bytes = b"reviewed model fixture";
    fs::write(&model_path, bytes).expect("fixture should be writable");

    let lease = verify_model_file(
        &model_path,
        root.path(),
        "fixture-asr",
        ModelCompatibility {
            purpose: "asr",
            runtime: "fixture-runtime",
            architecture: "fixture",
            quantization: "qtest",
        },
        manifest(entry_for(bytes)),
    )
    .expect("known-good fixture should verify")
    .into_immutable_path_lease()
    .expect("Windows should support an immutable path lease");

    assert_eq!(lease.entry().id, "fixture-asr");
    assert_eq!(
        lease.canonical_path(),
        fs::canonicalize(&model_path)
            .expect("fixture path should canonicalize")
            .as_path()
    );
    assert!(fs::write(&model_path, b"changed model fixture").is_err());
    assert!(fs::remove_file(&model_path).is_err());

    drop(lease);
    fs::write(&model_path, bytes).expect("dropping the lease should release mutation lock");
}

#[test]
fn unknown_or_corrupt_model_fails_closed() {
    let root = TestRoot::new("reject");
    let model_path = root.path().join("fixture.bin");
    fs::write(&model_path, b"actual bytes").expect("fixture should be writable");

    let unknown = verify_model_file(
        &model_path,
        root.path(),
        "not-approved",
        ModelCompatibility {
            purpose: "asr",
            runtime: "fixture-runtime",
            architecture: "fixture",
            quantization: "qtest",
        },
        manifest(entry_for(b"actual bytes")),
    );
    assert!(matches!(unknown, Err(ModelVerificationError::UnknownModel)));

    let corrupt = verify_model_file(
        &model_path,
        root.path(),
        "fixture-asr",
        ModelCompatibility {
            purpose: "asr",
            runtime: "fixture-runtime",
            architecture: "fixture",
            quantization: "qtest",
        },
        manifest(entry_for(b"actual bytez")),
    );
    assert!(matches!(corrupt, Err(ModelVerificationError::HashMismatch)));

    let wrong_size = verify_model_file(
        &model_path,
        root.path(),
        "fixture-asr",
        ModelCompatibility {
            purpose: "asr",
            runtime: "fixture-runtime",
            architecture: "fixture",
            quantization: "qtest",
        },
        manifest(entry_for(b"different bytes")),
    );
    assert!(matches!(
        wrong_size,
        Err(ModelVerificationError::SizeMismatch)
    ));
}

#[test]
fn path_and_manifest_compatibility_are_enforced() {
    let root = TestRoot::new("boundary");
    let outside = TestRoot::new("outside");
    let model_path = outside.path().join("fixture.bin");
    let bytes = b"reviewed model fixture";
    fs::write(&model_path, bytes).expect("fixture should be writable");

    let outside_result = verify_model_file(
        &model_path,
        root.path(),
        "fixture-asr",
        ModelCompatibility {
            purpose: "asr",
            runtime: "fixture-runtime",
            architecture: "fixture",
            quantization: "qtest",
        },
        manifest(entry_for(bytes)),
    );
    assert!(matches!(
        outside_result,
        Err(ModelVerificationError::PathOutsideApprovedRoot)
    ));

    let wrong_runtime = verify_model_file(
        &model_path,
        outside.path(),
        "fixture-asr",
        ModelCompatibility {
            purpose: "asr",
            runtime: "other-runtime",
            architecture: "fixture",
            quantization: "qtest",
        },
        manifest(entry_for(bytes)),
    );
    assert!(matches!(
        wrong_runtime,
        Err(ModelVerificationError::RuntimeMismatch)
    ));
}

#[test]
fn malformed_manifest_is_rejected_before_path_access() {
    let root = TestRoot::new("manifest");
    let malformed = ModelManifest {
        schema_version: MODEL_MANIFEST_SCHEMA_VERSION + 1,
        entries: &[],
    };

    let result = verify_model_file(
        &root.path().join("missing.bin"),
        root.path(),
        "fixture-asr",
        ModelCompatibility {
            purpose: "asr",
            runtime: "fixture-runtime",
            architecture: "fixture",
            quantization: "qtest",
        },
        malformed,
    );
    assert!(matches!(
        result,
        Err(ModelVerificationError::InvalidManifest)
    ));
}

#[test]
fn compiled_manifest_rejects_an_unknown_identity_before_path_access() {
    let result = verify_compiled_model_file(
        Path::new("not-used.bin"),
        Path::new("not-used-root"),
        "fixture-asr",
        ModelCompatibility {
            purpose: "asr",
            runtime: "fixture-runtime",
            architecture: "fixture",
            quantization: "qtest",
        },
    );
    assert!(matches!(result, Err(ModelVerificationError::UnknownModel)));
}

#[test]
#[ignore = "requires the separately downloaded reviewed model artifact"]
fn downloaded_reviewed_model_matches_the_compiled_manifest() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root should exist");
    let approved_root = workspace_root.join("models/whisper-tiny-q5_1");
    let model_path = approved_root.join("ggml-tiny-q5_1.bin");

    let verified = verify_compiled_model_file(
        &model_path,
        &approved_root,
        "asr-whisper-tiny-multilingual-q5_1",
        ModelCompatibility {
            purpose: "asr",
            runtime: "whisper.cpp",
            architecture: "whisper",
            quantization: "q5_1",
        },
    )
    .expect("downloaded reviewed model should pass the production gate");

    assert_eq!(verified.entry().size_bytes, 32_152_673);
    assert_eq!(verified.entry().license_id, "MIT");
}

#[test]
#[ignore = "requires the separately downloaded reviewed Nemotron model artifact"]
fn downloaded_reviewed_nemotron_matches_the_compiled_experimental_identity() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root should exist");
    let approved_root = workspace_root.join("models/nemotron-3.5-asr-streaming-0.6b-q8_0");
    let model_path = approved_root.join("nemotron-3.5-asr-streaming-0.6b.q8_0.gguf");

    let lease = verify_compiled_model_file(
        &model_path,
        &approved_root,
        "asr-nemotron-3.5-streaming-0.6b-q8_0-experimental",
        ModelCompatibility {
            purpose: "asr",
            runtime: "nemo-speech.cpp",
            architecture: "fastconformer-rnnt",
            quantization: "q8_0",
        },
    )
    .expect("downloaded Nemotron model should pass the compiled experimental gate")
    .into_immutable_path_lease()
    .expect("this platform should support the immutable path lease");

    assert_eq!(lease.entry().size_bytes, 741_548_352);
    assert_eq!(lease.entry().license_id, "LicenseRef-OpenMDW-1.1");
}
