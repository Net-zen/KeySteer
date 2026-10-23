#![forbid(unsafe_code)]

//! Application assembly and runtime orchestration.

mod bootstrap;
mod cli;
pub(crate) mod configuration;
pub(crate) mod mode_catalog;
pub(crate) mod paths;
mod preset_store;
mod restart;
#[allow(dead_code, unused_imports)]
pub(crate) mod runtime;

pub fn run_cli() -> std::process::ExitCode {
    crate::platform::run(cli::run_cli)
}
