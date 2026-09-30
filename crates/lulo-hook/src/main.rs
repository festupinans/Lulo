//! `lulo-hook`: runs on every Claude Code hook event and records the
//! session's current state in `%LOCALAPPDATA%\claude-status\<session_id>.json`.
//!
//! It runs on every tool call, so hook mode does the minimum: read stdin,
//! write one small file, exit 0. It never prints to stdout (Claude Code would
//! parse it) and never fails the hook.

mod install;
mod progress;
mod state;
mod status;
mod usage;

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
lulo-hook: records the live state of each Claude Code session for the Lulo widget.

USAGE:
    lulo-hook                       Double-click: copy itself to a fixed folder and install the hooks.
    lulo-hook hook                  Handle one hook event (JSON on stdin). Used by Claude Code.
    lulo-hook install [OPTIONS]     Add Lulo's hooks to ~/.claude/settings.json (backs it up first).
    lulo-hook uninstall [OPTIONS]   Remove Lulo's hooks from settings.json.
    lulo-hook statusline            Status line mode: save plan usage for the widget, print a line.
    lulo-hook status-dir            Print the folder where session files are written.

OPTIONS:
    --settings <PATH>   settings.json to edit (default: ~/.claude/settings.json)
    --exe <PATH>        Hook binary path to register (default: this executable)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        // Claude Code always pipes JSON in; a terminal on stdin means a person
        // double-clicked the .exe or ran it bare.
        None if std::io::stdin().is_terminal() => {
            let result = double_click_setup();
            if let Err(e) = &result {
                println!("\nNo se pudo instalar: {e}");
            }
            println!("\nPulsa Enter para cerrar.");
            let _ = std::io::stdin().read_line(&mut String::new());
            if result.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        None | Some("hook") => {
            run_hook();
            ExitCode::SUCCESS
        }
        Some("statusline") => {
            run_statusline();
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

fn run_statusline() {
    let mut raw = Vec::with_capacity(4096);
    if std::io::stdin().read_to_end(&mut raw).is_err() {
        return;
    }
    let Ok(input) = serde_json::from_slice(&raw) else {
        return;
    };
    let line = match status::status_dir() {
        Some(dir) => usage::apply(&dir, &input),
        None => usage::line(&input, 0),
    };
    println!("{line}");
}

/// Copies this .exe to a fixed folder (so the registered path survives the
/// download being moved or deleted) and installs the hooks pointing there.
fn double_click_setup() -> Result<(), String> {
    println!("Lulo: instalando el hook de estado para Claude Code...\n");
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = install::default_install_dir().ok_or("no se encontró la carpeta %LOCALAPPDATA%")?;
    let target = dir.join(current.file_name().ok_or("nombre de archivo inválido")?);
    if !same_file(&current, &target) {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::copy(&current, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    let settings =
        install::default_settings_path().ok_or("no se encontró tu carpeta de usuario")?;
    let outcome = install::install(&settings, &target).map_err(|e| e.to_string())?;
    let backup = outcome.backup;

    println!("Listo. Hook copiado en:   {}", target.display());
    println!("Hooks añadidos en:        {}", settings.display());
    if let Some(b) = backup {
        println!("Copia de seguridad en:    {}", b.display());
    }
    if let Some(status) = status::status_dir() {
        println!("Estados de las sesiones:  {}", status.display());
    }
    if !outcome.status_line {
        println!(
            "\nAviso: ya tienes una línea de estado propia, así que no la toqué. \
             Los anillos de uso de Lulo necesitan la de Lulo; quita \"statusLine\" \
             de {} y vuelve a instalar si la quieres.",
            settings.display()
        );
    }
    // Bring the widget along when it was downloaded next to the hook, and open it.
    let widget_name = format!("lulo-widget{}", std::env::consts::EXE_SUFFIX);
    let widget_src = current.with_file_name(&widget_name);
    let widget = dir.join(&widget_name);
    if widget_src.exists() && !same_file(&widget_src, &widget) {
        if let Err(e) = std::fs::copy(&widget_src, &widget) {
            println!(
                "Aviso: no se pudo copiar el widget ({e}). Si está abierto, ciérralo y repite."
            );
        }
    }
    if widget.exists() {
        println!("Widget:                   {}", widget.display());
        if std::process::Command::new(&widget).spawn().is_ok() {
            println!("\nWidget abierto. Clic derecho sobre él para \"Iniciar con Windows\".");
        }
    }

    println!("\nReinicia las sesiones de Claude Code abiertas para que empiecen a reportar.");
    println!("Para desinstalar: \"{}\" uninstall", target.display());
    Ok(())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
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
    let outcome = install::install(&settings, &exe).map_err(|e| e.to_string())?;
    println!("Lulo hooks installed in {}", settings.display());
    println!("Hook binary: {}", exe.display());
    if !outcome.status_line {
        println!("Kept your own statusLine, so the widget's usage rings get no data.");
    }
    if let Some(b) = outcome.backup {
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
