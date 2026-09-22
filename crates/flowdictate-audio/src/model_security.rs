//! Offline model identity and integrity gate.

use std::{
    ffi::OsStr,
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

/// Manifest schema accepted by this crate.
pub const MODEL_MANIFEST_SCHEMA_VERSION: u32 = 1;
/// Maximum model artifact size accepted by the local gate.
pub const MAX_MODEL_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const HASH_BUFFER_BYTES: usize = 64 * 1024;

/// A release-curated model identity and compatibility record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModelManifestEntry {
    /// Stable release-curated model identifier.
    pub id: &'static str,
    /// Exact artifact filename expected under the approved root.
    pub file_name: &'static str,
    /// Intended use, such as `asr`.
    pub purpose: &'static str,
    /// Reviewed model architecture identifier.
    pub architecture: &'static str,
    /// Reviewed quantization identifier.
    pub quantization: &'static str,
    /// Reviewed runtime adapter identifier.
    pub runtime: &'static str,
    /// Upstream repository used for release review.
    pub source_repository: &'static str,
    /// Immutable upstream repository revision used for acquisition.
    pub source_revision: &'static str,
    /// Reviewed SPDX model-weight license identifier.
    pub license_id: &'static str,
    /// Language coverage declared by the reviewed upstream model card.
    pub languages: &'static [&'static str],
    /// Exact artifact length in bytes.
    pub size_bytes: u64,
    /// Exact SHA-256 digest of the artifact bytes.
    pub sha256: [u8; 32],
}

/// Compiled model manifest supplied by the release build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModelManifest<'a> {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Release-curated entries.
    pub entries: &'a [ModelManifestEntry],
}

/// Runtime compatibility tuple required by a reviewed model adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModelCompatibility {
    /// Intended model purpose, such as `asr`.
    pub purpose: &'static str,
    /// Runtime adapter identifier.
    pub runtime: &'static str,
    /// Model architecture identifier.
    pub architecture: &'static str,
    /// Quantization identifier.
    pub quantization: &'static str,
}

static APPROVED_MODEL_ENTRIES: [ModelManifestEntry; 4] = [
    ModelManifestEntry {
        id: "asr-whisper-tiny-multilingual-q5_1",
        file_name: "ggml-tiny-q5_1.bin",
        purpose: "asr",
        architecture: "whisper",
        quantization: "q5_1",
        runtime: "whisper.cpp",
        source_repository: "https://huggingface.co/ggerganov/whisper.cpp",
        source_revision: "98aa99a0a9db05ae2342309f5096248665f7cba3",
        license_id: "MIT",
        languages: &["multilingual"],
        size_bytes: 32_152_673,
        sha256: [
            0x81, 0x87, 0x10, 0x56, 0x8d, 0xa3, 0xca, 0x15, 0x68, 0x9e, 0x31, 0xa7, 0x43, 0x19,
            0x7b, 0x52, 0x00, 0x07, 0x87, 0x2f, 0xf9, 0x57, 0x62, 0x37, 0xbd, 0xa9, 0x7b, 0xd1,
            0xb4, 0x69, 0xc3, 0xd7,
        ],
    },
    ModelManifestEntry {
        id: "asr-whisper-base-multilingual-q5_1-comparison",
        file_name: "ggml-base-q5_1.bin",
        purpose: "asr",
        architecture: "whisper",
        quantization: "q5_1",
        runtime: "whisper.cpp",
        source_repository: "https://huggingface.co/ggerganov/whisper.cpp",
        source_revision: "98aa99a0a9db05ae2342309f5096248665f7cba3",
        license_id: "MIT",
        languages: &["multilingual"],
        size_bytes: 59_707_625,
        sha256: [
            0x42, 0x2f, 0x1a, 0xe4, 0x52, 0xad, 0xe6, 0xf3, 0x0a, 0x00, 0x4d, 0x7e, 0x5c, 0x6a,
            0x43, 0x19, 0x5e, 0x44, 0x33, 0xbc, 0x37, 0x0b, 0xf2, 0x3f, 0xac, 0x9c, 0xc5, 0x91,
            0xf0, 0x1a, 0x88, 0x98,
        ],
    },
    ModelManifestEntry {
        id: "asr-nemotron-3.5-streaming-0.6b-q8_0-experimental",
        file_name: "nemotron-3.5-asr-streaming-0.6b.q8_0.gguf",
        purpose: "asr",
        architecture: "fastconformer-rnnt",
        quantization: "q8_0",
        runtime: "nemo-speech.cpp",
        source_repository: "https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b",
        source_revision: "1c8deaecc64b91f034d73e08dd8b64625eb3395d",
        license_id: "LicenseRef-OpenMDW-1.1",
        languages: &["multilingual"],
        size_bytes: 741_548_352,
        sha256: [
            0xa5, 0xc4, 0x35, 0xf2, 0x94, 0xee, 0xa8, 0xf8, 0x8c, 0xe6, 0x8d, 0xd2, 0x7b, 0x8c,
            0x3b, 0xfe, 0xa7, 0xf7, 0x77, 0xcb, 0x2f, 0xbb, 0xa0, 0x4f, 0xcd, 0x30, 0xea, 0xa5,
            0x55, 0xf4, 0x29, 0xae,
        ],
    },
    ModelManifestEntry {
        id: "refiner-qwen3-0.6b-q4_k_m",
        file_name: "Qwen3-0.6B-Q4_K_M.gguf",
        purpose: "refinement",
        architecture: "qwen3",
        quantization: "q4_k_m",
        runtime: "llama.cpp",
        source_repository: "https://huggingface.co/Qwen/Qwen3-0.6B-GGUF",
        source_revision: "1208e45d782fe18602c5eaf10e5758d5b0f24c03",
        license_id: "Apache-2.0",
        languages: &["multilingual"],
        size_bytes: 396_704_416,
        sha256: [
            0xb0, 0x63, 0x8f, 0x08, 0x41, 0x7a, 0x2d, 0x3c, 0x86, 0x52, 0x76, 0x04, 0x62, 0xeb,
            0x54, 0x07, 0xc6, 0xe3, 0x01, 0x73, 0xcf, 0x96, 0x08, 0xad, 0x08, 0x20, 0x75, 0x7a,
            0x28, 0x1e, 0xea, 0x0e,
        ],
    },
];

/// Release-compiled manifest containing only reviewed model identities.
pub const COMPILED_MODEL_MANIFEST: ModelManifest<'static> = ModelManifest {
    schema_version: MODEL_MANIFEST_SCHEMA_VERSION,
    entries: &APPROVED_MODEL_ENTRIES,
};

/// A model file whose path, size, compatibility, and digest passed the gate.
///
/// The verified file handle is retained so a downstream reviewed adapter can
/// consume the same opened bytes rather than reopening an unchecked path.
pub struct VerifiedModel {
    file: File,
    canonical_path: PathBuf,
    entry: ModelManifestEntry,
}

impl VerifiedModel {
    /// Returns the canonical path used for the verified handle.
    #[must_use]
    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    /// Returns the manifest identity that matched the opened file.
    #[must_use]
    pub const fn entry(&self) -> ModelManifestEntry {
        self.entry
    }

    /// Transfers the verified read handle to a reviewed model adapter.
    #[must_use]
    pub fn into_file(self) -> File {
        self.file
    }

    /// Converts this verified handle into an immutable path lease for a
    /// reviewed native adapter that cannot load from an existing file handle.
    ///
    /// On Windows, verification opens the file with `FILE_SHARE_READ` only.
    /// Holding the returned lease therefore prevents write, delete, rename, or
    /// replacement while the adapter reopens the canonical path for reading.
    /// Other platforms fail closed until an equivalent immutable-open protocol
    /// is implemented and tested.
    ///
    /// # Errors
    ///
    /// Returns [`ModelVerificationError::ImmutablePathUnsupported`] when the
    /// current platform cannot uphold the lease contract.
    pub fn into_immutable_path_lease(
        self,
    ) -> Result<VerifiedModelPathLease, ModelVerificationError> {
        #[cfg(windows)]
        {
            Ok(VerifiedModelPathLease {
                _file: self.file,
                canonical_path: self.canonical_path,
                entry: self.entry,
            })
        }
        #[cfg(not(windows))]
        {
            Err(ModelVerificationError::ImmutablePathUnsupported)
        }
    }

    /// Reads and independently re-verifies the same already-hashed handle into
    /// bounded owned memory for transfer across a local process boundary.
    ///
    /// # Errors
    ///
    /// Returns a payload-free model verification failure if the handle cannot
    /// be rewound/read, memory cannot be reserved, or the bytes no longer match
    /// the compiled manifest.
    pub fn into_verified_bytes(self) -> Result<VerifiedModelBytes, ModelVerificationError> {
        let entry = self.entry;
        let size =
            usize::try_from(entry.size_bytes).map_err(|_| ModelVerificationError::SizeMismatch)?;
        let mut file = self.file;
        file.seek(SeekFrom::Start(0))
            .map_err(|_| ModelVerificationError::ReadFailed)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| ModelVerificationError::AllocationFailed)?;
        bytes.resize(size, 0);
        file.read_exact(&mut bytes)
            .map_err(|_| ModelVerificationError::ReadFailed)?;
        verify_compiled_model_bytes(
            bytes,
            entry.id,
            ModelCompatibility {
                purpose: entry.purpose,
                runtime: entry.runtime,
                architecture: entry.architecture,
                quantization: entry.quantization,
            },
        )
    }
}

/// A verified canonical model path whose bytes cannot be replaced while held.
///
/// This lease is currently available only on Windows. The guarded handle is
/// deliberately private and remains alive for the complete native model-load
/// operation.
pub struct VerifiedModelPathLease {
    _file: File,
    canonical_path: PathBuf,
    entry: ModelManifestEntry,
}

impl VerifiedModelPathLease {
    /// Returns the canonical path protected by the retained verification handle.
    #[must_use]
    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    /// Returns the manifest identity matched by the protected file.
    #[must_use]
    pub const fn entry(&self) -> ModelManifestEntry {
        self.entry
    }
}

/// Owned model bytes that independently passed the compiled manifest gate.
pub struct VerifiedModelBytes {
    bytes: Vec<u8>,
    entry: ModelManifestEntry,
}

impl VerifiedModelBytes {
    /// Returns the manifest identity matched by these bytes.
    #[must_use]
    pub const fn entry(&self) -> ModelManifestEntry {
        self.entry
    }

    /// Returns the exact verified bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    /// Transfers ownership without changing the verified bytes.
    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.bytes
    }
}

/// Payload-free model verification failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelVerificationError {
    /// The manifest schema is unsupported or contains invalid records.
    InvalidManifest,
    /// The requested model ID is absent from the compiled manifest.
    UnknownModel,
    /// The approved root cannot be inspected.
    ApprovedRootUnavailable,
    /// The candidate path is outside the approved root.
    PathOutsideApprovedRoot,
    /// The candidate is not a regular file.
    NotRegularFile,
    /// The candidate or approved root is a filesystem reparse point.
    ReparsePoint,
    /// The file could not be opened for verification.
    OpenFailed,
    /// Memory for a bounded model copy could not be reserved.
    AllocationFailed,
    /// The artifact filename differs from the reviewed manifest record.
    FileNameMismatch,
    /// The file length differs from the manifest record.
    SizeMismatch,
    /// Reading the file failed before hashing completed.
    ReadFailed,
    /// The computed SHA-256 differs from the manifest record.
    HashMismatch,
    /// The requested purpose does not match the manifest record.
    PurposeMismatch,
    /// The requested runtime does not match the manifest record.
    RuntimeMismatch,
    /// The requested architecture does not match the manifest record.
    ArchitectureMismatch,
    /// The requested quantization does not match the manifest record.
    QuantizationMismatch,
    /// This platform cannot protect a verified path while a runtime reopens it.
    ImmutablePathUnsupported,
}

impl std::fmt::Display for ModelVerificationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidManifest => "model manifest is invalid",
            Self::UnknownModel => "model identity is not approved",
            Self::ApprovedRootUnavailable => "approved model root is unavailable",
            Self::PathOutsideApprovedRoot => "model path is outside the approved root",
            Self::NotRegularFile => "model path is not a regular file",
            Self::ReparsePoint => "model path uses a filesystem reparse point",
            Self::OpenFailed => "model file could not be opened",
            Self::AllocationFailed => "model memory allocation failed",
            Self::FileNameMismatch => "model filename does not match its manifest",
            Self::SizeMismatch => "model file size does not match its manifest",
            Self::ReadFailed => "model file could not be read",
            Self::HashMismatch => "model file hash does not match its manifest",
            Self::PurposeMismatch => "model purpose is incompatible",
            Self::RuntimeMismatch => "model runtime is incompatible",
            Self::ArchitectureMismatch => "model architecture is incompatible",
            Self::QuantizationMismatch => "model quantization is incompatible",
            Self::ImmutablePathUnsupported => {
                "immutable verified model paths are unsupported on this platform"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ModelVerificationError {}

/// Verifies an explicitly selected local model against a compiled manifest.
///
/// The returned handle is the same handle that was hashed. Callers must pass
/// it to the reviewed adapter; reopening `canonical_path()` would reintroduce
/// a path-swap race.
///
/// # Errors
///
/// Returns a payload-free error for every manifest, path, compatibility, I/O,
/// size, or digest failure. No model bytes are returned by this API.
pub fn verify_model_file(
    model_path: &Path,
    approved_root: &Path,
    model_id: &str,
    compatibility: ModelCompatibility,
    manifest: ModelManifest<'_>,
) -> Result<VerifiedModel, ModelVerificationError> {
    validate_manifest(manifest)?;
    let entry = manifest
        .entries
        .iter()
        .find(|entry| entry.id == model_id)
        .copied()
        .ok_or(ModelVerificationError::UnknownModel)?;
    if entry.purpose != compatibility.purpose {
        return Err(ModelVerificationError::PurposeMismatch);
    }
    if entry.runtime != compatibility.runtime {
        return Err(ModelVerificationError::RuntimeMismatch);
    }
    if entry.architecture != compatibility.architecture {
        return Err(ModelVerificationError::ArchitectureMismatch);
    }
    if entry.quantization != compatibility.quantization {
        return Err(ModelVerificationError::QuantizationMismatch);
    }
    if model_path.file_name() != Some(OsStr::new(entry.file_name)) {
        return Err(ModelVerificationError::FileNameMismatch);
    }

    let root_metadata = fs::symlink_metadata(approved_root)
        .map_err(|_| ModelVerificationError::ApprovedRootUnavailable)?;
    if is_reparse_point(&root_metadata) {
        return Err(ModelVerificationError::ReparsePoint);
    }
    if !root_metadata.is_dir() {
        return Err(ModelVerificationError::ApprovedRootUnavailable);
    }
    let canonical_root = fs::canonicalize(approved_root)
        .map_err(|_| ModelVerificationError::ApprovedRootUnavailable)?;

    let candidate_metadata =
        fs::symlink_metadata(model_path).map_err(|_| ModelVerificationError::OpenFailed)?;
    reject_non_regular_or_reparse(&candidate_metadata)?;
    let canonical_path =
        fs::canonicalize(model_path).map_err(|_| ModelVerificationError::OpenFailed)?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(ModelVerificationError::PathOutsideApprovedRoot);
    }

    let file = open_for_verification(&canonical_path)?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| ModelVerificationError::OpenFailed)?;
    reject_non_regular_or_reparse(&opened_metadata)?;
    if opened_metadata.len() != entry.size_bytes || candidate_metadata.len() != entry.size_bytes {
        return Err(ModelVerificationError::SizeMismatch);
    }

    let actual_hash = hash_file(
        file.try_clone()
            .map_err(|_| ModelVerificationError::OpenFailed)?,
    )?;
    if !digest_matches(&entry.sha256, &actual_hash) {
        return Err(ModelVerificationError::HashMismatch);
    }

    Ok(VerifiedModel {
        file,
        canonical_path,
        entry,
    })
}

/// Verifies an explicitly selected local model against this binary's manifest.
///
/// This is the production entry point. It cannot be redirected to a detached
/// manifest supplied by the user or by a model file.
///
/// # Errors
///
/// Returns the same payload-free failures as [`verify_model_file`]. Only the
/// exact release-curated identity embedded above can pass.
pub fn verify_compiled_model_file(
    model_path: &Path,
    approved_root: &Path,
    model_id: &str,
    compatibility: ModelCompatibility,
) -> Result<VerifiedModel, ModelVerificationError> {
    verify_model_file(
        model_path,
        approved_root,
        model_id,
        compatibility,
        COMPILED_MODEL_MANIFEST,
    )
}

/// Verifies owned model bytes against this binary's compiled manifest.
///
/// This is intended for a child process that received bytes through a bounded
/// local IPC channel. The child does not trust the parent's type state and
/// repeats identity, size, compatibility, and SHA-256 validation before native
/// parsing.
///
/// # Errors
///
/// Returns a payload-free error for unknown identity, compatibility, size, or
/// digest failures.
pub fn verify_compiled_model_bytes(
    bytes: Vec<u8>,
    model_id: &str,
    compatibility: ModelCompatibility,
) -> Result<VerifiedModelBytes, ModelVerificationError> {
    validate_manifest(COMPILED_MODEL_MANIFEST)?;
    let entry = COMPILED_MODEL_MANIFEST
        .entries
        .iter()
        .find(|entry| entry.id == model_id)
        .copied()
        .ok_or(ModelVerificationError::UnknownModel)?;
    if entry.purpose != compatibility.purpose {
        return Err(ModelVerificationError::PurposeMismatch);
    }
    if entry.runtime != compatibility.runtime {
        return Err(ModelVerificationError::RuntimeMismatch);
    }
    if entry.architecture != compatibility.architecture {
        return Err(ModelVerificationError::ArchitectureMismatch);
    }
    if entry.quantization != compatibility.quantization {
        return Err(ModelVerificationError::QuantizationMismatch);
    }
    if u64::try_from(bytes.len()) != Ok(entry.size_bytes) {
        return Err(ModelVerificationError::SizeMismatch);
    }
    let actual: [u8; 32] = Sha256::digest(&bytes).into();
    if !digest_matches(&entry.sha256, &actual) {
        return Err(ModelVerificationError::HashMismatch);
    }
    Ok(VerifiedModelBytes { bytes, entry })
}

fn validate_manifest(manifest: ModelManifest<'_>) -> Result<(), ModelVerificationError> {
    if manifest.schema_version != MODEL_MANIFEST_SCHEMA_VERSION {
        return Err(ModelVerificationError::InvalidManifest);
    }
    for (index, entry) in manifest.entries.iter().enumerate() {
        if entry.id.is_empty()
            || entry.purpose.is_empty()
            || entry.file_name.is_empty()
            || entry.architecture.is_empty()
            || entry.quantization.is_empty()
            || entry.runtime.is_empty()
            || entry.source_repository.is_empty()
            || entry.source_revision.is_empty()
            || entry.license_id.is_empty()
            || entry.languages.is_empty()
            || entry.languages.iter().any(|language| language.is_empty())
            || entry.size_bytes == 0
            || entry.size_bytes > MAX_MODEL_BYTES
            || entry.sha256 == [0; 32]
            || manifest.entries[..index]
                .iter()
                .any(|previous| previous.id == entry.id)
        {
            return Err(ModelVerificationError::InvalidManifest);
        }
    }
    Ok(())
}

fn hash_file(mut file: File) -> Result<[u8; 32], ModelVerificationError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; HASH_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer).map_err(map_read_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn open_for_verification(path: &Path) -> Result<File, ModelVerificationError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        // Share reads only: writers and deleters cannot replace the bytes
        // between verification and consumption by the reviewed adapter.
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        options.share_mode(FILE_SHARE_READ);
    }
    options
        .open(path)
        .map_err(|_| ModelVerificationError::OpenFailed)
}

fn map_read_error(_: io::Error) -> ModelVerificationError {
    ModelVerificationError::ReadFailed
}

fn digest_matches(expected: &[u8; 32], actual: &[u8; 32]) -> bool {
    let mut difference = 0_u8;
    for (left, right) in expected.iter().zip(actual) {
        difference |= left ^ right;
    }
    difference == 0
}

fn reject_non_regular_or_reparse(metadata: &Metadata) -> Result<(), ModelVerificationError> {
    if is_reparse_point(metadata) {
        return Err(ModelVerificationError::ReparsePoint);
    }
    if !metadata.is_file() {
        return Err(ModelVerificationError::NotRegularFile);
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn is_reparse_point(_: &Metadata) -> bool {
    false
}
