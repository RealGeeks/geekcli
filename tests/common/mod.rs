//! Shared helpers for integration tests.
//!
//! Keeps child-process environments clean while preserving `LLVM_PROFILE_FILE`
//! so `cargo llvm-cov` collects coverage from spawned `geekcli` processes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;

/// Clear the environment for a deterministic CLI run while keeping platform
/// basics (`PATH`, `SYSTEMROOT`) and `LLVM_PROFILE_FILE` when running under
/// `cargo llvm-cov`.
pub fn clean_env(mut cmd: Command) -> Command {
    cmd.env_clear()
        .env(
            "SYSTEMROOT",
            std::env::var("SYSTEMROOT").unwrap_or_default(),
        )
        .env("PATH", std::env::var("PATH").unwrap_or_default());
    if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
        cmd.env("LLVM_PROFILE_FILE", profile);
    }
    cmd
}

/// A `geekcli` command with a clean environment (see [`clean_env`]).
pub fn geekcli() -> Command {
    clean_env(Command::cargo_bin("geekcli").unwrap())
}
