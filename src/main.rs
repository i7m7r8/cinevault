use eframe::egui;
use crispy_m3u::parse;
use gstreamer_player::{Player, PlayerGMainContextSignalDispatcher, PlayerSignalDispatcher};
use std::sync::mpsc::{self, Receiver, Sender};

fn main() -> Result<(), eframe::Error> {
    // Initialize GStreamer
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

struct LiveTvApp {
    playlist_url: String,
    sports_url: String,
    channels: Vec<Channel>,
    selected_channel: Option<usize>,
    status: String,
    is_loading: bool,
    // Holds the player on the main thread
    player: Player,
    // Channel to receive fetched playlist text from the background thread
    fetch_tx: Sender<Result<String, String>>,
    fetch_rx: Receiver<Result<String, String>>,
}

impl LiveTvApp {
    fn new() -> Self {
        // 1. Initialize GStreamer Player with a signal dispatcher
        let dispatcher = PlayerGMainContextSignalDispatcher::new(None);
        let player = Player::new(
            None::<&gstreamer_player::PlayerVideoRenderer>,
            Some(&dispatcher.upcast::<PlayerSignalDispatcher>()),
        );

        // 2. Set up channel for background fetching
        let (fetch_tx, fetch_rx) = mpsc::channel();

        Self {
            playlist_url: String::new(),
            sports_url: "https://iptv-org.github.io/iptv/categories/sports.m3u".to_string(),
            channels: Vec::new(),
            selected_channel: None,
            status: "Ready. Load a general or sports playlist.".to_string(),
            is_loading: false,
            player,
            fetch_tx,
            fetch_rx,
        }
    }

    fn load_playlist_from_text(&mut self, text: &str) {
        match parse(text) {
            Ok(playlist) => {
                self.channels = playlist
                    .entries
                    .into_iter()
                    .filter_map(|entry| {
                        // Access fields directly as per crispy-m3u 0.1.1
                        let name = entry.name?;
                        let url = entry.url;
                        let group = entry.group_title;
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
        let tx = self.fetch_tx.clone();
        
        // Spawn background thread for network request
        std::thread::spawn(move || {
            let result = ureq::get(&url)
                .call()
                .map_err(|e| e.to_string())
                .and_then(|r| r.into_string().map_err(|e| e.to_string()));
            let _ = tx.send(result);
        });
    }
}

impl eframe::App for LiveTvApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check if the background thread finished fetching
        if self.is_loading {
            if let Ok(result) = self.fetch_rx.try_recv() {
                self.is_loading = false;
                match result {
                    Ok(text) => self.load_playlist_from_text(&text),
                    Err(e) => self.status = format!("Failed to fetch: {}", e),
                }
            } else {
                // Request a repaint so the UI updates when fetch finishes
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }

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
                ui.label("Sports URL:");
                ui.text_edit_singleline(&mut self.sports_url);
                if ui.button("⚽ Load Sports").clicked() && !self.is_loading {
                    let url = self.sports_url.trim();
                    if !url.is_empty() { self.load_playlist_from_url(url); }
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
                            // Play using the main thread player
                            self.player.set_uri(&ch.url);
                            self.player.play();
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
                    self.player.stop();
                    self.selected_channel = None;
                    self.status = "Stopped.".to_string();
                }
                ui.add_space(30.0);
                ui.label("Video opens in a separate GStreamer window.");
            });
        });
    }
}
