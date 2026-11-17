//! Datagram types and transport for the Hlidskjalf messaging protocol.
//!
//! Three emit paths:
//!   `emit()`                  — fire-and-forget, no validation (hardcoded senders)
//!   `emit_validated()`        — schema-validated, returns Err on malformed (caller handles)
//!   `emit_validated_or_alert()` — schema-validated, emits alert on failure (autonomous callers)
//!
//! Dual transport:
//!   1. Unix stream socket → record_datagrams daemon (persistent archive)
//!   2. UDP multicast 239.0.0.1:9899 → live consumers (Hlidskjalf, etc.)
//!
//! Transport failures are self-alerting: if all channels fail, or if a datagram
//! exceeds the UDP safe limit, a small alert datagram is emitted describing the
//! failure. Callers can also inspect the returned `EmitReport` for programmatic
//! handling (e.g. reporting to stderr in CLI tools).
//!
//! Protocol: compact JSON + newline (one datagram per line).

use std::io::Write;
use std::net::{Ipv4Addr, UdpSocket};
use std::os::unix::net::UnixStream;
use std::time::Duration;

// Re-export types from core crate — all consumers get them through this crate
pub use datagram_core::{Datagram, DatagramKind, Priority};

pub const SOCKET_PATH: &str = "/tmp/ai_logger.sock";
const WRITE_TIMEOUT: Duration = Duration::from_millis(200);

const MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 0, 0, 1);
const MULTICAST_PORT: u16 = 9899;

/// UDP payload safe limit. Loopback MTU is 65535, but IP+UDP headers
/// consume 28 bytes. Leave margin for framing.
const UDP_SAFE_LIMIT: usize = 65000;

/// What happened when we tried to send a datagram.
#[derive(Debug, Clone)]
pub struct EmitReport {
    pub unix_ok: bool,
    pub udp_ok: bool,
    pub udp_skipped_size: bool,
    pub size_bytes: usize,
}

impl EmitReport {
    pub fn all_failed(&self) -> bool {
        !self.unix_ok && !self.udp_ok
    }
}

/// Send a datagram. Fire-and-forget — never panics, never blocks.
/// No schema validation. Use for hardcoded senders with known-good shapes.
pub fn emit(datagram: &Datagram) {
    let _ = try_emit(datagram);
}

/// Send a datagram after validating against the compiled schema.
/// Returns the `EmitReport` on success or an Err with validation message.
/// Use for dynamic payloads (Traffic, Quality) constructed at runtime.
pub fn emit_validated(datagram: &Datagram) -> Result<EmitReport, String> {
    let json_str = serde_json::to_string(datagram)
        .map_err(|e| format!("serialization error: {e}"))?;

    let result = schemas_embedded::DATAGRAM
        .validate(&json_str)
        .map_err(|e| format!("schema error: {e}"))?;

    if !result.valid {
        return Err(result.message);
    }

    try_emit(datagram).map_err(|e| format!("transport error: {e}"))
}

/// Validate and emit, alerting on failure. For autonomous callers (watchers, syn)
/// where no human sees stderr. Returns true if emitted, false if validation failed.
///
/// On validation failure:
///   1. Emits a high-priority alert datagram (via emit(), bypasses validation)
///   2. The alert includes the validation error and the source that tried to emit
///   3. Returns false so callers can track accurate counts
pub fn emit_validated_or_alert(datagram: &Datagram, caller: &str) -> bool {
    match emit_validated(datagram) {
        Ok(_report) => true,
        Err(e) => {
            let alert = Datagram {
                timestamp: now(),
                source: caller.into(),
                kind: DatagramKind::Alert,
                classifier: None,
                priority: Priority::High,
                workspace: datagram.workspace.clone(),
                detail: Some(format!("Schema validation failed: {e}")),
                speech: Some(format!(
                    "WARNING: {} emitted a malformed datagram. Schema validation failed.",
                    caller
                )),
                payload: None,
            };
            emit(&alert);
            false
        }
    }
}

/// Serialize and send a datagram through available transport channels.
/// Self-alerts on total failure or when UDP is skipped due to size.
pub fn try_emit(datagram: &Datagram) -> Result<EmitReport, String> {
    let mut json = serde_json::to_vec(datagram)
        .map_err(|e| format!("serialization error: {e}"))?;
    json.push(b'\n');

    let size_bytes = json.len();

    // Channel 1: Unix stream to logging daemon (permanent archive)
    let unix_ok = try_unix_stream(&json).is_ok();

    // Channel 2: UDP multicast on loopback (live consumers)
    let udp_skipped_size = size_bytes > UDP_SAFE_LIMIT;
    let udp_ok = if udp_skipped_size {
        false
    } else {
        try_udp_multicast(&json).is_ok()
    };

    let report = EmitReport { unix_ok, udp_ok, udp_skipped_size, size_bytes };

    // Self-alert: if ALL channels failed, emit a small alert
    if report.all_failed() {
        emit_transport_alert(
            &format!(
                "All transport channels failed ({size_bytes} bytes). Unix: failed. UDP: {}.",
                if udp_skipped_size { "skipped (oversized)" } else { "failed" }
            ),
            datagram,
        );
    } else if udp_skipped_size {
        // UDP skipped but Unix succeeded — alert that live consumers missed it
        emit_transport_alert(
            &format!(
                "Datagram too large for UDP ({size_bytes} bytes, limit {UDP_SAFE_LIMIT}). Sent via Unix socket only — live consumers (Hlidskjalf) did not receive it.",
            ),
            datagram,
        );
    }

    Ok(report)
}

/// Emit a small alert describing a transport failure.
/// Non-recursive: bypasses try_emit, sends directly through both channels.
fn emit_transport_alert(detail: &str, original: &Datagram) {
    let alert = Datagram {
        timestamp: now(),
        source: original.source.clone(),
        kind: DatagramKind::Alert,
        classifier: None,
        priority: Priority::High,
        workspace: original.workspace.clone(),
        detail: Some(format!("Transport: {detail}")),
        speech: Some("WARNING: datagram transport failure. Check Hlidskjalf.".into()),
        payload: None,
    };
    // Direct channel sends — no recursion through try_emit
    if let Ok(mut json) = serde_json::to_vec(&alert) {
        json.push(b'\n');
        let _ = try_unix_stream(&json);
        let _ = try_udp_multicast(&json);
    }
}

fn try_unix_stream(json: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = UnixStream::connect(SOCKET_PATH)?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    stream.write_all(json)?;
    Ok(())
}

fn try_udp_multicast(json: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    // Default SO_SNDBUF is 9216 on macOS — too small for quality datagrams
    // with many check groups. Raise to 65535 (IP-layer max).
    set_send_buffer(&socket, 65535)?;
    socket.set_multicast_ttl_v4(1)?;
    set_multicast_interface(&socket, Ipv4Addr::LOCALHOST)?;
    socket.send_to(json, (MULTICAST_ADDR, MULTICAST_PORT))?;
    Ok(())
}

/// Set SO_SNDBUF on a UDP socket.
fn set_send_buffer(socket: &UdpSocket, size: i32) -> Result<(), std::io::Error> {
    use std::os::unix::io::AsRawFd;
    let fd = socket.as_raw_fd();
    let ret = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &size as *const i32 as *const libc::c_void,
            std::mem::size_of::<i32>() as libc::socklen_t,
        )
    };
    if ret == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

/// Bind multicast output to a specific interface via IP_MULTICAST_IF.
fn set_multicast_interface(
    socket: &UdpSocket,
    interface: Ipv4Addr,
) -> Result<(), std::io::Error> {
    use std::os::unix::io::AsRawFd;
    let fd = socket.as_raw_fd();
    let addr = interface.octets();
    let ret = unsafe {
        libc::setsockopt(
            fd,
            libc::IPPROTO_IP,
            libc::IP_MULTICAST_IF,
            addr.as_ptr() as *const libc::c_void,
            4, // sizeof(struct in_addr)
        )
    };
    if ret == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Current time as Unix timestamp (f64).
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Get the workspace name from CLAUDE_PROJECT_DIR env var.
/// Prefer `workspace_from_path()` when you know the directory being operated on.
pub fn workspace_name() -> String {
    std::env::var("CLAUDE_PROJECT_DIR")
        .ok()
        .and_then(|p| {
            std::path::Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        })
        .unwrap_or_default()
}

/// Derive workspace identity from a filesystem path.
///
/// Paths under `~/ai/` become `@{relative}` with `:` separators
/// (e.g. `@smidja:nornir`). Colon separators avoid `/` which breaks
/// Svelte 5's reactive proxy in template rendering.
/// Paths outside `~/ai/` or when no path is meaningful: `@`.
pub fn workspace_from_path(scan_dir: &std::path::Path) -> String {
    let ai_base = ai_base_dir();

    let resolved = scan_dir.canonicalize().unwrap_or_else(|_| scan_dir.to_path_buf());
    let path_str = resolved.to_string_lossy();

    if let Some(relative) = path_str.strip_prefix(&ai_base) {
        let trimmed = relative.trim_end_matches('/');
        if trimmed.is_empty() {
            "@".to_string()
        } else {
            format!("@{}", trimmed.replace('/', ":"))
        }
    } else {
        "@".to_string()
    }
}

/// Compact an absolute path for display.
///
/// Paths under `~/ai/` become `@{relative}` with `/` separators preserved
/// (e.g. `/Users/johnny/ai/intercept/traffic/odinn/file.jsonl`
///    → `@intercept/traffic/odinn/file.jsonl`).
/// Paths outside `~/ai/` are returned unchanged.
pub fn compact_path(path: &str) -> String {
    let ai_base = ai_base_dir();
    if let Some(relative) = path.strip_prefix(&ai_base) {
        format!("@{relative}")
    } else {
        path.to_string()
    }
}

/// The ai home with its closing `/`, as the prefix a path under it starts with.
fn ai_base_dir() -> String {
    let ai_home = path_core::ai_home_under(std::env::var("HOME").ok().as_deref());
    format!("{}/", ai_home.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn workspace_from_path_under_ai() {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = format!("{home}/ai/smidja/nornir");
        assert_eq!(workspace_from_path(Path::new(&path)), "@smidja:nornir");
    }

    #[test]
    fn workspace_from_path_nested() {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = format!("{home}/ai/spaces/bragi");
        assert_eq!(workspace_from_path(Path::new(&path)), "@spaces:bragi");
    }

    #[test]
    fn workspace_from_path_ai_root() {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = format!("{home}/ai/");
        assert_eq!(workspace_from_path(Path::new(&path)), "@");
    }

    #[test]
    fn workspace_from_path_outside_ai() {
        assert_eq!(workspace_from_path(Path::new("/tmp/something")), "@");
    }

    #[test]
    fn workspace_from_path_trailing_slash() {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = format!("{home}/ai/smidja/nornir/");
        assert_eq!(workspace_from_path(Path::new(&path)), "@smidja:nornir");
    }

    #[test]
    fn compact_path_under_ai() {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = format!("{home}/ai/intercept/traffic/odinn/mainexch_abc.jsonl");
        assert_eq!(compact_path(&path), "@intercept/traffic/odinn/mainexch_abc.jsonl");
    }

    #[test]
    fn compact_path_outside_ai() {
        assert_eq!(compact_path("/tmp/something"), "/tmp/something");
    }
}
