pub mod controller;
pub mod forward;
pub mod hostkeys;
pub mod location;
pub mod login;
pub mod model;
pub mod probe;
pub mod probe_session;
pub mod reconnect;
pub mod repository;
pub mod runtime;
pub mod socks;
pub mod ssh;
pub mod ssh_command;
pub mod ssh_config;
pub mod storage;
pub mod traffic;

/// Local clients can outlive the backend. Keep old approval and import tokens
/// distinct after restart while leaving room for increments below JS MAX_SAFE_INTEGER.
fn generation_counter() -> std::sync::atomic::AtomicU64 {
    (rand::random::<u64>() >> 13).into()
}
