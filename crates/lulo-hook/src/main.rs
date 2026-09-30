//! `lulo-hook`: runs on every Claude Code hook event and records the
//! session's current state in `%LOCALAPPDATA%\claude-status\<session_id>.json`.
//!
//! It runs on every tool call, so hook mode does the minimum: read stdin,
//! write one small file, exit 0. It never prints to stdout (Claude Code would
//! parse it) and never fails the hook.

mod install;
mod state;
mod status;

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
lulo-hook: records the live state of each Claude Code session for the Lulo widget.

USAGE:
    lulo-hook hook                  Handle one hook event (JSON on stdin). Used by Claude Code.
    lulo-hook install [OPTIONS]     Add Lulo's hooks to ~/.claude/settings.json (backs it up first).
    lulo-hook uninstall [OPTIONS]   Remove Lulo's hooks from settings.json.
    lulo-hook status-dir            Print the folder where session files are written.

OPTIONS:
    --settings <PATH>   settings.json to edit (default: ~/.claude/settings.json)
    --exe <PATH>        Hook binary path to register (default: this executable)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("hook") => {
            run_hook();
            ExitCode::SUCCESS
        }
        Some("install") => report(cmd_install(&args[1..])),
        Some("uninstall") => report(cmd_uninstall(&args[1..])),
        Some("status-dir") => match status::status_dir() {
            Some(dir) => {
                println!("{}", dir.display());
                ExitCode::SUCCESS
            }
            None => report(Err("could not determine the status folder".into())),
        },
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprint!("unknown command: {other}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run_hook() {
    let mut raw = Vec::with_capacity(4096);
    if std::io::stdin().read_to_end(&mut raw).is_err() {
        return;
    }
    let Ok(input) = serde_json::from_slice(&raw) else {
        return;
    };
    let Some(dir) = status::status_dir() else {
        return;
    };
    if let Err(e) = status::apply(&dir, &input) {
        // Stderr is only shown in Claude Code's debug output for exit 0.
        eprintln!("lulo-hook: {e}");
    }
}

fn cmd_install(args: &[String]) -> Result<(), String> {
    let settings = settings_arg(args)?;
    let exe = match flag_value(args, "--exe")? {
        Some(p) => PathBuf::from(p),
        None => {
            std::env::current_exe().map_err(|e| format!("could not locate this executable: {e}"))?
        }
    };
    let backup = install::install(&settings, &exe).map_err(|e| e.to_string())?;
    println!("Lulo hooks installed in {}", settings.display());
    println!("Hook binary: {}", exe.display());
    if let Some(b) = backup {
        println!("Backup of the previous file: {}", b.display());
    }
    println!("Open sessions pick up the change after a restart (or /hooks).");
    Ok(())
}

fn cmd_uninstall(args: &[String]) -> Result<(), String> {
    let settings = settings_arg(args)?;
    match install::uninstall(&settings).map_err(|e| e.to_string())? {
        Some(b) => println!(
            "Lulo hooks removed from {} (backup: {})",
            settings.display(),
            b.display()
        ),
        None => println!("No Lulo hooks found in {}", settings.display()),
    }
    Ok(())
}

fn settings_arg(args: &[String]) -> Result<PathBuf, String> {
    match flag_value(args, "--settings")? {
        Some(p) => Ok(PathBuf::from(p)),
        None => install::default_settings_path()
            .ok_or_else(|| "could not find your home folder; pass --settings".into()),
    }
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Result<Option<&'a str>, String> {
    for (i, a) in args.iter().enumerate() {
        if a == flag {
            return args
                .get(i + 1)
                .map(|v| Some(v.as_str()))
                .ok_or_else(|| format!("{flag} needs a value"));
        }
        if !matches!(a.as_str(), "--settings" | "--exe") && a.starts_with("--") {
            return Err(format!("unknown option: {a}"));
        }
    }
    Ok(None)
}

fn report(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lulo-hook: {e}");
            ExitCode::FAILURE
        }
    }
}
