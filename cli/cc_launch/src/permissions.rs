/// A permission is a set of directories exempt from the hooks' probing check.
/// These are superpowers — granted by PERSONA, never by manual selection.
///
/// The persona→permission mapping is compiled into this binary deliberately:
/// the grant mechanism gets the same protection level as the paths it grants
/// access to. Changing a grant requires editing nornir source and rebuilding
/// via nornir_deploy — there is no config file and no TUI control.
///
/// The actual mechanism is HOOK_LLM_ALLOW_PATHS (colon-separated paths),
/// set once at launch before exec'ing claude. It cannot be added to a
/// running session.
pub struct Permission {
    pub name: &'static str,
    /// Directories exempt from probing (HOOK_LLM_ALLOW_PATHS).
    pub paths: &'static [&'static str],
}

/// Security-system access: enforcement source, deployed tools, the hooks'
/// decision logs, hook config.
/// Tyr operates the security system, so every Tyr launch carries this.
///
/// The paths are under the home directory of whoever builds the binary, taken
/// when it is compiled: the grant is a constant of the binary, and nothing in a
/// launch's environment moves it.
static SECURITY_INFRASTRUCTURE: Permission = Permission {
    name: "Security Infrastructure",
    paths: &[
        concat!(env!("HOME"), "/ai/smidja/nornir/"),
        concat!(env!("HOME"), "/ai/tools/"),
        concat!(env!("HOME"), "/ai/logs/"),
        concat!(env!("HOME"), "/.claude/"),
    ],
};

/// The permission a persona carries at every launch, if any.
///
/// Tyr always — launching Tyr means working on the security system, so the
/// power and the role arrive together. No other persona has any grant, and
/// there is no manual path to one.
pub fn permission_for_persona(persona_id: &str) -> Option<&'static Permission> {
    match persona_id {
        "tyr" => Some(&SECURITY_INFRASTRUCTURE),
        _ => None,
    }
}

/// Build the HOOK_LLM_ALLOW_PATHS value for a persona.
/// Returns None for personas that carry no permission.
pub fn allow_paths_for_persona(persona_id: &str) -> Option<String> {
    permission_for_persona(persona_id).map(|permission| permission.paths.join(":"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tyr_always_has_security_infrastructure() {
        let permission = permission_for_persona("tyr");
        assert!(permission.is_some(), "Tyr must always carry the grant");
        assert_eq!(permission.unwrap().name, "Security Infrastructure");
    }

    #[test]
    fn no_other_persona_has_any_grant() {
        for persona in ["mimir", "bragi", "odinn", "freyja", "loki", ""] {
            assert!(
                permission_for_persona(persona).is_none(),
                "{persona} must not carry any grant"
            );
        }
    }

    #[test]
    fn tyr_allow_paths_are_colon_joined() {
        let paths = allow_paths_for_persona("tyr").unwrap();
        assert!(paths.contains("/ai/smidja/nornir/"));
        assert!(paths.contains(':'));
    }

    #[test]
    fn every_granted_path_is_an_absolute_directory_under_the_builder_s_home() {
        let home = env!("HOME");
        for path in SECURITY_INFRASTRUCTURE.paths {
            assert!(path.starts_with('/'), "{path}");
            assert!(path.starts_with(home), "{path}");
            assert!(path.ends_with('/'), "{path}");
        }
    }

    #[test]
    fn the_grant_does_not_follow_the_launch_environment() {
        let before = allow_paths_for_persona("tyr");
        std::env::set_var("HOME", "/somewhere/else");
        let after = allow_paths_for_persona("tyr");
        std::env::set_var("HOME", env!("HOME"));
        assert_eq!(before, after);
    }

    #[test]
    fn non_tyr_allow_paths_are_none() {
        assert!(allow_paths_for_persona("kvasir").is_none());
    }
}
