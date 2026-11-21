//! relay_mic_to_voice — relay the Mac's mic mute state to Claude Code's voice keybinding.
//!
//! macOS only: it listens to CoreAudio for the mute state and drives a synthetic
//! key hold, neither of which exists elsewhere. The implementation is in
//! `macos.rs`. On any other system the binary still builds, so the workspace builds
//! and tests everywhere; it answers `--help` and otherwise exits 2.

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
fn main() {
    macos::main();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    let asked_for_help = std::env::args().skip(1).any(|arg| arg == "--help" || arg == "-h");
    println!("relay_mic_to_voice: relays the Mac's mic mute state to Claude Code's voice keybinding. macOS only.");
    if !asked_for_help {
        std::process::exit(2);
    }
}
