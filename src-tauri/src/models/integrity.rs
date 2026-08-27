//! Shared integrity and path-containment helpers for local model artifacts.
//!
//! Catalog entries are the only authority for model filenames. Downloaders
//! verify the streamed bytes before publishing a final file; managers perform
//! a one-time verification migration for models installed by older releases.

use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArtifactSpec {
    pub model_id: &'static str,
    pub repository: &'static str,
    pub revision: &'static str,
    pub filename: &'static str,
    pub size_bytes: u64,
    /// Lower-case SHA-256, sourced from the upstream Git LFS object ID.
    pub sha256: &'static str,
}

impl ArtifactSpec {
    pub fn download_url(self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repository, self.revision, self.filename
        )
    }
}

pub(crate) fn safe_artifact_path(root: &Path, filename: &str) -> Result<PathBuf, String> {
    if filename.is_empty() || filename.contains('/') || filename.contains('\\') {
        return Err("model artifact filename must be a single path component".into());
    }
    let mut components = Path::new(filename).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(root.join(filename)),
        _ => Err("model artifact filename must be a single path component".into()),
    }
}

/// Containment for a multi-file model's own directory.
///
/// Directory-backed catalog entries store their artifacts under
/// `models_dir/<dir_name>/<filename>`. Both halves are validated as a single
/// path component, so the pair can never escape the models directory. Callers
/// pass the returned directory to the per-file helpers as their `root`, which
/// keeps size/SHA-256 enforcement byte-for-byte identical to single-file models.
pub(crate) fn safe_model_dir(root: &Path, dir_name: &str) -> Result<PathBuf, String> {
    let dir = safe_artifact_path(root, dir_name)
        .map_err(|_| "model directory name must be a single path component".to_string())?;
    // Mirrors the per-file guards below: a directory that already exists but
    // resolves through a symlink/junction must never be trusted as the real
    // model directory. A directory that does not exist yet (the common case
    // before a first download) is not rejected here.
    if let Ok(metadata) = std::fs::symlink_metadata(&dir) {
        if metadata.file_type().is_symlink() {
            return Err("model directory must not be a symlink or junction".to_string());
        }
    }
    Ok(dir)
}

pub(crate) fn expected_file_exists(root: &Path, spec: ArtifactSpec) -> bool {
    let Ok(path) = safe_artifact_path(root, spec.filename) else {
        return false;
    };
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    metadata.file_type().is_file()
        && !metadata.file_type().is_symlink()
        && metadata.len() == spec.size_bytes
}

pub(crate) fn verify_or_migrate_file(root: &Path, spec: ArtifactSpec) -> Result<PathBuf, String> {
    validate_spec(spec)?;
    let path = safe_artifact_path(root, spec.filename)?;
    let metadata = checked_metadata(&path, spec)?;

    if marker_matches(&path, spec, &metadata) {
        return Ok(path);
    }

    let actual = hash_file(&path)?;
    if actual != spec.sha256 {
        return Err(format!(
            "integrity check failed for model '{}': SHA-256 mismatch",
            spec.model_id
        ));
    }
    write_verification_marker(&path, spec)?;
    Ok(path)
}

pub(crate) fn verify_stream_result(
    spec: ArtifactSpec,
    downloaded_bytes: u64,
    actual_sha256: &str,
) -> Result<(), String> {
    validate_spec(spec)?;
    if downloaded_bytes != spec.size_bytes {
        return Err(format!(
            "integrity check failed for model '{}': expected {} bytes, received {}",
            spec.model_id, spec.size_bytes, downloaded_bytes
        ));
    }
    if actual_sha256 != spec.sha256 {
        return Err(format!(
            "integrity check failed for model '{}': SHA-256 mismatch",
            spec.model_id
        ));
    }
    Ok(())
}

pub(crate) fn write_verification_marker(path: &Path, spec: ArtifactSpec) -> Result<(), String> {
    let metadata = checked_metadata(path, spec)?;
    let stamp = modified_stamp(&metadata)?;
    let marker = verification_marker_path(path)?;
    let contents = format!(
        "omnivox-model-v1\nsha256={}\nsize={}\nmodified={}\n",
        spec.sha256, spec.size_bytes, stamp
    );
    std::fs::write(marker, contents)
        .map_err(|e| format!("failed to record model verification: {e}"))
}

pub(crate) fn remove_verification_marker(path: &Path) {
    if let Ok(marker) = verification_marker_path(path) {
        match std::fs::remove_file(marker) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {}
        }
    }
}

pub(crate) fn sha256_hex(hasher: Sha256) -> String {
    format!("{:x}", hasher.finalize())
}

pub(crate) fn new_hasher() -> Sha256 {
    Sha256::new()
}

fn validate_spec(spec: ArtifactSpec) -> Result<(), String> {
    if spec.sha256.len() != 64
        || !spec
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(format!(
            "model '{}' has an invalid integrity manifest entry",
            spec.model_id
        ));
    }
    let _ = safe_artifact_path(Path::new("."), spec.filename)?;
    Ok(())
}

fn checked_metadata(path: &Path, spec: ArtifactSpec) -> Result<std::fs::Metadata, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| format!("model '{}' is not available: {e}", spec.model_id))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!(
            "model '{}' must resolve to a regular file",
            spec.model_id
        ));
    }
    if metadata.len() != spec.size_bytes {
        return Err(format!(
            "integrity check failed for model '{}': expected {} bytes, found {}",
            spec.model_id,
            spec.size_bytes,
            metadata.len()
        ));
    }
    Ok(metadata)
}

fn hash_file(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|e| format!("failed to open model for hashing: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("failed to hash model: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(sha256_hex(hasher))
}

fn marker_matches(path: &Path, spec: ArtifactSpec, metadata: &std::fs::Metadata) -> bool {
    let Ok(marker) = verification_marker_path(path) else {
        return false;
    };
    let Ok(contents) = std::fs::read_to_string(marker) else {
        return false;
    };
    let Ok(stamp) = modified_stamp(metadata) else {
        return false;
    };
    contents
        == format!(
            "omnivox-model-v1\nsha256={}\nsize={}\nmodified={}\n",
            spec.sha256, spec.size_bytes, stamp
        )
}

fn modified_stamp(metadata: &std::fs::Metadata) -> Result<u128, String> {
    metadata
        .modified()
        .map_err(|e| format!("failed to read model modification time: {e}"))?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|_| "model modification time predates the Unix epoch".into())
}

fn verification_marker_path(path: &Path) -> Result<PathBuf, String> {
    let filename = path
        .file_name()
        .ok_or_else(|| "model path has no filename".to_string())?;
    let mut marker_name = OsString::from(filename);
    marker_name.push(".omnivox-verified");
    Ok(path.with_file_name(marker_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SPEC: ArtifactSpec = ArtifactSpec {
        model_id: "test-model",
        repository: "example/test",
        revision: "0123456789012345678901234567890123456789",
        filename: "model.bin",
        size_bytes: 11,
        sha256: "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
    };

    #[test]
    fn artifact_paths_reject_traversal_and_nested_names() {
        let root = Path::new("models");
        assert!(safe_artifact_path(root, "model.bin").is_ok());
        for invalid in [
            "",
            "../model.bin",
            "..\\model.bin",
            "nested/model.bin",
            "/tmp/model",
        ] {
            assert!(
                safe_artifact_path(root, invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn model_directories_reject_traversal_and_nested_names() {
        let root = Path::new("models");
        assert_eq!(
            safe_model_dir(root, "parakeet-tdt-0.6b-v2-int8").unwrap(),
            root.join("parakeet-tdt-0.6b-v2-int8")
        );
        for invalid in ["", "..", "../models", "..\\models", "nested/dir", "/tmp/x"] {
            assert!(
                safe_model_dir(root, invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn safe_model_dir_rejects_an_existing_symlink() {
        let root = std::env::temp_dir().join(format!(
            "omnivox-integrity-symlink-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let real_dir = root.join("real-model-dir");
        std::fs::create_dir_all(&real_dir).unwrap();
        let link_name = "linked-model-dir";
        let link_path = root.join(link_name);

        #[cfg(windows)]
        let symlink_created = std::os::windows::fs::symlink_dir(&real_dir, &link_path).is_ok();
        #[cfg(unix)]
        let symlink_created = std::os::unix::fs::symlink(&real_dir, &link_path).is_ok();
        #[cfg(not(any(windows, unix)))]
        let symlink_created = false;

        if symlink_created {
            assert!(
                safe_model_dir(&root, link_name).is_err(),
                "a directory symlink must be rejected"
            );
        } else {
            // Creating a directory symlink on Windows normally requires
            // Developer Mode or an elevated process, neither of which this
            // test can assume. Fall back to covering just the
            // name-validation half of `safe_model_dir` here; the symlink
            // rejection itself is exercised by `expected_file_exists`'s
            // identical per-file guard, which needs no special privilege.
            assert!(safe_model_dir(&root, link_name).is_ok());
        }

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn old_files_are_verified_once_and_hash_mismatches_are_rejected() {
        let dir =
            std::env::temp_dir().join(format!("omnivox-integrity-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join(TEST_SPEC.filename);
        std::fs::write(&model, b"hello world").unwrap();

        assert_eq!(verify_or_migrate_file(&dir, TEST_SPEC).unwrap(), model);
        let marker = verification_marker_path(&model).unwrap();
        assert!(marker.is_file());

        std::fs::write(&model, b"hello rust!").unwrap();
        // Force the slow path deterministically. Filesystems differ in
        // modification-time resolution, so this test must not assume two
        // immediate writes receive different timestamps.
        std::fs::remove_file(marker).unwrap();
        assert!(verify_or_migrate_file(&dir, TEST_SPEC).is_err());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn stream_verification_requires_exact_size_and_digest() {
        assert!(verify_stream_result(TEST_SPEC, 11, TEST_SPEC.sha256).is_ok());
        assert!(verify_stream_result(TEST_SPEC, 10, TEST_SPEC.sha256).is_err());
        assert!(verify_stream_result(TEST_SPEC, 11, &"0".repeat(64)).is_err());
    }
}
