//! Relay mic mute state to CC /voice keybinding.
//!
//! Listens for CoreAudio input-device mute changes (e.g. from a MuteMe button)
//! and drives CC's push-to-talk in HOLD mode via a `keystroke_io::SyntheticHold`:
//! holds the keybind while the mic is unmuted, releases on mute. The synthetic-
//! hold mechanics live in `keystroke_io`; this daemon is just the mute→hold policy
//! (plus the recording lock / hush / queue-drain coordination).
//!
//! Usage:
//!     relay_mic_to_voice --key ctrl+shift+k --repeat-ms 10 --verbose
//!
//! On unmute (recording starts), calls `hush` to stop any in-progress speech.

use clap::Parser;
use keystroke_io::{KeyCombo, SyntheticHold};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

// =============================================================================
// CLI
// =============================================================================

#[derive(Parser, Debug)]
#[command(about = "Relay mic mute state to CC /voice keybinding")]
struct Args {
    /// Key combo to hold (e.g. space, meta+k, ctrl+shift+k)
    #[arg(long, default_value = "space")]
    key: String,

    /// Print state transitions to stderr
    #[arg(long)]
    verbose: bool,

    /// Milliseconds between key re-presses while holding, simulating key-repeat
    /// for CC's hold detection. Lower = tighter hold; raise if it floods, lower
    /// if CC drops to "processing" mid-hold (the listening/processing cycle).
    #[arg(long, default_value = "8")]
    repeat_ms: u64,
}

// =============================================================================
// CoreAudio mic mute listener
// =============================================================================

mod audio {
    use coreaudio_sys::*;
    use std::os::raw::c_void;

    /// Get the default input (microphone) device ID.
    pub fn default_input_device() -> Result<AudioDeviceID, String> {
        let mut device_id: AudioDeviceID = 0;
        let mut size = std::mem::size_of::<AudioDeviceID>() as u32;
        let mut address = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultInputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        let status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject,
                &mut address,
                0,
                std::ptr::null(),
                &mut size,
                &mut device_id as *mut _ as *mut c_void,
            )
        };
        if status != 0 {
            return Err(format!("AudioObjectGetPropertyData failed: {status}"));
        }
        if device_id == 0 || device_id == kAudioDeviceUnknown {
            return Err("no default input device found".into());
        }
        Ok(device_id)
    }

    /// Read the current mute state of a device's input scope.
    pub fn is_input_muted(device_id: AudioDeviceID) -> Result<bool, String> {
        let mut muted: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let mut address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyMute,
            mScope: kAudioDevicePropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain,
        };
        let status = unsafe {
            AudioObjectGetPropertyData(
                device_id,
                &mut address,
                0,
                std::ptr::null(),
                &mut size,
                &mut muted as *mut _ as *mut c_void,
            )
        };
        if status != 0 {
            return Err(format!("failed to read mute state: {status}"));
        }
        Ok(muted != 0)
    }

    /// Context passed to the CoreAudio property listener callback.
    pub struct ListenerContext {
        pub device_id: AudioDeviceID,
        pub callback: Box<dyn Fn(bool) + Send>,
    }

    /// CoreAudio property listener callback — called on mute state changes.
    unsafe extern "C" fn mute_listener_proc(
        _object_id: AudioObjectID,
        _num_addresses: u32,
        _addresses: *const AudioObjectPropertyAddress,
        client_data: *mut c_void,
    ) -> OSStatus {
        let ctx = &*(client_data as *const ListenerContext);
        if let Ok(muted) = is_input_muted(ctx.device_id) {
            (ctx.callback)(muted);
        }
        0
    }

    /// Register a listener for mute state changes on the given device.
    /// Returns a boxed context that must be kept alive for the listener duration.
    pub fn register_mute_listener(
        device_id: AudioDeviceID,
        callback: impl Fn(bool) + Send + 'static,
    ) -> Result<Box<ListenerContext>, String> {
        let ctx = Box::new(ListenerContext {
            device_id,
            callback: Box::new(callback),
        });

        let mut address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyMute,
            mScope: kAudioDevicePropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain,
        };

        let status = unsafe {
            AudioObjectAddPropertyListener(
                device_id,
                &mut address,
                Some(mute_listener_proc),
                &*ctx as *const ListenerContext as *mut c_void,
            )
        };
        if status != 0 {
            return Err(format!("AudioObjectAddPropertyListener failed: {status}"));
        }
        Ok(ctx)
    }
}

// =============================================================================
// Speech control
// =============================================================================

fn recording_lock_path() -> std::path::PathBuf {
    write_engine::ai_home().join("control/voice/RECORDING.lock")
}

fn queue_path() -> std::path::PathBuf {
    write_engine::ai_home().join("voice/queue.jsonl")
}

fn create_recording_lock() {
    let path = recording_lock_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::File::create(&path);
}

fn remove_recording_lock() {
    let _ = std::fs::remove_file(recording_lock_path());
}

/// Call `hush` to stop any in-progress announce/TTS playback.
fn hush() {
    let _ = std::process::Command::new("hush")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Check if the frontmost macOS app is a terminal emulator.
fn terminal_is_focused() -> bool {
    let output = match std::process::Command::new("osascript")
        .args(["-e", "tell application \"System Events\" to get name of first application process whose frontmost is true"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };
    let name = String::from_utf8_lossy(&output.stdout);
    let name = name.trim();
    matches!(name, "iTerm2" | "Terminal" | "Ghostty" | "kitty" | "WezTerm" | "Alacritty")
}

/// Drain queued announce messages, playing each sequentially.
fn drain_queue(verbose: bool) {
    let path = queue_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return, // no queue file = nothing to drain
    };

    let _ = std::fs::remove_file(&path);

    for line in content.lines() {
        let entry: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                if verbose {
                    eprintln!("relay_mic_to_voice: skipping bad queue entry: {e}");
                }
                continue;
            }
        };

        let args: Vec<String> = entry["args"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let text = entry["text"].as_str().unwrap_or("");

        if verbose {
            eprintln!("relay_mic_to_voice: replaying queued announce: {args:?}");
        }

        let mut cmd = std::process::Command::new("announce");
        cmd.args(&args);

        // If text is present, pipe it via --stdin; otherwise pass as positional arg
        if !text.is_empty() {
            cmd.arg("--stdin");
            cmd.stdin(std::process::Stdio::piped());
        }

        match cmd.spawn() {
            Ok(mut child) => {
                if !text.is_empty() {
                    if let Some(ref mut stdin) = child.stdin.take() {
                        use std::io::Write;
                        let _ = stdin.write_all(text.as_bytes());
                    }
                }
                let _ = child.wait(); // sequential — avoid overlapping playback
            }
            Err(e) => {
                if verbose {
                    eprintln!("relay_mic_to_voice: failed to spawn announce: {e}");
                }
            }
        }
    }
}

// =============================================================================
// Signal handling
// =============================================================================

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn shutdown_handler(_sig: i32) {
    SHUTDOWN.store(true, Ordering::Relaxed);
}

fn install_signal_handlers() {
    unsafe {
        libc::signal(libc::SIGINT, shutdown_handler as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, shutdown_handler as *const () as libc::sighandler_t);
    }
}

// =============================================================================
// State machine
// =============================================================================

struct RecordingState {
    recording: bool,
    holding: bool,
}

impl RecordingState {
    fn new() -> Self {
        Self { recording: false, holding: false }
    }

    /// Mic unmuted — start recording; hold the keybind if a terminal is focused.
    fn start(&mut self, hold: &SyntheticHold, verbose: bool) {
        create_recording_lock();
        hush();
        self.recording = true;

        if terminal_is_focused() {
            hold.start();
            self.holding = true;
        } else if verbose {
            eprintln!("relay_mic_to_voice: no terminal focused, skipping keystrokes");
        }
    }

    /// Mic muted — release the hold, stop recording, drain the queue.
    fn stop(&mut self, hold: &SyntheticHold, verbose: bool) {
        if self.holding {
            hold.stop();
            self.holding = false;
        }
        remove_recording_lock();
        self.recording = false;
        drain_queue(verbose);
    }
}

// =============================================================================
// Run
// =============================================================================

fn run(args: Args) -> Result<(), String> {
    let combo = KeyCombo::parse(&args.key)?;
    let verbose = args.verbose;

    eprintln!("relay_mic_to_voice: listening for mic mute changes, key={}", args.key);

    let device_id = audio::default_input_device()?;
    let initial_muted = audio::is_input_muted(device_id)?;
    eprintln!("relay_mic_to_voice: device={device_id}, initial_muted={initial_muted}");

    let (tx, rx) = mpsc::channel::<bool>();
    let _listener_ctx = audio::register_mute_listener(device_id, move |muted| {
        let _ = tx.send(muted);
    })?;

    install_signal_handlers();

    let hold = SyntheticHold::new(combo, Duration::from_millis(args.repeat_ms))?;
    let mut state = RecordingState::new();

    event_loop(&rx, &hold, &mut state, verbose);

    state.stop(&hold, verbose);
    eprintln!("relay_mic_to_voice: shutdown");
    Ok(())
}

fn event_loop(
    rx: &mpsc::Receiver<bool>,
    hold: &SyntheticHold,
    state: &mut RecordingState,
    verbose: bool,
) {
    // Poll interval only bounds how quickly we notice SHUTDOWN; mute changes
    // arrive as events. The hold's own worker thread handles key-repeat.
    let poll_interval = Duration::from_millis(200);

    while !SHUTDOWN.load(Ordering::Relaxed) {
        match rx.recv_timeout(poll_interval) {
            Ok(muted) => {
                if verbose {
                    let label = if muted { "muted" } else { "unmuted" };
                    eprintln!("relay_mic_to_voice: mic {label}");
                }
                if muted && state.recording {
                    state.stop(hold, verbose);
                } else if !muted && !state.recording {
                    state.start(hold, verbose);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            _ => {}
        }
    }
}

// =============================================================================
// Entry point
// =============================================================================

pub fn main() {
    let args = Args::parse();
    if let Err(e) = run(args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
