use eframe::{egui, epi};
use once_cell::sync::Lazy;
use std::{sync::Arc, time::Duration};

use crate::{
    ctrl_surf::{self, Timecode},
    mpris,
};

static NO_PLAYER: Lazy<Arc<str>> = Lazy::new(|| "No Player".into());
const STORAGE_PLAYER: &str = "player";

pub enum Response {
    Use(Arc<str>),
    CheckingList,
    Position(Duration),
    PlayPause,
    Previous,
    Next,
}

pub struct PlayerPanel {
    list: Vec<Arc<str>>,
    cur: Arc<str>,
    is_playing: bool,
    artist: Option<Arc<str>>,
    album: Option<Arc<str>>,
    title: Option<Arc<str>>,
    position: Duration,
    position_str: Option<String>,
    duration: Duration,
    duration_str: Option<String>,
    is_pending_seek: bool,
    texture: Option<(Arc<str>, egui::TextureHandle)>,
    egui_ctx: Option<egui::Context>,
}

impl PlayerPanel {
    pub fn new() -> Self {
        Self {
            list: Vec::new(),
            cur: NO_PLAYER.clone(),
            is_playing: false,
            artist: None,
            album: None,
            title: None,
            position: Duration::ZERO,
            position_str: None,
            duration: Duration::ZERO,
            duration_str: None,
            is_pending_seek: false,
            texture: None,
            egui_ctx: None,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<Response> {
        use Response::*;

        let mut resp = None;
        let no_stroke = egui::Frame::default().stroke(egui::Stroke::none());

        let mut margin = ui.spacing().window_margin;
        margin.left = 0.0;
        egui::TopBottomPanel::top("player-selection")
            .frame(no_stroke.margin(margin))
            .show_inside(ui, |ui| {
                let player_resp = egui::ComboBox::from_label("Player")
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

                if let Some(None) = player_resp {
                    resp = Some(CheckingList);
                } else {
                    resp = player_resp.flatten();
                }
            });

        margin.bottom = 0.0;
        egui::TopBottomPanel::bottom("player-progress-and-controls")
            .frame(no_stroke.margin(margin))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut margin = ui.spacing().window_margin;
                    margin.left = 0.0;
                    margin.right = 0.0;
                    margin.top = 0.0;

                    egui::SidePanel::right("player-position-controls")
                        .frame(no_stroke.margin(margin))
                        .show_inside(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.monospace(format!(
                                    "{} / {}",
                                    self.position_str.as_ref().map_or("--:--", String::as_str),
                                    self.duration_str.as_ref().map_or("--:--", String::as_str),
                                ));

                                if ui.button("⏮").clicked() {
                                    resp = Some(Previous);
                                }
                                let play_pause_btn = if self.is_playing {
                                    ui.button("⏸")
                                } else {
                                    ui.button("▶")
                                };
                                if play_pause_btn.clicked() {
                                    resp = Some(PlayPause);
                                }
                                if ui.button("⏭").clicked() {
                                    resp = Some(Next);
                                }
                            });
                        });

                    let mut pos = self.position.as_secs();
                    egui::CentralPanel::default()
                        .frame(no_stroke.margin(margin))
                        .show_inside(ui, |ui| {
                            ui.spacing_mut().slider_width = ui.available_size().x;
                            let mut dur = self.duration.as_secs();
                            if dur == 0 {
                                // Force to one otherwise the slider is centered.
                                dur = 1;
                            }
                            if ui
                                .add(egui::Slider::new(&mut pos, 0..=dur).show_value(false))
                                .changed()
                                && !self.is_pending_seek
                            {
                                self.position = Duration::from_secs(pos);
                                self.is_pending_seek = true;
                                resp = Some(Position(self.position));
                            }
                        });
                });
            });

        let mut margin = ui.spacing().window_margin;
        margin.left = 0.0;
        margin.right = 0.0;
        margin.top *= 1.5;
        margin.bottom = 0.5;
        egui::CentralPanel::default()
            .frame(no_stroke.margin(margin))
            .show_inside(ui, |ui| {
                ui.spacing_mut().item_spacing.x *= 2.0;
                let av_size = ui.available_size();

                if let Some((_, ref texture)) = self.texture {
                    egui::SidePanel::left("player-track-image")
                        .frame(no_stroke)
                        .show_inside(ui, |ui| {
                            let img_size = texture.size_vec2();

                            let mut width = (0.667 * av_size.x).min(img_size.x);
                            let mut height = img_size.y * width / img_size.x;
                            if height > av_size.y {
                                height = av_size.y;
                                width = img_size.x * height / img_size.y;
                            }

                            ui.image(
                                texture,
                                egui::Vec2::new(width.min(img_size.x), height.min(img_size.y)),
                            );
                        });
                }

                egui::CentralPanel::default()
                    .frame(no_stroke)
                    .show_inside(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.heading(self.artist.as_ref().map_or("", Arc::as_ref));
                            ui.separator();
                            ui.heading(self.album.as_ref().map_or("", Arc::as_ref));
                            ui.add_space(20f32);
                            ui.label(self.title.as_ref().map_or("", Arc::as_ref));
                        })
                    });
            });

        resp
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

impl PlayerPanel {
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
        self.album = track.album.clone();
        self.title = track.title.clone();
        self.duration = track.duration.unwrap_or(Duration::ZERO);
        self.duration_str = track.duration.map(Timecode::from).map(|tc| format!("{tc}"));

        match track.image_url {
            Some(ref url) => {
                if self.texture.as_ref().map_or(true, |(cur, _)| cur != url) {
                    if let Some(ref ctx) = self.egui_ctx {
                        let path = url.trim_start_matches("file://");
                        let res = image::io::Reader::open(path)
                            .map_err(|err| {
                                log::warn!("Failed to read image: {err}");
                            })
                            .and_then(|reader| {
                                reader.decode().map_err(|err| {
                                    log::warn!("Failed to decode image: {err}");
                                })
                            });

                        let image = match res {
                            Ok(image) => image,
                            Err(()) => {
                                self.texture = None;
                                return;
                            }
                        };

                        let size = [image.width() as _, image.height() as _];
                        let image_buffer = image.to_rgba8();
                        let pixels = image_buffer.as_flat_samples();
                        let color_image =
                            egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());

                        self.texture =
                            Some((url.clone(), ctx.load_texture("track-image", color_image)));
                    }
                }
            }
            None => self.texture = None,
        }
    }

    pub fn update_position(&mut self, pos: Duration) {
        if self.is_pending_seek {
            return;
        }

        self.position = pos;
        self.position_str = Some(format!("{}", Timecode::from(pos)));
    }

    pub fn reset_pending_seek(&mut self) {
        self.is_pending_seek = false;
    }

    pub fn set_playback_status(&mut self, is_playing: bool) {
        self.is_playing = is_playing;
    }

    pub fn play_pause(&mut self) {
        self.is_playing = !self.is_playing;
    }

    pub fn reset_data(&mut self) {
        self.artist = None;
        self.title = None;
        self.position = Duration::ZERO;
        self.position_str = None;
        self.duration = Duration::ZERO;
        self.duration_str = None;
        self.is_pending_seek = false;
        self.texture = None;
    }

    pub fn have_context(&mut self, ctx: egui::Context) {
        self.egui_ctx = Some(ctx);
    }
}
