//! MPP (Machine Payments Protocol) support for 402-gated RPC endpoints.
//!
//! - [`keys`]: Auto-discovery of signing keys from the Tempo wallet.
//! - [`transport`]: HTTP transport that handles 402 challenges automatically.

pub mod keys;
pub mod persist;
pub mod session;
pub mod transport;
pub mod ws;

#[cfg(test)]
pub(crate) mod test_env {
    use std::sync::OnceLock;
    use tokio::sync::{Mutex, MutexGuard};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    pub(crate) fn lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.get_or_init(|| Mutex::new(())).blocking_lock()
    }

    pub(crate) async fn lock_async() -> MutexGuard<'static, ()> {
        ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await
    }
}
