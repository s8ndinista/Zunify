#![cfg_attr(target_os = "emscripten", no_main)]

mod app;
#[cfg(not(target_os = "emscripten"))]
mod audio;
#[cfg(not(target_os = "emscripten"))]
mod desktop;
#[cfg(not(target_os = "emscripten"))]
mod local_media;
mod effects;
mod spectrum;
mod view;
#[cfg(not(target_os = "emscripten"))]
mod spotify;
#[cfg(not(target_os = "emscripten"))]
mod spotify_player;

#[cfg(not(target_os = "emscripten"))]
fn main() -> std::process::ExitCode {
    app::run_desktop()
}
