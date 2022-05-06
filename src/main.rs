pub mod bytes;

pub mod ctrl_surf;
pub use ctrl_surf::ControlSurface;

pub mod midi;
pub mod mpris;
mod ui;

const APP_NAME: &str = "Media Player Controller";

fn main() {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .init();

    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "media-player-controller",
        options,
        Box::new(|cc| Box::new(ui::App::new(APP_NAME, cc))),
    );
}
