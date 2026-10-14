//! KeySteer binary entry point.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    keysteer::app::run_cli()
}
