use eframe::egui;
use std::env;
use keyfm::config::{load_config, save_config, Config};
use keyfm::theme::{LIGHT, DARK, Theme};
use keyfm::helpers::get_start_dir;

struct KeyFmApp {
    config: Config,
    theme: Theme,
    start_dir: std::path::PathBuf,
}

impl KeyFmApp {
    fn new(start: Option<String>) -> Self {
        let config = load_config();
        let theme = if config.dark { DARK } else { LIGHT };
        let start_dir = get_start_dir(start);
        Self { config, theme, start_dir }
    }
}

impl eframe::App for KeyFmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Toggle theme with Ctrl+Shift+D for testing Phase 1
        if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::D)) {
            self.config.dark = !self.config.dark;
            self.theme = if self.config.dark { DARK } else { LIGHT };
            save_config(&self.config);
        }

        eg::CentralPanel::default().show(ctx, |ui| {
            ui.heading("keyfm 1.3.0");
            ui.label(format!("Start dir: {:?}", self.start_dir));
            ui.label(format!("Dark mode: {} (Press Ctrl+Shift+D to toggle)", self.config.dark));
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let args: Vec<String> = env::args().collect();
    for arg in &args {
        if arg == "-h" || arg == "--help" {
            println!("keyfm 1.3.0 - Keyboard-driven desktop file manager");
            println!("Usage: keyfm [START_DIR]");
            return Ok(());
        }
        if arg == "-V" || arg == "--version" {
            println!("keyfm 1.3.0");
            return Ok(());
        }
    }

    let start = args.get(1).cloned();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "keyfm",
        options,
        Box::new(|_cc| Box::new(KeyFmApp::new(start))),
    )
}
