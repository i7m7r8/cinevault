use eframe::egui;
use crispy_m3u::{parse, M3uEntry};
use gstreamer_player::Player;
use std::sync::mpsc::{self, Sender};
use std::thread;

fn main() -> Result<(), eframe::Error> {
    gstreamer::init().expect("Failed to initialize GStreamer");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 650.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Rust Live TV + Sports",
        options,
        Box::new(|_cc| Box::new(LiveTvApp::new())),
    )
}

#[derive(Default, Clone)]
struct Channel {
    name: String,
    url: String,
    group: Option<String>,
}

enum PlaybackCommand {
    Play(String),
    Stop,
}

struct LiveTvApp {
    playlist_url: String,
    sports_url: String, // New: stores the sports playlist URL
    channels: Vec<Channel>,
    selected_channel: Option<usize>,
    status: String,
    is_loading: bool,
    playback_tx: Sender<PlaybackCommand>,
}

impl LiveTvApp {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        // Spawn player thread (same as before)
        thread::spawn(move || {
            let player = Player::new(None, None).expect("Failed to create GStreamer player");
            loop {
                match rx.recv() {
                    Ok(PlaybackCommand::Play(uri)) => {
                        player.set_uri(&uri);
                        player.play();
                    }
                    Ok(PlaybackCommand::Stop) => player.stop(),
                    Err(_) => break,
                }
            }
        });

        Self {
            playlist_url: String::new(),
            // Default to a reliable, auto-updated sports playlist (iptv-org)
            sports_url: "https://iptv-org.github.io/iptv/categories/sports.m3u".to_string(),
            channels: Vec::new(),
            selected_channel: None,
            status: "Ready. Load a general or sports playlist.".to_string(),
            is_loading: false,
            playback_tx: tx,
        }
    }

    fn load_playlist_from_text(&mut self, text: &str) {
        match parse(text) {
            Ok(playlist) => {
                self.channels = playlist
                    .entries
                    .into_iter()
                    .filter_map(|entry| {
                        let name = entry.name.clone()?;
                        let url = entry.url.clone();
                        let group = entry.attributes.get("group-title").cloned();
                        Some(Channel { name, url, group })
                    })
                    .collect();
                self.status = format!("Loaded {} channels.", self.channels.len());
                self.selected_channel = None;
            }
            Err(e) => self.status = format!("Failed to parse playlist: {}", e),
        }
    }

    fn load_playlist_from_url(&mut self, url: &str) {
        self.is_loading = true;
        self.status = "Fetching playlist...".to_string();
        let url = url.to_string();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = ureq::get(&url)
                .call()
                .and_then(|r| r.into_string().map_err(|e| ureq::Error::from(e)));
            let _ = tx.send(result);
        });
        if let Ok(result) = rx.recv_timeout(std::time::Duration::from_secs(15)) {
            self.is_loading = false;
            match result {
                Ok(text) => self.load_playlist_from_text(&text),
                Err(e) => self.status = format!("Failed to fetch: {}", e),
            }
        } else {
            self.is_loading = false;
            self.status = "Fetch timed out.".to_string();
        }
    }

    // New helper: loads the sports list using the dedicated sports_url
    fn load_sports(&mut self) {
        if !self.sports_url.trim().is_empty() {
            self.load_playlist_from_url(self.sports_url.trim());
        }
    }
}

impl eframe::App for LiveTvApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("M3U URL:");
                ui.text_edit_singleline(&mut self.playlist_url);
                if ui.button("Load URL").clicked() && !self.is_loading {
                    let url = self.playlist_url.trim();
                    if !url.is_empty() { self.load_playlist_from_url(url); }
                }
                if ui.button("Open File...").clicked() && !self.is_loading {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("M3U Playlist", &["m3u", "m3u8"])
                        .pick_file()
                    {
                        if let Ok(text) = std::fs::read_to_string(&path) {
                            self.load_playlist_from_text(&text);
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                // Sports quick-load section
                ui.label("Sports URL:");
                ui.text_edit_singleline(&mut self.sports_url);
                if ui.button("⚽ Load Sports").clicked() && !self.is_loading {
                    self.load_sports();
                }
            });
            ui.separator();
            ui.label(&self.status);
            if self.is_loading { ui.spinner(); }
        });

        egui::SidePanel::left("channel_list")
            .resizable(true)
            .default_width(260.0)
            .show(ctx, |ui| {
                ui.heading("Channels");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, ch) in self.channels.iter().enumerate() {
                        let is_selected = self.selected_channel == Some(i);
                        let label = match &ch.group {
                            Some(g) => format!("[{}] {}", g, ch.name),
                            None => ch.name.clone(),
                        };
                        if ui.selectable_label(is_selected, label).clicked() {
                            self.selected_channel = Some(i);
                            let _ = self.playback_tx.send(PlaybackCommand::Play(ch.url.clone()));
                            self.status = format!("Playing: {}", ch.name);
                        }
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                if let Some(idx) = self.selected_channel {
                    if let Some(ch) = self.channels.get(idx) {
                        ui.heading(&ch.name);
                        ui.label(&ch.url);
                    }
                } else {
                    ui.label("Select a channel to start playback.");
                }
                ui.add_space(20.0);
                if ui.button("⏹ Stop Playback").clicked() {
                    let _ = self.playback_tx.send(PlaybackCommand::Stop);
                    self.selected_channel = None;
                    self.status = "Stopped.".to_string();
                }
                ui.add_space(30.0);
                ui.label("Video opens in a separate GStreamer window.");
            });
        });
    }
}
