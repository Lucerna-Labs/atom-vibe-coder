#![cfg_attr(windows, windows_subsystem = "windows")]

mod model;
mod ui;

#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    // The windows subsystem has no console, so a panic would vanish silently;
    // persist it where an operator (or a gate script) can find it.
    std::panic::set_hook(Box::new(|info| {
        let path = std::env::temp_dir().join("math-atoms-native-panic.log");
        let backtrace = std::backtrace::Backtrace::force_capture();
        let _ = std::fs::write(&path, format!("{info}\n{backtrace}"));
    }));
    win::run();
}

#[cfg(not(windows))]
fn main() {
    println!(
        "Atom Vibe Coder native shell uses the Win32 API. Run on Windows with: cargo run -p math-atoms-native"
    );
}
