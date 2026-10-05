//! Manual acceptance test for the separately acquired reviewed model.

use std::path::PathBuf;

use flowdictate_asr::{
    AsrConfig, CanonicalAudio, LocalWhisper, ASR_SAMPLE_RATE_HZ, WHISPER_COMPATIBILITY,
};
use flowdictate_audio::verify_compiled_model_file;

#[test]
#[ignore = "requires the separately reviewed 32.2 MB local model artifact"]
fn reviewed_model_initializes_and_transcribes_silence() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("workspace path is unavailable")?
        .to_path_buf();
    let approved_root = workspace.join("models").join("whisper-tiny-q5_1");
    let model_path = approved_root.join("ggml-tiny-q5_1.bin");
    let verified = verify_compiled_model_file(
        &model_path,
        &approved_root,
        "asr-whisper-tiny-multilingual-q5_1",
        WHISPER_COMPATIBILITY,
    )?;
    let runtime = LocalWhisper::from_verified_model(verified, AsrConfig::default())?;
    let silence = vec![0.0_f32; ASR_SAMPLE_RATE_HZ];
    let transcript = runtime.transcribe(CanonicalAudio::new(&silence)?)?;

    assert!(transcript.text().len() <= flowdictate_asr::MAX_TRANSCRIPT_BYTES);
    assert!(transcript.segments().len() <= flowdictate_asr::MAX_TRANSCRIPT_SEGMENTS);
    assert_eq!(LocalWhisper::runtime_version(), "1.8.3");
    Ok(())
}
