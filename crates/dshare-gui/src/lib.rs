//! Configuration GUI built on egui/eframe.
//!
//! Three tabs:
//! - General: role (server/client), bind/connect addr, clipboard toggle
//! - Layout: visual placement of peer screens around the server
//! - Status: connected peers, ping/RTT (TODO)

use dshare_core::config::{Config, Role};
use dshare_core::layout::{Edge, PeerScreen, Screen};
use eframe::egui;
use std::path::PathBuf;
use uuid::Uuid;

pub fn run() -> anyhow::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Dshare",
        opts,
        Box::new(|_cc| Ok(Box::new(App::load_or_default()))),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))
}

struct App {
    config: Config,
    config_path: PathBuf,
    tab: Tab,
    status: String,
}

#[derive(PartialEq, Eq)]
enum Tab {
    General,
    Layout,
    Status,
}

impl App {
    fn load_or_default() -> Self {
        let path = Config::default_path();
        let config = Config::load(&path).unwrap_or_else(|_| Config::default());
        Self {
            config,
            config_path: path,
            tab: Tab::General,
            status: String::new(),
        }
    }

    fn save(&mut self) {
        match self.config.save(&self.config_path) {
            Ok(()) => self.status = format!("saved to {}", self.config_path.display()),
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::General, "General");
                ui.selectable_value(&mut self.tab, Tab::Layout, "Layout");
                ui.selectable_value(&mut self.tab, Tab::Status, "Status");
                ui.separator();
                if ui.button("Save").clicked() {
                    self.save();
                }
                ui.label(&self.status);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::General => general_tab(ui, &mut self.config),
            Tab::Layout => layout_tab(ui, &mut self.config),
            Tab::Status => status_tab(ui),
        });
    }
}

fn general_tab(ui: &mut egui::Ui, cfg: &mut Config) {
    ui.heading("General");
    ui.add_space(8.0);

    egui::Grid::new("general").num_columns(2).show(ui, |ui| {
        ui.label("Role");
        ui.horizontal(|ui| {
            ui.radio_value(&mut cfg.role, Role::Server, "Server (capture)");
            ui.radio_value(&mut cfg.role, Role::Client, "Client (receive)");
        });
        ui.end_row();

        ui.label("Bind address");
        ui.text_edit_singleline(&mut cfg.bind_addr);
        ui.end_row();

        ui.label("Server address");
        let mut server_str = cfg.server_addr.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut server_str).changed() {
            cfg.server_addr = if server_str.is_empty() { None } else { Some(server_str) };
        }
        ui.end_row();

        ui.label("Clipboard sync");
        ui.checkbox(&mut cfg.clipboard_sync, "");
        ui.end_row();
    });
}

fn layout_tab(ui: &mut egui::Ui, cfg: &mut Config) {
    ui.heading("Display Layout");
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.label("Server screen:");
        ui.add(egui::DragValue::new(&mut cfg.layout.server_screen.width).prefix("w "));
        ui.add(egui::DragValue::new(&mut cfg.layout.server_screen.height).prefix("h "));
    });

    ui.separator();
    ui.label("Peers:");

    let mut remove: Option<usize> = None;
    for (i, peer) in cfg.layout.peers.iter_mut().enumerate() {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut peer.name);
                egui::ComboBox::from_id_salt(("edge", i))
                    .selected_text(format!("{:?}", peer.edge))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut peer.edge, Edge::Left, "Left");
                        ui.selectable_value(&mut peer.edge, Edge::Right, "Right");
                        ui.selectable_value(&mut peer.edge, Edge::Top, "Top");
                        ui.selectable_value(&mut peer.edge, Edge::Bottom, "Bottom");
                    });
                ui.add(egui::DragValue::new(&mut peer.offset).prefix("off "));
                ui.add(egui::DragValue::new(&mut peer.screen.width).prefix("w "));
                ui.add(egui::DragValue::new(&mut peer.screen.height).prefix("h "));
                if ui.button("✕").clicked() {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        cfg.layout.peers.remove(i);
    }
    if ui.button("+ Add peer").clicked() {
        cfg.layout.peers.push(PeerScreen {
            peer_id: Uuid::new_v4(),
            name: "peer".into(),
            edge: Edge::Right,
            offset: 0,
            screen: Screen::default(),
        });
    }

    ui.separator();
    ui.label("Preview:");
    layout_preview(ui, &cfg.layout);
}

fn layout_preview(ui: &mut egui::Ui, layout: &dshare_core::layout::Layout) {
    let scale = 0.15_f32;
    let server_size = egui::vec2(
        layout.server_screen.width as f32 * scale,
        layout.server_screen.height as f32 * scale,
    );
    let total_w = server_size.x * 4.0;
    let total_h = server_size.y * 4.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total_w, total_h), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let center = rect.center();

    let server_rect = egui::Rect::from_center_size(center, server_size);
    painter.rect_filled(server_rect, 4.0, egui::Color32::from_rgb(80, 100, 160));
    painter.text(
        server_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Server",
        egui::FontId::default(),
        egui::Color32::WHITE,
    );

    for peer in &layout.peers {
        let peer_size = egui::vec2(
            peer.screen.width as f32 * scale,
            peer.screen.height as f32 * scale,
        );
        let offset_px = peer.offset as f32 * scale;
        let peer_center = match peer.edge {
            Edge::Right => egui::pos2(
                server_rect.right() + peer_size.x / 2.0,
                server_rect.top() + offset_px + peer_size.y / 2.0,
            ),
            Edge::Left => egui::pos2(
                server_rect.left() - peer_size.x / 2.0,
                server_rect.top() + offset_px + peer_size.y / 2.0,
            ),
            Edge::Top => egui::pos2(
                server_rect.left() + offset_px + peer_size.x / 2.0,
                server_rect.top() - peer_size.y / 2.0,
            ),
            Edge::Bottom => egui::pos2(
                server_rect.left() + offset_px + peer_size.x / 2.0,
                server_rect.bottom() + peer_size.y / 2.0,
            ),
        };
        let r = egui::Rect::from_center_size(peer_center, peer_size);
        painter.rect_filled(r, 4.0, egui::Color32::from_rgb(120, 160, 100));
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            &peer.name,
            egui::FontId::default(),
            egui::Color32::WHITE,
        );
    }
}

fn status_tab(ui: &mut egui::Ui) {
    ui.heading("Status");
    ui.label("Connected peers, RTT, throughput will appear here.");
    ui.label("(wired up once daemon exposes a status channel)");
}
