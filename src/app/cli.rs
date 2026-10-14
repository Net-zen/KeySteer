//! Command-line parsing and user-facing help.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Default)]
pub(crate) struct CliOptions {
    pub(crate) config: Option<PathBuf>,
    pub(crate) command: CliCommand,
}

// Diagnostic order preserves the existing dump > check > doctor precedence.
// Startup options only modify CliOptions; Run never requests a console.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CliCommand {
    #[default]
    Run,
    Doctor,
    Check,
    DumpConfig,
    Help,
    Version,
}

fn needs_console(args: &Result<CliOptions, String>) -> bool {
    !matches!(args, Ok(options) if options.command == CliCommand::Run)
}

pub fn run_cli() -> ExitCode {
    // A replacement process waits here before logging, hooks, workers or native
    // application initialization. EOF arrives only after the old instance stops.
    let restart_snapshot = if std::env::args_os().nth(1).as_deref()
        == Some(std::ffi::OsStr::new(super::restart::ARGUMENT))
    {
        match super::restart::receive() {
            Ok(snapshot) => Some(snapshot),
            Err(error) => {
                crate::support::logging::emergency_console(format_args!("{error}"));
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    #[cfg(target_os = "windows")]
    if let Some(result) = crate::platform::run_internal_update_helper() {
        return if result.is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    #[cfg(target_os = "windows")]
    if let Some(exit) = run_internal_wechat_helper() {
        return exit;
    }
    let args = if restart_snapshot.is_some() {
        Ok(CliOptions::default())
    } else {
        parse_args(std::env::args_os().skip(1))
    };
    // Internal helpers have already returned. Parse once before preparing any
    // output so GUI options, including their values, never allocate a console.
    if needs_console(&args) {
        #[cfg(target_os = "windows")]
        crate::platform::prepare_console_for_cli();
    }
    let log_path = match crate::support::logging::init(log_candidates()) {
        Ok(path) => Some(path.to_path_buf()),
        Err(error) => {
            crate::report_error!("logging", "{error}");
            None
        }
    };
    crate::support::logging::install_panic_hook();
    let result = match restart_snapshot {
        Some(snapshot) => super::bootstrap::resume(snapshot),
        None => args.and_then(|args| match args.command {
            CliCommand::Help => {
                print_help();
                Ok(None)
            }
            CliCommand::Version => {
                println!("KeySteer {}", env!("CARGO_PKG_VERSION"));
                Ok(None)
            }
            _ => super::bootstrap::run(args),
        }),
    };
    match result {
        Ok(restart) => {
            crate::support::logging::end_session();
            crate::support::logging::flush();
            if let Some(restart) = restart
                && let Err(error) = restart.commit()
            {
                crate::report_error!("reload", "{error}");
                crate::support::logging::flush();
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            crate::report_error!("cli", "{error}");
            if let Some(log_path) = log_path {
                crate::support::logging::emergency_console(format_args!(
                    "diagnostic log: {}",
                    log_path.display()
                ));
            }
            crate::support::logging::flush();
            ExitCode::FAILURE
        }
    }
}

fn log_candidates() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = super::paths::data_file("keysteer.log")
        .into_iter()
        .collect();
    let fallback = std::env::temp_dir().join("KeySteer").join("keysteer.log");
    if !paths.contains(&fallback) {
        paths.push(fallback);
    }
    paths
}

fn parse_args(arguments: impl IntoIterator<Item = OsString>) -> Result<CliOptions, String> {
    let mut args = CliOptions::default();
    let mut iter = arguments.into_iter();

    while let Some(arg) = iter.next() {
        match arg.to_str() {
            Some("-h" | "--help") => {
                args.command = CliCommand::Help;
                break;
            }
            Some("-V" | "--version") => {
                args.command = CliCommand::Version;
                break;
            }
            Some("-c" | "--config") => {
                let path = iter
                    .next()
                    .ok_or_else(|| format!("{} requires a path", arg.to_string_lossy()))?;
                args.config = Some(PathBuf::from(path));
            }
            Some("--check") => args.command = args.command.max(CliCommand::Check),
            Some("--dump-config") => args.command = args.command.max(CliCommand::DumpConfig),
            Some("--doctor") => args.command = args.command.max(CliCommand::Doctor),
            _ => return Err(format!("unknown argument: {}", arg.to_string_lossy())),
        }
    }
    Ok(args)
}

#[cfg(target_os = "windows")]
fn run_internal_wechat_helper() -> Option<ExitCode> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--internal-wechat-ocr-helper")) {
        return None;
    }
    let Some(bridge) = arguments.next() else {
        return Some(ExitCode::FAILURE);
    };
    let Some(component) = arguments.next() else {
        return Some(ExitCode::FAILURE);
    };
    let Some(runtime) = arguments.next() else {
        return Some(ExitCode::FAILURE);
    };
    if arguments.next().is_some() {
        return Some(ExitCode::FAILURE);
    }
    Some(
        if crate::platform::run_internal_wechat_ocr_helper(
            PathBuf::from(bridge),
            PathBuf::from(component),
            PathBuf::from(runtime),
        )
        .is_ok()
        {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        },
    )
}

fn print_help() {
    println!(
        "\
KeySteer {version} — keyboard-driven mouse control

USAGE:
    keysteer [OPTIONS]

OPTIONS:
    -c, --config <PATH>  Use an explicit keysteer.<name>.toml file
        --check          Validate the configuration and exit
        --doctor         Report whether the program can work here, and exit
        --dump-config    Print the effective configuration and exit
    -h, --help           Show this help
    -V, --version        Show the version

MODES:
    idle             Silent resting state; only listens for the keys that
                     launch a mode, so the program stays out of the way
    normal           Move the pointer, click and scroll; entry point to the
                     three targeting modes below
    grid             Full-screen labelled coordinate grid
    recursive_grid   Recursively subdividing grid
    ui_hint          Labels every clickable element

LAUNCHING (from idle):
    Primary+E            normal          Primary is Cmd on macOS,
                                        Ctrl on Windows and Linux

IN NORMAL MODE:
    h j k l          move the pointer
    caps/left-shift  precision / slow; v or b is fast
    ; ' right-shift left / right / middle click
    n                toggle left-button drag
    m ,              scroll down / up
    g f Primary+F    grid / recursive_grid / ui_hint
    t y i u          home / end / page_up / page_down
    q or esc         back to idle

    Everything is rebindable; see the `[normal.bindings]` section.

CONFIGURATION:
    Configuration is optional. Without one, KeySteer uses built-in defaults.
    User profiles take precedence over keysteer.default.toml. Regular files
    and symbolic links to files share the same name ordering, matching:
      keysteer.<name>.toml
    Reload reads the current link target; config edits preserve the link.

    A packaged macOS app reads and writes configuration and keysteer.log in:
      ~/Library/Application Support/KeySteer/
    Bare binaries and Windows portable builds keep them beside the executable.
    With -c, a bare file name is resolved beside the executable; a relative or
    absolute path is used as written.

    `[key_aliases]` applies globally; `[key_aliases.windows|macos|linux]`
    overrides it on one platform. Generic modifiers match both physical sides;
    `left_` / `right_` require one side.

    Built for the {backend} backend.",
        version = env!("CARGO_PKG_VERSION"),
        backend = crate::platform::backend_name(),
    );
    if let Some(path) = crate::support::logging::path() {
        println!(
            "\nDIAGNOSTICS:\n    Always-on runtime log: {}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<CliOptions, String> {
        parse_args(arguments.iter().map(OsString::from))
    }

    #[test]
    fn gui_launch_options_and_option_values_do_not_request_a_console() {
        for arguments in [
            vec![],
            vec!["-c", "keysteer.mine.toml"],
            vec!["--config", "D:/dotfiles/keysteer.mine.toml"],
            vec!["--config", "--doctor"],
        ] {
            let request = parse(&arguments);
            assert!(!needs_console(&request), "{arguments:?}");
            let options = request.unwrap();
            assert_eq!(options.command, CliCommand::Run);
            assert_eq!(options.config, arguments.last().map(PathBuf::from));
        }
    }

    #[test]
    fn output_commands_request_a_console_regardless_of_argument_order() {
        for option in [
            "--help",
            "-h",
            "--version",
            "-V",
            "--check",
            "--dump-config",
            "--doctor",
        ] {
            for arguments in [
                vec![option],
                vec!["--config", "keysteer.mine.toml", option],
                vec![option, "--config", "keysteer.mine.toml"],
            ] {
                let parsed = parse(&arguments);
                assert!(parsed.is_ok(), "{arguments:?}");
                assert!(needs_console(&parsed), "{arguments:?}");
            }
        }
    }

    #[test]
    fn diagnostics_keep_the_config_path_and_existing_combined_options() {
        for commands in [
            ["--doctor", "--dump-config", "--check"],
            ["--dump-config", "--check", "--doctor"],
            ["--check", "--doctor", "--dump-config"],
        ] {
            let mut arguments = vec!["-c", "keysteer.mine.toml"];
            arguments.extend(commands);
            let options = parse(&arguments).unwrap();
            assert_eq!(options.config, Some(PathBuf::from("keysteer.mine.toml")));
            assert_eq!(options.command, CliCommand::DumpConfig);
        }
        for arguments in [["--check", "--doctor"], ["--doctor", "--check"]] {
            assert_eq!(parse(&arguments).unwrap().command, CliCommand::Check);
        }
    }

    #[test]
    fn malformed_arguments_request_a_console_for_the_error() {
        for arguments in [vec!["--unknown"], vec!["-c"], vec!["--config"]] {
            let request = parse(&arguments);
            assert!(request.is_err());
            assert!(needs_console(&request));
        }
    }

    #[test]
    fn application_log_precedes_the_emergency_fallback() {
        let expected = super::super::paths::data_file("keysteer.log").unwrap();
        let paths = log_candidates();
        assert_eq!(paths.first(), Some(&expected));
        assert_eq!(
            paths.last(),
            Some(&std::env::temp_dir().join("KeySteer").join("keysteer.log"))
        );
    }
}
