/// Platform login registration is separate from configuration persistence for rollback testing.
pub trait LoginStartup: Send + Sync {
    type Snapshot;
    fn enabled(&self) -> Result<bool, String>;
    fn apply(&self, enabled: bool) -> Result<(), String>;
    fn snapshot(&self) -> Result<Self::Snapshot, String>;
    fn restore(&self, snapshot: Self::Snapshot) -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::Controller;
    use std::sync::atomic::{AtomicBool, Ordering};
    #[derive(Default)]
    struct Platform {
        enabled: AtomicBool,
        fail: AtomicBool,
    }
    impl LoginStartup for Platform {
        type Snapshot = bool;
        fn snapshot(&self) -> Result<bool, String> {
            self.enabled()
        }
        fn restore(&self, snapshot: bool) -> Result<(), String> {
            self.apply(snapshot)
        }
        fn enabled(&self) -> Result<bool, String> {
            Ok(self.enabled.load(Ordering::SeqCst))
        }
        fn apply(&self, enabled: bool) -> Result<(), String> {
            if self.fail.load(Ordering::SeqCst) {
                return Err("Registration denied".into());
            }
            self.enabled.store(enabled, Ordering::SeqCst);
            Ok(())
        }
    }
    #[tokio::test]
    async fn platform_failure_does_not_change_configuration_and_restart_reconciles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let known_hosts = dir.path().join("known_hosts");
        let controller = Controller::open(path.clone(), known_hosts.clone()).unwrap();
        let platform = Platform::default();
        platform.fail.store(true, Ordering::SeqCst);
        assert!(controller.set_auto_run(true, &platform).await.is_err());
        assert!(!controller.snapshot().auto_run);
        assert!(!path.exists());
        platform.fail.store(false, Ordering::SeqCst);
        controller.set_auto_run(true, &platform).await.unwrap();
        assert!(platform.enabled().unwrap());
        drop(controller);
        platform.enabled.store(false, Ordering::SeqCst);
        let controller = Controller::open(path, known_hosts).unwrap();
        controller.reconcile_auto_run(&platform).await.unwrap();
        assert!(platform.enabled().unwrap());
        controller.set_auto_run(false, &platform).await.unwrap();
        assert!(!platform.enabled().unwrap());
        assert!(!controller.snapshot().auto_run);
    }
    #[tokio::test]
    async fn configuration_write_failure_rolls_back_platform_registration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let controller = Controller::open(path.clone(), dir.path().join("known_hosts")).unwrap();
        let platform = Platform::default();
        std::fs::create_dir(path).unwrap();
        assert!(controller.set_auto_run(true, &platform).await.is_err());
        assert!(!platform.enabled().unwrap());
        assert!(!controller.snapshot().auto_run);
    }
}
