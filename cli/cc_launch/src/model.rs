use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Fragment {
    pub path: PathBuf,
    pub id: String,
    pub category: FragmentCategory,
    pub load: LoadPolicy,
    pub display_name: String,
    pub byte_size: usize,
    pub coding_related: bool,
}

#[derive(Debug, Clone)]
pub struct Descriptor {
    pub path: PathBuf,
    pub id: String,
    pub parent: Option<String>,
    pub languages: Vec<String>,
    pub display_name: String,
    pub byte_size: usize,
    pub kind: DescriptorKind,
}

/// A language is the set of coding fragments that apply to that language only.
/// It is loaded on top of the general coding rules when selected, and its name
/// is the name of its directory under `coding/`.
#[derive(Debug, Clone)]
pub struct Language {
    pub name: String,
    pub fragments: Vec<Fragment>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DescriptorKind {
    System,
    Space,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FragmentCategory {
    Persona,
    CognitiveMode,
    Behavior,
    Communication,
    Safety,
    Anthropic,
    Coding,
    Expertise,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoadPolicy {
    Always,
    Implement,
    Manual,
}

#[derive(Debug)]
pub struct Library {
    pub foundation: Option<Fragment>,
    pub personas: Vec<Fragment>,
    pub always: Vec<Fragment>,
    pub coding: Vec<Fragment>,
    pub expertise: Vec<Fragment>,
    pub descriptors: Vec<Descriptor>,
    pub languages: Vec<Language>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceProfile {
    pub name: String,
    pub path: PathBuf,
    pub persona: String,
    pub coding: bool,
    pub expertise: Vec<String>,
    pub primary_descriptor: Option<String>,
    pub descriptors: Vec<String>,
}

pub struct AppState {
    pub library: Library,
    pub profiles: Vec<WorkspaceProfile>,
    pub selected_workspace: Option<usize>,
    pub selected_persona: Option<usize>,
    pub coding_enabled: bool,
    pub selected_languages: Vec<bool>,
    pub selected_expertise: Vec<bool>,
    pub selected_descriptors: Vec<bool>,
    pub active_section: Section,
    pub section_cursor: usize,
    pub passthrough_flags: Vec<String>,
    pub update_mode: bool,
    /// True once the user has picked a persona themselves. A workspace profile
    /// then fills the slot only when this is false — selecting a workspace must
    /// never silently replace an identity the user already chose.
    pub persona_chosen_explicitly: bool,
    /// Where cc_launch was invoked. This is the workspace when no profile is
    /// selected — "Auto" means *this* directory, never "no directory".
    pub launch_directory: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Section {
    Workspace,
    Persona,
    Coding,
    Descriptors,
    Expertise,
}

pub enum TuiOutcome {
    Launch,
    Quit,
}

impl Section {
    /// Next section following column-aware order:
    /// C1: Persona → Workspace → Expertise
    /// C2: Descriptors → Coding
    /// Tab wraps: Coding → Persona
    ///
    /// Persona leads because it decides the most: identity, toolkit and
    /// permissions travel with it, while the workspace defaults to wherever
    /// the launcher was invoked and usually needs no decision at all.
    pub fn next(self) -> Self {
        match self {
            Section::Persona => Section::Workspace,
            Section::Workspace => Section::Expertise,
            Section::Expertise => Section::Descriptors,
            Section::Descriptors => Section::Coding,
            Section::Coding => Section::Persona,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Section::Persona => Section::Coding,
            Section::Workspace => Section::Persona,
            Section::Expertise => Section::Workspace,
            Section::Descriptors => Section::Expertise,
            Section::Coding => Section::Descriptors,
        }
    }
}

/// Persona used when nothing else determines one — no profile selected, or a
/// profile naming a persona that does not exist. Odinn is the all-rounder, so
/// it is the least wrong identity for a session that has not been told who it
/// is. Previously this was whichever persona sorted first by filename, which
/// made Bragi the default by alphabetical accident.
const DEFAULT_PERSONA_ID: &str = "odinn";

impl Library {
    /// Index of the default persona, or None only when the library has none.
    /// Falls back to the first persona if DEFAULT_PERSONA_ID is missing — a
    /// renamed file must not leave a session with no identity at all.
    pub fn default_persona_index(&self) -> Option<usize> {
        if self.personas.is_empty() {
            return None;
        }
        Some(
            self.personas
                .iter()
                .position(|persona| persona.id == DEFAULT_PERSONA_ID)
                .unwrap_or(0),
        )
    }

    /// Returns indices of system descriptors only (not spaces).
    pub fn system_indices(&self) -> Vec<usize> {
        self.descriptors
            .iter()
            .enumerate()
            .filter(|(_, d)| d.kind == DescriptorKind::System)
            .map(|(i, _)| i)
            .collect()
    }

    /// Find the space descriptor matching a persona+path combination.
    /// Returns the descriptor index if persona id matches a space id
    /// AND the profile path contains /spaces/{persona_id}.
    pub fn matching_space_index(&self, persona_id: &str, workspace_path: &Path) -> Option<usize> {
        // Check path contains /spaces/{persona_id}
        let path_str = workspace_path.to_string_lossy();
        let space_marker = format!("/spaces/{persona_id}");
        if !path_str.contains(&space_marker) {
            return None;
        }
        self.descriptors
            .iter()
            .position(|d| d.kind == DescriptorKind::Space && d.id == persona_id)
    }
}

impl AppState {
    pub fn new(
        library: Library,
        profiles: Vec<WorkspaceProfile>,
        passthrough_flags: Vec<String>,
        update_mode: bool,
        launch_directory: PathBuf,
    ) -> Self {
        let expertise_count = library.expertise.len();
        let descriptor_count = library.descriptors.len();

        // Coding on by default: the general coding rules. Languages start
        // unselected; each one adds its own fragments when chosen.
        let selected_languages = vec![false; library.languages.len()];

        let initial_persona = library.default_persona_index();

        Self {
            library,
            profiles,
            selected_workspace: None,
            selected_persona: initial_persona,
            coding_enabled: true,
            selected_languages,
            selected_expertise: vec![false; expertise_count],
            selected_descriptors: vec![false; descriptor_count],
            active_section: Section::Persona,
            section_cursor: 0,
            passthrough_flags,
            update_mode,
            persona_chosen_explicitly: false,
            launch_directory,
        }
    }

    pub fn selected_language_names(&self) -> Vec<&str> {
        self.library
            .languages
            .iter()
            .enumerate()
            .filter(|(index, _)| self.selected_languages.get(*index).copied().unwrap_or(false))
            .map(|(_, language)| language.name.as_str())
            .collect()
    }

    /// The fragments of every selected language, in language order. Empty when
    /// coding is disabled — language rules sit on top of the general coding set.
    pub fn selected_language_fragments(&self) -> Vec<&Fragment> {
        if !self.coding_enabled {
            return Vec::new();
        }
        self.library
            .languages
            .iter()
            .enumerate()
            .filter(|(index, _)| self.selected_languages.get(*index).copied().unwrap_or(false))
            .flat_map(|(_, language)| language.fragments.iter())
            .collect()
    }

    /// Recompute language selections from the union of all selected descriptors.
    pub fn sync_languages_from_descriptors(&mut self) {
        for lang in &mut self.selected_languages {
            *lang = false;
        }

        // Collect descriptor indices to pull languages from
        let mut desc_indices: Vec<usize> = self
            .selected_descriptors
            .iter()
            .enumerate()
            .filter(|(_, sel)| **sel)
            .map(|(i, _)| i)
            .collect();

        if let Some(space_idx) = self.auto_space_index() {
            desc_indices.push(space_idx);
        }

        // Union all languages
        for desc_idx in desc_indices {
            let langs: Vec<String> = match self.library.descriptors.get(desc_idx) {
                Some(d) => d.languages.clone(),
                None => continue,
            };
            for lang in &langs {
                let pos = match self.library.languages.iter().position(|l| &l.name == lang) {
                    Some(p) => p,
                    None => continue,
                };
                if let Some(selected) = self.selected_languages.get_mut(pos) {
                    *selected = true;
                }
            }
        }
    }

    /// The directory this session will run in. A selected profile names it;
    /// otherwise it is the directory cc_launch was invoked from. There is
    /// always an answer — every path-dependent behaviour (space matching,
    /// language derivation, the launch spec) depends on that being true.
    pub fn workspace_path(&self) -> &Path {
        self.selected_workspace
            .and_then(|index| self.profiles.get(index))
            .map(|profile| profile.path.as_path())
            .unwrap_or(self.launch_directory.as_path())
    }

    /// Returns the auto-matched space descriptor index, if any.
    /// Uses the currently selected persona (not the profile default).
    pub fn auto_space_index(&self) -> Option<usize> {
        let persona_id = self.selected_persona
            .and_then(|i| self.library.personas.get(i))
            .map(|p| p.id.as_str())?;
        self.library
            .matching_space_index(persona_id, self.workspace_path())
    }

    /// Returns system descriptor indices ordered for prompt assembly:
    /// primary first, then siblings, then parent overview last.
    /// Space descriptors are NOT included here (they go via auto_space_index).
    pub fn ordered_descriptor_indices(&self) -> Vec<usize> {
        let mut primary = Vec::new();
        let mut siblings = Vec::new();
        let mut overviews = Vec::new();

        let primary_id = self
            .selected_workspace
            .and_then(|ws| self.profiles.get(ws))
            .and_then(|profile| profile.primary_descriptor.as_deref());

        for (index, selected) in self.selected_descriptors.iter().enumerate() {
            if !*selected {
                continue;
            }
            let descriptor = match self.library.descriptors.get(index) {
                Some(d) => d,
                None => continue,
            };
            // Skip spaces — they're handled separately
            if descriptor.kind == DescriptorKind::Space {
                continue;
            }

            if primary_id == Some(descriptor.id.as_str()) {
                primary.push(index);
            } else if self.is_overview_descriptor(index) {
                overviews.push(index);
            } else {
                siblings.push(index);
            }
        }

        let mut result = Vec::new();
        result.extend(primary);
        result.extend(siblings);
        result.extend(overviews);
        result
    }

    /// Check if a descriptor is an overview (has children pointing to it)
    fn is_overview_descriptor(&self, index: usize) -> bool {
        let descriptor = match self.library.descriptors.get(index) {
            Some(d) => d,
            None => return false,
        };
        self.library
            .descriptors
            .iter()
            .any(|d| d.parent.as_deref() == Some(descriptor.id.as_str()))
    }

    /// Map from system-only cursor position to full descriptor index.
    pub fn system_cursor_to_index(&self, cursor: usize) -> Option<usize> {
        self.library.system_indices().get(cursor).copied()
    }

    /// Count of system descriptors (for section_len).
    pub fn system_count(&self) -> usize {
        self.library.system_indices().len()
    }

    /// Record the user's own persona choice. Distinct from assigning a default
    /// so that a later workspace selection knows not to overwrite it.
    pub fn choose_persona(&mut self, persona_index: usize) {
        if persona_index < self.library.personas.len() {
            self.selected_persona = Some(persona_index);
            self.persona_chosen_explicitly = true;
        }
    }

    pub fn apply_workspace_profile(&mut self, profile_index: usize) {
        let profile = match self.profiles.get(profile_index) {
            Some(profile) => profile.clone(),
            None => return,
        };

        self.selected_workspace = Some(profile_index);

        // The profile's persona is a DEFAULT, not an override: it fills the slot
        // only when the user has not chosen for themselves. A profile naming a
        // persona that does not exist leaves the current one standing rather
        // than clearing it — a session must never end up with no identity.
        if !self.persona_chosen_explicitly {
            if let Some(index) = self
                .library
                .personas
                .iter()
                .position(|persona| persona.id == profile.persona)
            {
                self.selected_persona = Some(index);
            }
        }

        // Set coding
        self.coding_enabled = profile.coding;

        // Set expertise
        for (index, fragment) in self.library.expertise.iter().enumerate() {
            self.selected_expertise[index] = profile.expertise.contains(&fragment.id);
        }

        // Permissions are persona-bound (see permissions.rs) — nothing to set here.

        // Set system descriptors based on primary_descriptor + smart family logic
        self.apply_descriptor_defaults(&profile);

        // Derive languages from selected descriptors
        self.sync_languages_from_descriptors();
    }

    fn apply_descriptor_defaults(&mut self, profile: &WorkspaceProfile) {
        // Clear all descriptors first
        for selected in &mut self.selected_descriptors {
            *selected = false;
        }

        let descriptors = &self.library.descriptors;
        let explicit_ids = &profile.descriptors;
        let mut indices_to_select: Vec<usize> = Vec::new();

        // Explicit descriptor list from profile (always honored)
        for (index, descriptor) in descriptors.iter().enumerate() {
            if explicit_ids.iter().any(|id| id == &descriptor.id) {
                indices_to_select.push(index);
            }
        }

        // Primary descriptor + smart family logic
        let primary_id = match &profile.primary_descriptor {
            Some(id) => id.as_str(),
            None => {
                // No primary, just apply explicit list
                for index in indices_to_select {
                    if let Some(selected) = self.selected_descriptors.get_mut(index) {
                        *selected = true;
                    }
                }
                return;
            }
        };

        if let Some(primary_index) = descriptors
            .iter()
            .position(|d| d.id == primary_id && d.kind == DescriptorKind::System)
        {
            indices_to_select.push(primary_index);

            // If primary is a child, also select siblings + parent overview
            if let Some(parent_id) = descriptors[primary_index].parent.as_deref() {
                for (index, descriptor) in descriptors.iter().enumerate() {
                    if descriptor.kind != DescriptorKind::System {
                        continue;
                    }
                    if descriptor.parent.as_deref() == Some(parent_id)
                        || descriptor.id == parent_id
                    {
                        indices_to_select.push(index);
                    }
                }
            }
            // Parent selected: just primary + explicit list. Children not auto-selected.
        }

        for index in indices_to_select {
            if let Some(selected) = self.selected_descriptors.get_mut(index) {
                *selected = true;
            }
        }
    }

    pub fn clear_workspace_profile(&mut self) {
        self.selected_workspace = None;
        if !self.persona_chosen_explicitly {
            self.selected_persona = self.library.default_persona_index();
        }
        self.coding_enabled = true;
        for selected in &mut self.selected_languages {
            *selected = false;
        }
        for selected in &mut self.selected_expertise {
            *selected = false;
        }
        for selected in &mut self.selected_descriptors {
            *selected = false;
        }
    }

    pub fn toggle_coding(&mut self) {
        self.coding_enabled = !self.coding_enabled;
        if self.coding_enabled {
            self.sync_languages_from_descriptors();
            for (index, fragment) in self.library.expertise.iter().enumerate() {
                if fragment.coding_related {
                    if let Some(selected) = self.selected_expertise.get_mut(index) {
                        *selected = true;
                    }
                }
            }
        } else {
            for (index, fragment) in self.library.expertise.iter().enumerate() {
                if fragment.coding_related {
                    if let Some(selected) = self.selected_expertise.get_mut(index) {
                        *selected = false;
                    }
                }
            }
        }
    }

    pub fn section_len(&self, section: Section) -> usize {
        match section {
            Section::Workspace => self.profiles.len() + 1,
            Section::Persona => self.library.personas.len(),
            Section::Descriptors => self.system_count(),
            Section::Coding => 1 + self.library.languages.len(),
            Section::Expertise => self.library.expertise.len(),
        }
    }
}
