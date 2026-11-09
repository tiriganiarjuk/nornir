//! Pure logic for the announce TTS CLI.
//!
//! Config types, settings resolution, caching, and audio conversion.
//! No I/O, no network, no filesystem, no process management.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// =============================================================================
// Constants
// =============================================================================

pub const ELEVENLABS_BASE: &str = "https://api.elevenlabs.io";
pub const KOKORO_BASE: &str = "http://127.0.0.1:8880";
pub const FILENAME_MAX_TEXT_LEN: usize = 48;
pub const HASH_SUFFIX_LEN: usize = 4;

// =============================================================================
// Config types
// =============================================================================

#[derive(Deserialize)]
pub struct VoiceDef {
    pub id: String,
    pub speed: Option<f32>,
}

#[derive(Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub backend: Backend,
    #[serde(default)]
    pub voices: HashMap<String, VoiceDef>,
    #[serde(default)]
    pub default: ProfileSettings,
    #[serde(default)]
    pub profile: HashMap<String, ProfileSettings>,
}

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    #[default]
    Kokoro,
    Elevenlabs,
}

#[derive(Deserialize, Default)]
pub struct ProfileSettings {
    pub voice: Option<String>,
    pub model: Option<String>,
    pub speed: Option<f32>,
    pub output_format: Option<String>,
    pub language_code: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct LookupTable {
    #[serde(default)]
    pub messages: HashMap<String, String>,
}

#[derive(Deserialize, serde::Serialize, Default)]
pub struct HitsTable {
    #[serde(default)]
    pub hits: HashMap<String, u32>,
}

// =============================================================================
// Resolved settings
// =============================================================================

pub struct Resolved {
    pub voice_id: String,
    pub model: String,
    pub speed: f32,
    pub language_code: String,
    pub text: String,
    pub api_key: String,
}

/// Input parameters for settings resolution. Mirrors the CLI args
/// that affect config merging, without coupling to clap.
pub struct ResolveInput<'a> {
    pub profile_name: Option<&'a str>,
    pub voice_override: Option<&'a str>,
    pub speed_override: Option<f32>,
    pub language: Option<&'a str>,
    pub lookup: Option<&'a str>,
    pub translate: Option<&'a str>,
    pub voice_lock: Option<&'a ProfileSettings>,
}

/// Resolve settings by merging 4 layers: CLI > VOICE.lock > profile > default.
/// `api_key` is passed in — the caller handles loading it from the environment.
pub fn resolve_settings(
    input: &ResolveInput,
    config: &Config,
    text: String,
    api_key: String,
) -> Resolved {
    let defaults = &config.default;
    let profile = input
        .profile_name
        .and_then(|name| config.profile.get(name));

    let voice_name = input
        .voice_override
        .or_else(|| input.voice_lock.and_then(|v| v.voice.as_deref()))
        .or_else(|| profile.and_then(|p| p.voice.as_deref()))
        .or_else(|| defaults.voice.as_deref())
        .unwrap_or("alloy");

    let voice_def = config.voices.get(voice_name);
    let voice_id = voice_def
        .map(|v| v.id.as_str())
        .unwrap_or(voice_name)
        .to_string();

    let model = profile
        .and_then(|p| p.model.as_deref())
        .or(defaults.model.as_deref())
        .unwrap_or("eleven_multilingual_v2")
        .to_string();

    let speed = input
        .speed_override
        .or_else(|| input.voice_lock.and_then(|v| v.speed))
        .or_else(|| profile.and_then(|p| p.speed))
        .or_else(|| voice_def.and_then(|v| v.speed))
        .or(defaults.speed)
        .unwrap_or(1.0);

    let language_code = input
        .language
        .or(input.lookup)
        .or(input.translate)
        .or_else(|| input.voice_lock.and_then(|v| v.language_code.as_deref()))
        .or_else(|| profile.and_then(|p| p.language_code.as_deref()))
        .or(defaults.language_code.as_deref())
        .unwrap_or("en")
        .to_string();

    let speed = (speed.clamp(0.7, 1.2) * 100.0).round() / 100.0;

    Resolved {
        voice_id,
        model,
        speed,
        language_code,
        text,
        api_key,
    }
}

// =============================================================================
// Lookup
// =============================================================================

/// Look up a pre-translated version of the text. Returns (text, language_code).
/// If `lookup_lang` is set and a match is found in the table, returns the
/// translated text and the lookup language. Otherwise returns the original.
pub fn apply_lookup(
    lookup_lang: Option<&str>,
    table: Option<&LookupTable>,
    text: &str,
    default_lang: &str,
) -> (String, String) {
    if let (Some(lang), Some(tbl)) = (lookup_lang, table) {
        if let Some(translated) = tbl.messages.get(text) {
            return (translated.to_string(), lang.to_string());
        }
    }
    (text.to_string(), default_lang.to_string())
}

// =============================================================================
// Cache
// =============================================================================

/// Compute a SHA-256 hash over TTS parameters for cache keying.
pub fn compute_hash(text: &str, voice_id: &str, speed: f32, model: &str, lang: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher.update(voice_id.as_bytes());
    hasher.update(speed.to_le_bytes());
    hasher.update(model.as_bytes());
    hasher.update(lang.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Sanitize text for use in a cache filename.
pub fn sanitize_filename(text: &str) -> String {
    let cleaned: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let joined = words.join("_");
    if joined.len() > FILENAME_MAX_TEXT_LEN {
        joined[..FILENAME_MAX_TEXT_LEN]
            .trim_end_matches('_')
            .to_string()
    } else {
        joined
    }
}

/// Build a cache file path. Pure path construction — caller handles `create_dir_all`.
pub fn build_cache_path(
    audio_dir: &Path,
    voice_id: &str,
    lang: &str,
    text: &str,
    hash_short: &str,
) -> PathBuf {
    let dir = audio_dir.join(voice_id).join(lang);
    let name = sanitize_filename(text);
    dir.join(format!("{name}_{hash_short}.mp3"))
}

// =============================================================================
// API
// =============================================================================

/// Build the JSON body for an ElevenLabs TTS request.
pub fn build_tts_body(text: &str, model: &str, speed: f32, lang: &str) -> serde_json::Value {
    let speed_rounded = ((speed as f64) * 100.0).round() / 100.0;
    serde_json::json!({
        "text": text,
        "model_id": model,
        "voice_settings": {
            "speed": speed_rounded
        },
        "language_code": lang
    })
}

/// Build the JSON body for a Kokoro TTS request (OpenAI-compatible).
pub fn build_kokoro_body(text: &str, voice: &str, speed: f32) -> serde_json::Value {
    let speed_rounded = ((speed as f64) * 100.0).round() / 100.0;
    serde_json::json!({
        "model": "kokoro",
        "input": text,
        "voice": voice,
        "speed": speed_rounded,
        "response_format": "mp3"
    })
}

// =============================================================================
// Audio
// =============================================================================

/// Convert PCM S16LE bytes to f32 samples normalized to [-1.0, 1.0].
pub fn pcm_s16le_to_f32(data: &[u8]) -> Vec<f32> {
    data.chunks_exact(2)
        .map(|chunk| {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            sample as f32 / 32768.0
        })
        .collect()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- compute_hash ---

    #[test]
    fn compute_hash_deterministic() {
        let h1 = compute_hash("hello", "voice1", 1.0, "model", "en");
        let h2 = compute_hash("hello", "voice1", 1.0, "model", "en");
        assert_eq!(h1, h2);
    }

    #[test]
    fn compute_hash_different_text_different_hash() {
        let h1 = compute_hash("hello", "voice1", 1.0, "model", "en");
        let h2 = compute_hash("world", "voice1", 1.0, "model", "en");
        assert_ne!(h1, h2);
    }

    #[test]
    fn compute_hash_different_voice_different_hash() {
        let h1 = compute_hash("hello", "voice1", 1.0, "model", "en");
        let h2 = compute_hash("hello", "voice2", 1.0, "model", "en");
        assert_ne!(h1, h2);
    }

    #[test]
    fn compute_hash_different_speed_different_hash() {
        let h1 = compute_hash("hello", "voice1", 1.0, "model", "en");
        let h2 = compute_hash("hello", "voice1", 1.1, "model", "en");
        assert_ne!(h1, h2);
    }

    #[test]
    fn compute_hash_different_lang_different_hash() {
        let h1 = compute_hash("hello", "voice1", 1.0, "model", "en");
        let h2 = compute_hash("hello", "voice1", 1.0, "model", "de");
        assert_ne!(h1, h2);
    }

    // --- sanitize_filename ---

    #[test]
    fn sanitize_filename_basic() {
        assert_eq!(sanitize_filename("Hello World"), "hello_world");
    }

    #[test]
    fn sanitize_filename_special_chars() {
        assert_eq!(sanitize_filename("he!!o w@rld"), "he_o_w_rld");
    }

    #[test]
    fn sanitize_filename_long_string() {
        let long = "a ".repeat(50);
        let result = sanitize_filename(&long);
        assert!(result.len() <= FILENAME_MAX_TEXT_LEN);
        assert!(!result.ends_with('_'));
    }

    #[test]
    fn sanitize_filename_empty() {
        assert_eq!(sanitize_filename(""), "");
    }

    #[test]
    fn sanitize_filename_unicode() {
        // is_alphanumeric() matches accented characters
        let result = sanitize_filename("café résumé");
        assert_eq!(result, "café_résumé");
    }

    // --- build_cache_path ---

    #[test]
    fn build_cache_path_structure() {
        let path = build_cache_path(
            Path::new("/audio"),
            "voice123",
            "en",
            "hello world",
            "abcd",
        );
        assert_eq!(
            path,
            PathBuf::from("/audio/voice123/en/hello_world_abcd.mp3")
        );
    }

    #[test]
    fn build_cache_path_no_filesystem() {
        let path = build_cache_path(
            Path::new("/nonexistent"),
            "v",
            "de",
            "test",
            "1234",
        );
        assert_eq!(path, PathBuf::from("/nonexistent/v/de/test_1234.mp3"));
    }

    // --- build_tts_body ---

    #[test]
    fn build_tts_body_shape() {
        let body = build_tts_body("hello", "eleven_multilingual_v2", 1.0, "en");
        assert_eq!(body["text"], "hello");
        assert_eq!(body["model_id"], "eleven_multilingual_v2");
        assert_eq!(body["voice_settings"]["speed"], 1.0);
        assert_eq!(body["language_code"], "en");
    }

    #[test]
    fn build_tts_body_speed_rounding() {
        let body = build_tts_body("test", "model", 0.999, "en");
        assert_eq!(body["voice_settings"]["speed"], 1.0);
    }

    // --- pcm_s16le_to_f32 ---

    #[test]
    fn pcm_s16le_to_f32_silence() {
        let data = [0u8, 0, 0, 0]; // two zero samples
        let result = pcm_s16le_to_f32(&data);
        assert_eq!(result, vec![0.0, 0.0]);
    }

    #[test]
    fn pcm_s16le_to_f32_max_positive() {
        let sample: i16 = i16::MAX; // 32767
        let bytes = sample.to_le_bytes();
        let result = pcm_s16le_to_f32(&bytes);
        assert!((result[0] - 0.999969).abs() < 0.0001);
    }

    #[test]
    fn pcm_s16le_to_f32_max_negative() {
        let sample: i16 = i16::MIN; // -32768
        let bytes = sample.to_le_bytes();
        let result = pcm_s16le_to_f32(&bytes);
        assert_eq!(result[0], -1.0);
    }

    #[test]
    fn pcm_s16le_to_f32_odd_length() {
        let data = [0u8, 0, 0]; // 3 bytes → only 1 complete sample
        let result = pcm_s16le_to_f32(&data);
        assert_eq!(result.len(), 1);
    }

    // --- resolve_settings ---

    #[test]
    fn resolve_settings_defaults() {
        let config = Config::default();
        let input = ResolveInput {
            profile_name: None,
            voice_override: None,
            speed_override: None,
            language: None,
            lookup: None,
            translate: None,
            voice_lock: None,
        };
        let resolved = resolve_settings(&input, &config, "hello".into(), "key".into());
        assert_eq!(resolved.voice_id, "alloy");
        assert_eq!(resolved.model, "eleven_multilingual_v2");
        assert_eq!(resolved.speed, 1.0);
        assert_eq!(resolved.language_code, "en");
        assert_eq!(resolved.text, "hello");
        assert_eq!(resolved.api_key, "key");
    }

    #[test]
    fn resolve_settings_cli_overrides() {
        let config = Config::default();
        let input = ResolveInput {
            profile_name: None,
            voice_override: Some("custom_voice"),
            speed_override: Some(0.9),
            language: Some("de"),
            lookup: None,
            translate: None,
            voice_lock: None,
        };
        let resolved = resolve_settings(&input, &config, "test".into(), "k".into());
        assert_eq!(resolved.voice_id, "custom_voice");
        assert_eq!(resolved.speed, 0.9);
        assert_eq!(resolved.language_code, "de");
    }

    #[test]
    fn resolve_settings_profile_fallback() {
        let mut profiles = HashMap::new();
        profiles.insert(
            "cc".to_string(),
            ProfileSettings {
                voice: Some("jarvis".to_string()),
                model: Some("turbo".to_string()),
                speed: Some(1.1),
                language_code: Some("sv".to_string()),
                output_format: None,
            },
        );
        let config = Config {
            backend: Backend::default(),
            voices: HashMap::new(),
            default: ProfileSettings::default(),
            profile: profiles,
        };
        let input = ResolveInput {
            profile_name: Some("cc"),
            voice_override: None,
            speed_override: None,
            language: None,
            lookup: None,
            translate: None,
            voice_lock: None,
        };
        let resolved = resolve_settings(&input, &config, "hi".into(), "k".into());
        assert_eq!(resolved.voice_id, "jarvis");
        assert_eq!(resolved.model, "turbo");
        assert_eq!(resolved.speed, 1.1);
        assert_eq!(resolved.language_code, "sv");
    }

    #[test]
    fn resolve_settings_voice_lock_overrides_profile() {
        let mut profiles = HashMap::new();
        profiles.insert(
            "cc".to_string(),
            ProfileSettings {
                voice: Some("jarvis".to_string()),
                speed: Some(1.0),
                ..Default::default()
            },
        );
        let config = Config {
            backend: Backend::default(),
            voices: HashMap::new(),
            default: ProfileSettings::default(),
            profile: profiles,
        };
        let lock = ProfileSettings {
            voice: Some("locked_voice".to_string()),
            speed: Some(0.8),
            ..Default::default()
        };
        let input = ResolveInput {
            profile_name: Some("cc"),
            voice_override: None,
            speed_override: None,
            language: None,
            lookup: None,
            translate: None,
            voice_lock: Some(&lock),
        };
        let resolved = resolve_settings(&input, &config, "hi".into(), "k".into());
        assert_eq!(resolved.voice_id, "locked_voice");
        assert_eq!(resolved.speed, 0.8);
    }

    #[test]
    fn resolve_settings_speed_clamped() {
        let config = Config::default();
        let input = ResolveInput {
            profile_name: None,
            voice_override: None,
            speed_override: Some(5.0), // way over 1.2 max
            language: None,
            lookup: None,
            translate: None,
            voice_lock: None,
        };
        let resolved = resolve_settings(&input, &config, "t".into(), "k".into());
        assert_eq!(resolved.speed, 1.2);
    }

    #[test]
    fn resolve_settings_voice_def_maps_name_to_id() {
        let mut voices = HashMap::new();
        voices.insert(
            "myvoice".to_string(),
            VoiceDef {
                id: "abc123".to_string(),
                speed: None,
            },
        );
        let config = Config {
            backend: Backend::default(),
            voices,
            default: ProfileSettings {
                voice: Some("myvoice".to_string()),
                ..Default::default()
            },
            profile: HashMap::new(),
        };
        let input = ResolveInput {
            profile_name: None,
            voice_override: None,
            speed_override: None,
            language: None,
            lookup: None,
            translate: None,
            voice_lock: None,
        };
        let resolved = resolve_settings(&input, &config, "t".into(), "k".into());
        assert_eq!(resolved.voice_id, "abc123");
    }

    // --- apply_lookup ---

    #[test]
    fn apply_lookup_match_found() {
        let mut messages = HashMap::new();
        messages.insert("hello".to_string(), "hej".to_string());
        let table = LookupTable { messages };
        let (text, lang) = apply_lookup(Some("sv"), Some(&table), "hello", "en");
        assert_eq!(text, "hej");
        assert_eq!(lang, "sv");
    }

    #[test]
    fn apply_lookup_no_match() {
        let table = LookupTable {
            messages: HashMap::new(),
        };
        let (text, lang) = apply_lookup(Some("sv"), Some(&table), "hello", "en");
        assert_eq!(text, "hello");
        assert_eq!(lang, "en");
    }

    #[test]
    fn apply_lookup_no_lang() {
        let (text, lang) = apply_lookup(None, None, "hello", "en");
        assert_eq!(text, "hello");
        assert_eq!(lang, "en");
    }
}
