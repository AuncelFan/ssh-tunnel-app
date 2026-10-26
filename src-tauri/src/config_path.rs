use std::{ffi::OsString, path::PathBuf};

pub const DEFAULT_DIRECTORY: &str = crate::identity::CURRENT.config_directory;

pub fn configuration_path() -> Result<PathBuf, String> {
    select(
        std::env::var_os("SSH_TUNNEL_CONFIG"),
        std::env::args_os().skip(1),
        std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")),
    )
}

fn absolute(raw: OsString, error: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(error.into())
    }
}

fn select(
    override_path: Option<OsString>,
    arguments: impl IntoIterator<Item = OsString>,
    home: Option<OsString>,
) -> Result<PathBuf, String> {
    // Preserve override precedence; invalid explicit paths must never fall back.
    if let Some(path) = override_path {
        return absolute(path, "SSH_TUNNEL_CONFIG must be an absolute path");
    }
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--config" {
            return absolute(
                arguments
                    .next()
                    .ok_or("--config requires an absolute file path")?,
                "--config requires an absolute file path",
            );
        }
    }
    let home = absolute(
        home.ok_or("Cannot locate the home directory")?,
        "Home directory must be an absolute path",
    )?;
    Ok(home.join(DEFAULT_DIRECTORY).join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_environment_wins_without_requiring_home() {
        let dir = tempfile::tempdir().unwrap();
        let expected = dir.path().join("space in config.toml");
        assert_eq!(
            select(
                Some(expected.clone().into_os_string()),
                ["--config".into(), "invalid".into()],
                None
            )
            .unwrap(),
            expected
        );
    }

    #[test]
    fn invalid_environment_never_falls_back_to_valid_cli() {
        let dir = tempfile::tempdir().unwrap();
        for raw in ["", "relative.toml"] {
            assert!(
                select(
                    Some(raw.into()),
                    [
                        "--config".into(),
                        dir.path().join("config.toml").into_os_string()
                    ],
                    Some(dir.path().into())
                )
                .is_err()
            );
        }
    }

    #[test]
    fn cli_accepts_absolute_path_and_rejects_missing_or_relative_value() {
        let dir = tempfile::tempdir().unwrap();
        let expected = dir.path().join("config.toml");
        assert_eq!(
            select(
                None,
                ["--config".into(), expected.clone().into_os_string()],
                None
            )
            .unwrap(),
            expected
        );
        assert!(select(None, ["--config".into()], Some(dir.path().into())).is_err());
        assert!(
            select(
                None,
                ["--config".into(), "relative.toml".into()],
                Some(dir.path().into())
            )
            .is_err()
        );
    }

    #[test]
    fn implicit_path_obeys_build_identity_and_requires_absolute_home() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            select(None, [], Some(dir.path().into())).unwrap(),
            dir.path().join(DEFAULT_DIRECTORY).join("config.toml")
        );
        for home in [None, Some("".into()), Some("relative".into())] {
            assert!(select(None, [], home).is_err());
        }
        assert!(!dir.path().join(DEFAULT_DIRECTORY).exists());
    }
}
