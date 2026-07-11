use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::io::Read;

/// SHA-256 of a file's bytes, streamed. Hex-encoded, lowercase.
///
/// Error contexts carry the BASENAME only: error messages end up in the
/// stdout JSON payload, which must never contain full filesystem paths.
pub fn sha256_file(path: &str) -> Result<String> {
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<file>".to_string());
    let file = std::fs::File::open(path).with_context(|| format!("open {name}"))?;
    let mut reader = std::io::BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// SHA-256 of an in-memory byte slice. Hex-encoded, lowercase. The single source
/// for hashing produced bytes (result snapshots, backup-integrity checks) —
/// consumed by apply/restructure/undo so the write path and the recovery path
/// agree on exactly one digest function.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_known_content_to_known_digest() {
        let dir = std::env::temp_dir().join("xlq-hash-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("abc-{}.bin", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(path.to_str().unwrap()).unwrap(),
            // SHA-256("abc"), FIPS 180-2 test vector.
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn open_error_carries_basename_only() {
        let err = sha256_file("/tmp/xlq-secret-dir-name/missing.xlsx")
            .expect_err("missing file must fail");
        let text = format!("{err:#}");
        assert!(text.contains("missing.xlsx"), "basename missing: {text}");
        assert!(
            !text.contains("xlq-secret-dir-name"),
            "directory leaked into error: {text}"
        );
    }

    #[test]
    fn path_without_file_name_component_uses_placeholder_and_read_fails() {
        // "/" has no file-name component (the "<file>" placeholder branch)
        // and opening it succeeds on Linux but reading fails (EISDIR), which
        // exercises the read error path too.
        assert!(sha256_file("/").is_err());
    }
}
