#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
mod control;
#[cfg(windows)]
mod process;

#[cfg(windows)]
fn main() -> eframe::Result {
    let Some(_instance) =
        process::single_instance().map_err(|error| eframe::Error::AppCreation(Box::new(error)))?
    else {
        return Ok(());
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 840.0])
            .with_min_inner_size([760.0, 540.0])
            .with_window_level(eframe::egui::WindowLevel::AlwaysOnTop),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Search Probes",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The Search Probes game UI runs on Windows or inside the game's Proton prefix.");
}
