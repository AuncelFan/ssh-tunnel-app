pub struct Identity {
    pub config_directory: &'static str,
    pub startup_name: &'static str,
}

pub const fn for_build(development: bool) -> Identity {
    if development {
        Identity {
            config_directory: ".ssh-tunnel-migration",
            startup_name: "ssh-tunnel-migration",
        }
    } else {
        Identity {
            config_directory: ".ssh-tunnel",
            startup_name: "ssh-tunnel",
        }
    }
}

pub const CURRENT: Identity = for_build(cfg!(debug_assertions));

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_uses_production_identity_and_development_stays_isolated() {
        assert_eq!(for_build(false).config_directory, ".ssh-tunnel");
        assert_eq!(for_build(false).startup_name, "ssh-tunnel");
        assert_eq!(for_build(true).config_directory, ".ssh-tunnel-migration");
        assert_eq!(for_build(true).startup_name, "ssh-tunnel-migration");
    }
}
