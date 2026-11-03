use egui::Color32;

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Color32,
    pub fg: Color32,
    pub accent: Color32,
    pub selection: Color32,
    pub border: Color32,
}

pub const LIGHT: Theme = Theme {
    bg: Color32::from_rgb(248, 249, 250),
    fg: Color32::from_rgb(33, 37, 41),
    accent: Color32::from_rgb(13, 110, 253),
    selection: Color32::from_rgb(222, 226, 230),
    border: Color32::from_rgb(206, 212, 218),
};

pub const DARK: Theme = Theme {
    bg: Color32::from_rgb(33, 37, 41),
    fg: Color32::from_rgb(248, 249, 250),
    accent: Color32::from_rgb(13, 110, 253),
    selection: Color32::from_rgb(73, 80, 87),
    border: Color32::from_rgb(52, 58, 64),
};
