use std::fs;
use std::path::Path;

use regex::Regex;

use crate::model::{
    Descriptor, DescriptorKind, Fragment, FragmentCategory, Language, Library, LoadPolicy,
};

pub fn scan_library(base: &Path) -> Result<Library, String> {
    let mut foundation: Option<Fragment> = None;
    let mut personas = Vec::new();
    let mut always = Vec::new();
    let mut coding = Vec::new();
    let mut languages = Vec::new();
    let mut expertise = Vec::new();
    let mut descriptors = Vec::new();

    // Scan foundation/*.xml — single slot, the collaboration paradigm
    let foundation_dir = base.join("foundation");
    if foundation_dir.is_dir() {
        let frags = scan_xml_dir(&foundation_dir, FragmentCategory::CognitiveMode)?;
        foundation = frags.into_iter().next();
    }

    // Scan personas/*.xml
    let persona_dir = base.join("personas");
    if persona_dir.is_dir() {
        for frag in scan_xml_dir(&persona_dir, FragmentCategory::Persona)? {
            personas.push(frag);
        }
    }

    // Scan always-loaded categories
    let always_dirs = [
        ("cognitive-mode", FragmentCategory::CognitiveMode),
        ("behavior", FragmentCategory::Behavior),
        ("communication", FragmentCategory::Communication),
        ("safety", FragmentCategory::Safety),
        ("anthropic", FragmentCategory::Anthropic),
    ];

    for (dir_name, category) in &always_dirs {
        let dir = base.join(dir_name);
        if dir.is_dir() {
            for frag in scan_xml_dir(&dir, category.clone())? {
                if frag.load == LoadPolicy::Always {
                    always.push(frag);
                }
            }
        }
    }

    // Scan coding/*.xml — the general coding rules — and coding/<language>/*.xml,
    // the rules that apply to one language only.
    let coding_dir = base.join("coding");
    if coding_dir.is_dir() {
        coding = scan_xml_dir(&coding_dir, FragmentCategory::Coding)?;
        languages = scan_language_dirs(&coding_dir)?;
    }

    // Scan expertise/*.xml
    let expertise_dir = base.join("expertise");
    if expertise_dir.is_dir() {
        expertise = scan_expertise_xml(&expertise_dir)?;
    }

    // Scan systems/*.xml
    let systems_dir = base.join("systems");
    if systems_dir.is_dir() {
        descriptors.extend(scan_descriptor_dir(&systems_dir, DescriptorKind::System)?);
    }

    // Scan spaces/*.xml
    let spaces_dir = base.join("spaces");
    if spaces_dir.is_dir() {
        descriptors.extend(scan_descriptor_dir(&spaces_dir, DescriptorKind::Space)?);
    }

    // Sort all by filename
    personas.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    always.sort_by(|a, b| {
        category_order(&a.category)
            .cmp(&category_order(&b.category))
            .then_with(|| a.path.file_name().cmp(&b.path.file_name()))
    });
    coding.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    expertise.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    descriptors.sort_by(|a, b| a.display_name.cmp(&b.display_name));

    Ok(Library {
        foundation,
        personas,
        always,
        coding,
        expertise,
        descriptors,
        languages,
    })
}

fn category_order(category: &FragmentCategory) -> u8 {
    match category {
        FragmentCategory::CognitiveMode => 0,
        FragmentCategory::Behavior => 1,
        FragmentCategory::Communication => 2,
        FragmentCategory::Safety => 3,
        FragmentCategory::Anthropic => 4,
        _ => 5,
    }
}

fn scan_xml_dir(
    directory: &Path,
    default_category: FragmentCategory,
) -> Result<Vec<Fragment>, String> {
    let mut fragments = Vec::new();

    let entries =
        fs::read_dir(directory).map_err(|e| format!("read dir {}: {e}", directory.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("xml") {
            continue;
        }

        let content =
            fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;

        let byte_size = content.len();
        let (id, load) = parse_xml_root_attrs(&content, &default_category)?;
        let display_name = id_to_display(&id);

        fragments.push(Fragment {
            path,
            id,
            category: default_category.clone(),
            load,
            display_name,
            byte_size,
            coding_related: false,
        });
    }

    Ok(fragments)
}

/// Each directory under `coding/` that holds fragments is a language: the
/// directory name is the language name, and its fragments are the rules loaded
/// when that language is selected. A language exists exactly when it has
/// fragments, so every entry offered adds something to the prompt.
fn scan_language_dirs(coding_dir: &Path) -> Result<Vec<Language>, String> {
    let mut languages = Vec::new();

    let entries = fs::read_dir(coding_dir)
        .map_err(|e| format!("read dir {}: {e}", coding_dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        let mut fragments = scan_xml_dir(&path, FragmentCategory::Coding)?;
        if fragments.is_empty() {
            continue;
        }
        fragments.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));

        languages.push(Language { name, fragments });
    }

    languages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(languages)
}

fn scan_descriptor_dir(
    directory: &Path,
    kind: DescriptorKind,
) -> Result<Vec<Descriptor>, String> {
    let mut descriptors = Vec::new();

    let entries =
        fs::read_dir(directory).map_err(|e| format!("read dir {}: {e}", directory.display()))?;

    let id_re = Regex::new(r#"id="([^"]+)""#).map_err(|e| format!("regex: {e}"))?;
    let parent_re = Regex::new(r#"parent="([^"]+)""#).map_err(|e| format!("regex: {e}"))?;
    let lang_re = Regex::new(r#"languages="([^"]*)"#).map_err(|e| format!("regex: {e}"))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("xml") {
            continue;
        }

        let content =
            fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;

        let byte_size = content.len();
        let first_line = content.lines().next().unwrap_or("");

        let id = id_re
            .captures(first_line)
            .map(|c| c[1].to_string())
            .unwrap_or_else(|| {
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            });

        let parent = parent_re
            .captures(first_line)
            .map(|c| c[1].to_string());

        let languages: Vec<String> = lang_re
            .captures(first_line)
            .map(|c| {
                c[1].split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let display_name = id_to_display(&id);

        descriptors.push(Descriptor {
            path,
            id,
            parent,
            languages,
            display_name,
            byte_size,
            kind: kind.clone(),
        });
    }

    Ok(descriptors)
}

fn scan_expertise_xml(directory: &Path) -> Result<Vec<Fragment>, String> {
    let mut fragments = Vec::new();
    let domain_re = Regex::new(r#"domain="([^"]+)""#).map_err(|e| format!("regex: {e}"))?;

    let entries =
        fs::read_dir(directory).map_err(|e| format!("read dir {}: {e}", directory.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("xml") {
            continue;
        }

        let content =
            fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;

        let byte_size = content.len();
        let first_line = content.lines().next().unwrap_or("");

        let (id, _load) = parse_xml_root_attrs(&content, &FragmentCategory::Expertise)?;
        let display_name = id_to_display(&id);

        let domain = domain_re
            .captures(first_line)
            .map(|c| c[1].to_string())
            .unwrap_or_default();

        let coding_related = domain == "coding";

        fragments.push(Fragment {
            path,
            id,
            category: FragmentCategory::Expertise,
            load: LoadPolicy::Manual,
            display_name,
            byte_size,
            coding_related,
        });
    }

    Ok(fragments)
}

fn parse_xml_root_attrs(
    content: &str,
    default_category: &FragmentCategory,
) -> Result<(String, LoadPolicy), String> {
    let id_re = Regex::new(r#"id="([^"]+)""#).map_err(|e| format!("regex: {e}"))?;
    let load_re = Regex::new(r#"load="([^"]+)""#).map_err(|e| format!("regex: {e}"))?;

    let first_line = content.lines().next().unwrap_or("");

    let id = id_re
        .captures(first_line)
        .map(|c| c[1].to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let load = load_re
        .captures(first_line)
        .map(|c| match &c[1] {
            "always" => LoadPolicy::Always,
            "implement" => LoadPolicy::Implement,
            _ => LoadPolicy::Manual,
        })
        .unwrap_or(match default_category {
            FragmentCategory::Persona => LoadPolicy::Manual,
            FragmentCategory::Coding => LoadPolicy::Implement,
            _ => LoadPolicy::Always,
        });

    Ok((id, load))
}

fn id_to_display(id: &str) -> String {
    id.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

