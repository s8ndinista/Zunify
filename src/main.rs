#![cfg_attr(target_os = "emscripten", no_main)]

mod app;
#[cfg(not(target_os = "emscripten"))]
mod desktop;
mod effects;
#[cfg(not(target_os = "emscripten"))]
mod spotify;

#[cfg(not(target_os = "emscripten"))]
fn main() -> std::process::ExitCode {
    app::run_desktop()
}
