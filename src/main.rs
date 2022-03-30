pub mod bytes;

pub mod ctrl_surf;
pub use ctrl_surf::ControlSurface;

pub mod midi;
pub mod mpris;
mod ui;

fn main() {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .init();

    match ui::App::try_new("MPRIS Controller").map(|ui| {
        let options = eframe::NativeOptions::default();
        eframe::run_native(Box::new(ui), options);
    }) {
        Ok(()) => log::info!("Exiting"),
        Err(err) => {
            use std::error::Error;

            log::error!("Error: {}", err);
            if let Some(source) = err.source() {
                log::error!("\t{}", source)
            }
        }
    }
}
