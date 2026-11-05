use eframe::egui;
use std::env;
use keyfm::app::KeyFmApp;

fn main() -> Result<(), eframe::Error> {
    let args: Vec<String> = env::args().collect();
    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            println!("keyfm {} - Keyboard-driven desktop file manager", keyfm::VERSION);
            println!("Usage: keyfm [START_DIR]");
            return Ok(());
        }
        if arg == "-V" || arg == "--version" {
            println!("keyfm {}", keyfm::VERSION);
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
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();
            #[cfg(windows)]
            {
                if let Ok(font_data) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
                    fonts.font_data.insert("segoe".to_owned(), egui::FontData::from_owned(font_data));
                    fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "segoe".to_owned());
                    fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap().insert(0, "segoe".to_owned());
                }
            }
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(KeyFmApp::new(start)))
        }),
    )
}