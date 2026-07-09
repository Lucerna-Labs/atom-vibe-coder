mod model;
mod ui;

#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(not(windows))]
fn main() {
    println!(
        "Math Atoms Coder native shell uses the Win32 API. Run on Windows with: cargo run -p math-atoms-native"
    );
}
