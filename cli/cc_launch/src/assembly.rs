use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::model::AppState;

pub fn estimate_tokens(state: &AppState) -> usize {
    let mut total_bytes: usize = 0;

    // Foundation
    if let Some(ref foundation) = state.library.foundation {
        total_bytes += foundation.byte_size;
    }

    // Always-loaded fragments
    for fragment in &state.library.always {
        total_bytes += fragment.byte_size;
    }

    // Selected persona
    if let Some(index) = state.selected_persona {
        if let Some(persona) = state.library.personas.get(index) {
            total_bytes += persona.byte_size;
        }
    }

    // Auto-matched space descriptor
    if let Some(space_idx) = state.auto_space_index() {
        if let Some(descriptor) = state.library.descriptors.get(space_idx) {
            total_bytes += descriptor.byte_size;
        }
    }

    // Selected system descriptors
    for index in state.ordered_descriptor_indices() {
        if let Some(descriptor) = state.library.descriptors.get(index) {
            total_bytes += descriptor.byte_size;
        }
    }

    // Coding fragments (if enabled)
    if state.coding_enabled {
        for fragment in &state.library.coding {
            total_bytes += fragment.byte_size;
        }
    }

    // Selected expertise
    for (index, selected) in state.selected_expertise.iter().enumerate() {
        if *selected {
            if let Some(fragment) = state.library.expertise.get(index) {
                total_bytes += fragment.byte_size;
            }
        }
    }

    total_bytes / 4
}

pub fn write_prompt_file(state: &AppState) -> Result<PathBuf, String> {
    if state.selected_persona.is_none() {
        return Err("No persona selected. Select a persona before launching.".to_string());
    }

    let prompt = assemble_prompt(state)?;
    let output_path = std::env::temp_dir().join("cc_launch_prompt.xml");
    fs::write(&output_path, &prompt)
        .map_err(|e| format!("write prompt file: {e}"))?;

    // Persist assembled prompt into workspace for hook-based re-injection on compact.
    let workspace_copy = state.workspace_path().join(".SYSTEM_PROMPT.xml");
    let _ = fs::write(&workspace_copy, &prompt);

    Ok(output_path)
}

fn assemble_prompt(state: &AppState) -> Result<String, String> {
    let mut parts: Vec<String> = Vec::new();

    // === IDENTITY LAYER (who you are, how you think) ===

    // 1. Foundation — the collaboration paradigm. Position 1, always.
    if let Some(ref foundation) = state.library.foundation {
        parts.push(read_fragment(&foundation.path)?);
    }

    // 2. Persona — who you are within the collaboration. Position 2.
    if let Some(index) = state.selected_persona {
        if let Some(persona) = state.library.personas.get(index) {
            parts.push(read_fragment(&persona.path)?);
        }
    }

    // 3. Always-loaded: cognitive-mode → behavior → communication → safety → anthropic
    for fragment in &state.library.always {
        parts.push(read_fragment(&fragment.path)?);
    }

    // === CONTEXT LAYER (where you are, what you're working on) ===

    // 3. Auto-matched space descriptor (your workspace environment)
    if let Some(space_idx) = state.auto_space_index() {
        if let Some(descriptor) = state.library.descriptors.get(space_idx) {
            parts.push(read_fragment(&descriptor.path)?);
        }
    }

    // 4. System descriptors (primary first, siblings, overview last)
    for index in state.ordered_descriptor_indices() {
        if let Some(descriptor) = state.library.descriptors.get(index) {
            parts.push(read_fragment(&descriptor.path)?);
        }
    }

    // === MODE LAYER (how you work this session) ===

    // 5. Coding fragments (if enabled)
    if state.coding_enabled {
        for fragment in &state.library.coding {
            parts.push(read_fragment(&fragment.path)?);
        }
    }

    // 6. Language context (only when coding enabled)
    let languages = state.selected_language_names();
    if state.coding_enabled && !languages.is_empty() {
        let lang_tags: Vec<String> = languages
            .iter()
            .map(|language| format!("  <language>{language}</language>"))
            .collect();
        parts.push(format!(
            "<context>\n{}\n</context>",
            lang_tags.join("\n")
        ));
    }

    // 7. Selected expertise
    for (index, selected) in state.selected_expertise.iter().enumerate() {
        if *selected {
            if let Some(fragment) = state.library.expertise.get(index) {
                parts.push(read_fragment(&fragment.path)?);
            }
        }
    }

    let persona_id = state
        .selected_persona
        .and_then(|i| state.library.personas.get(i))
        .map(|p| p.id.as_str())
        .unwrap_or("none");

    let workspace_name = state
        .workspace_path()
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "auto".to_string());

    let body = parts.join("\n\n");

    // Check for unsubstituted template variables (e.g. {MEMORY_DIR})
    let template_re = Regex::new(r"\{[A-Z][A-Z_]+\}").map_err(|e| format!("regex: {e}"))?;
    if let Some(m) = template_re.find(&body) {
        return Err(format!("Unsubstituted template variable in assembled prompt: {}", m.as_str()));
    }

    Ok(format!(
        "<prompt version=\"1\" persona=\"{persona_id}\" workspace=\"{workspace_name}\" coding=\"{}\">\n\n{body}\n\n</prompt>",
        state.coding_enabled,
    ))
}

fn read_fragment(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))
}

/// Write assembled prompt directly into a session's control directory.
/// Used by --update to hot-swap the system prompt for an active session.
pub fn write_update_file(state: &AppState, session_id: &str, workspace_name: &str) -> Result<PathBuf, String> {
    if state.selected_persona.is_none() {
        return Err("No persona selected. Select a persona before updating.".to_string());
    }

    let prompt = assemble_prompt(state)?;
    let session_dir = workspace_registry::workspace_control_dir(workspace_name).join(session_id);

    if !session_dir.exists() {
        return Err(format!(
            "Session directory does not exist: {}\nIs the session ID correct?",
            session_dir.display()
        ));
    }

    let target = session_dir.join("SYSTEM_PROMPT.xml");
    fs::write(&target, &prompt)
        .map_err(|e| format!("write update file: {e}"))?;

    Ok(target)
}

/// Allow-paths for the launching session, derived from the selected persona.
/// Persona-bound: Tyr always carries security-infrastructure access, no other
/// persona carries anything. See permissions.rs for the compiled mapping.
pub fn build_allow_paths(state: &AppState) -> Option<String> {
    let persona_id = selected_persona_id(state)?;
    crate::permissions::allow_paths_for_persona(persona_id)
}

/// The id of the currently selected persona, if any.
pub fn selected_persona_id(state: &AppState) -> Option<&str> {
    state
        .selected_persona
        .and_then(|index| state.library.personas.get(index))
        .map(|persona| persona.id.as_str())
}

// ---------------------------------------------------------------------------
// Launch spec — the handoff between `--pick` and `--go`.
//
// The pick phase runs the TUI and assembles everything, then records what the
// go phase needs into a spec file. Between the two, the user's SHELL changes
// directory to the workspace (via the cc_launch shell function) — the one
// thing no child process can do for it. The go phase consumes the spec and
// execs claude. The spec is ephemeral by construction: read once, deleted.
// ---------------------------------------------------------------------------

/// Everything `--go` needs to launch the session.
pub struct LaunchSpec {
    pub prompt_path: PathBuf,
    pub workspace_path: Option<PathBuf>,
    pub persona_id: Option<String>,
    pub claude_args: Vec<String>,
}

/// Location of the launch spec file (alongside the assembled prompt).
pub fn launch_spec_path() -> PathBuf {
    std::env::temp_dir().join("cc_launch_spec.toml")
}

/// Write the launch spec for a pick→cd→go launch sequence.
pub fn write_launch_spec(state: &AppState, prompt_path: &Path) -> Result<(), String> {
    let mut table = toml::Table::new();
    table.insert(
        "prompt_path".to_string(),
        toml::Value::String(prompt_path.to_string_lossy().to_string()),
    );
    table.insert(
        "workspace_path".to_string(),
        toml::Value::String(state.workspace_path().to_string_lossy().to_string()),
    );
    if let Some(persona_id) = selected_persona_id(state) {
        table.insert(
            "persona_id".to_string(),
            toml::Value::String(persona_id.to_string()),
        );
    }
    table.insert(
        "claude_args".to_string(),
        toml::Value::Array(
            state
                .passthrough_flags
                .iter()
                .map(|flag| toml::Value::String(flag.clone()))
                .collect(),
        ),
    );

    let serialized = toml::to_string(&table).map_err(|e| format!("serialize launch spec: {e}"))?;
    fs::write(launch_spec_path(), serialized).map_err(|e| format!("write launch spec: {e}"))
}

/// Read AND DELETE the launch spec. One-shot by design — a stale spec must
/// never launch a second, unintended session.
pub fn consume_launch_spec() -> Result<LaunchSpec, String> {
    let spec_path = launch_spec_path();
    let raw = fs::read_to_string(&spec_path)
        .map_err(|_| "no launch spec found — run `cc_launch --pick` first".to_string())?;
    let _ = fs::remove_file(&spec_path);

    let table: toml::Table = raw
        .parse()
        .map_err(|e| format!("parse launch spec: {e}"))?;

    let prompt_path = table
        .get("prompt_path")
        .and_then(|value| value.as_str())
        .map(PathBuf::from)
        .ok_or_else(|| "launch spec missing prompt_path".to_string())?;

    let workspace_path = table
        .get("workspace_path")
        .and_then(|value| value.as_str())
        .map(PathBuf::from);

    let persona_id = table
        .get("persona_id")
        .and_then(|value| value.as_str())
        .map(str::to_string);

    let claude_args = table
        .get("claude_args")
        .and_then(|value| value.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    Ok(LaunchSpec { prompt_path, workspace_path, persona_id, claude_args })
}

/// Remove any stale launch spec (e.g. after a TUI quit).
pub fn discard_launch_spec() {
    let _ = fs::remove_file(launch_spec_path());
}
