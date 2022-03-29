use eframe::{egui, epi};
use once_cell::sync::Lazy;
use std::sync::Arc;

use crate::{ctrl_surf, mpris};

static NO_PLAYER: Lazy<Arc<str>> = Lazy::new(|| "No Player".into());
const STORAGE_PLAYER: &str = "player";

pub enum Response {
    Use(Arc<str>),
    CheckingList,
}

pub struct PlayerWidget {
    list: Vec<Arc<str>>,
    cur: Arc<str>,
    artist: Option<Arc<str>>,
    title: Option<Arc<str>>,
    position: Option<String>,
    texture: Option<egui::TextureHandle>,
    egui_ctx: Option<egui::Context>,
}

impl PlayerWidget {
    pub fn new() -> Self {
        Self {
            list: Vec::new(),
            cur: NO_PLAYER.clone(),
            artist: None,
            title: None,
            position: None,
            texture: None,
            egui_ctx: None,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<Response> {
        use Response::*;

        ui.vertical(|ui| {
            let resp = egui::ComboBox::from_label("Player")
                .selected_text(self.cur.as_ref())
                .show_ui(ui, |ui| {
                    let mut resp = None;
                    for player in self.list.iter() {
                        if ui
                            .selectable_value(&mut self.cur, player.clone(), player.as_ref())
                            .clicked()
                        {
                            resp = Some(Use(player.clone()));
                        }
                    }

                    resp
                })
                .inner;

            ui.add_space(20f32);
            ui.horizontal(|ui| {
                if let Some(ref texture) = self.texture {
                    let av_size = ui.available_size();
                    let img_size = texture.size_vec2();

                    let width = (av_size.x / 2f32).min(img_size.x);
                    let height = img_size.y * width / img_size.x;

                    // FIXME adjust according to the actual remaining height
                    /*
                    dbg!(&av_size, &img_size);
                    if height > av_size.y {
                        height = av_size.y;
                        width = img_size.x * height / img_size.y;
                    }
                    */

                    ui.image(texture, egui::Vec2::new(width, height));
                    ui.separator();
                }

                egui::Grid::new("track").num_columns(2).show(ui, |ui| {
                    ui.label("Artist:");
                    ui.label(self.artist.as_ref().map_or("", Arc::as_ref));
                    ui.end_row();

                    ui.label("Title:");
                    ui.label(self.title.as_ref().map_or("", Arc::as_ref));
                    ui.end_row();

                    ui.label("Position:");
                    ui.label(self.position.as_ref().map_or("--:--", String::as_str));
                    ui.end_row();
                })
            });

            if let Some(None) = resp {
                Some(CheckingList)
            } else {
                resp.flatten()
            }
        })
        .inner
    }

    pub fn setup(&mut self, storage: Option<&dyn epi::Storage>) -> Option<Response> {
        use Response::*;

        if let Some(storage) = storage {
            if let Some(player) = storage.get_string(STORAGE_PLAYER) {
                return Some(Use(player.into()));
            }
        }

        None
    }

    pub fn save(&self, storage: &mut dyn epi::Storage) {
        if self.cur != *NO_PLAYER {
            storage.set_string(STORAGE_PLAYER, self.cur.to_string());
        }
    }
}

impl PlayerWidget {
    pub fn update_players(&mut self, players: &mpris::Players) {
        self.list.clear();

        self.list.extend(players.list());
        self.list.sort();

        if let Some(cur) = players.cur() {
            self.cur = cur;
        } else {
            assert!(self.list.is_empty());
            self.cur = NO_PLAYER.clone();
        }
    }

    pub fn update_track(&mut self, track: &ctrl_surf::Track) {
        self.artist = track.artist.clone();
        self.title = track.title.clone();

        self.texture = track
            .image
            .as_ref()
            .zip(self.egui_ctx.as_ref())
            .map(|(image, ctx)| {
                let size = [image.width() as _, image.height() as _];
                let image_buffer = image.to_rgba8();
                let pixels = image_buffer.as_flat_samples();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());

                ctx.load_texture("my-image", color_image)
            });
    }

    pub fn update_position(&mut self, tc: ctrl_surf::Timecode) {
        self.position = Some(format!("{}", tc));
    }

    pub fn reset_data(&mut self) {
        self.artist = None;
        self.title = None;
        self.position = None;
        self.texture = None;
    }

    pub fn have_context(&mut self, ctx: egui::Context) {
        self.egui_ctx = Some(ctx);
    }
}
