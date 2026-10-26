use std::{
    fs::File,
    io::Write,
    path::Path,
    sync::{Mutex, OnceLock},
};

fn open(path: &Path) -> Option<File> {
    if !path.is_absolute() {
        return None;
    }
    File::options().write(true).create_new(true).open(path).ok()
}

/// Opt-in, static lifecycle markers only; never overwrite an existing artifact.
pub fn mark(stage: &'static str) {
    static OUTPUT: OnceLock<Mutex<Option<File>>> = OnceLock::new();
    let output = OUTPUT.get_or_init(|| {
        Mutex::new(
            std::env::var_os("SSH_TUNNEL_STARTUP_TRACE").and_then(|path| open(Path::new(&path))),
        )
    });
    if let Ok(mut output) = output.lock()
        && let Some(file) = output.as_mut()
    {
        let _ = writeln!(file, "{stage}");
        let _ = file.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trace_rejects_relative_paths_and_preserves_existing_files() {
        assert!(open(Path::new("relative.log")).is_none());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.log");
        let mut file = open(&path).unwrap();
        writeln!(file, "test.marker").unwrap();
        drop(file);
        assert!(open(&path).is_none());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "test.marker\n");
    }
}
