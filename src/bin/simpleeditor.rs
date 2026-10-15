// On Windows this binary is the GUI launcher (Explorer, Start menu,
// shortcuts): no console window opens. `se.exe` stays a console program so
// that `se --help` and error messages are visible in a terminal.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    simpleeditor::run("simpleeditor")
}
