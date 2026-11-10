//! The digest of a rendered closure: sha256 over exactly the bytes
//! `render::render_symbols` produces, as lowercase hex.

use sha2::{Digest, Sha256};

pub fn closure_digest(rendered: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(rendered.as_bytes());
    format!("{:x}", hasher.finalize())
}
