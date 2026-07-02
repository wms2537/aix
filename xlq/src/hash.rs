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
