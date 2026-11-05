use egui::Color32;

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Color32,
    pub fg: Color32,
    pub field: Color32,
    pub side: Color32,
    pub sel_bg: Color32,
    pub sel_fg: Color32,
    pub head: Color32,
    pub border: Color32,
    pub entry: Color32,
    pub tab: Color32,
    pub tab_sel: Color32,
    pub menu_bg: Color32,
    pub menu_active: Color32,
    pub dim: Color32,
    pub scroll: Color32,
    pub accent: Color32,
    pub accent_fg: Color32,
    pub hover: Color32,
    pub danger: Color32,
}

pub const LIGHT: Theme = Theme {
    bg: Color32::from_rgb(0xF3, 0xF3, 0xF3),       // Main window background (Mica light)
    fg: Color32::from_rgb(0x1A, 0x1A, 0x1A),       // Primary text (off-black)
    field: Color32::from_rgb(0xFF, 0xFF, 0xFF),    // Text inputs & search boxes
    side: Color32::from_rgb(0xEA, 0xEA, 0xEA),     // Sidebar (slightly darker)
    sel_bg: Color32::from_rgb(0xCC, 0xE4, 0xF7),   // Selection (Win11 light blue)
    sel_fg: Color32::from_rgb(0x00, 0x00, 0x00),   // Text on selection
    head: Color32::from_rgb(0xF3, 0xF3, 0xF3),     // Table headers
    border: Color32::from_rgb(0xE0, 0xE0, 0xE0),   // Subtle dividing lines
    entry: Color32::from_rgb(0xFF, 0xFF, 0xFF),    // List row backgrounds
    tab: Color32::from_rgb(0xE8, 0xE8, 0xE8),      // Inactive tabs
    tab_sel: Color32::from_rgb(0xFF, 0xFF, 0xFF),  // Active tab
    menu_bg: Color32::from_rgb(0xF9, 0xF9, 0xF9),  // Dropdown menus
    menu_active: Color32::from_rgb(0xE5, 0xE5, 0xE5), // Hovered menu item
    dim: Color32::from_rgb(0x6B, 0x6B, 0x6B),      // Secondary text
    scroll: Color32::from_rgb(0xC8, 0xC8, 0xC8),   // Scrollbars
    accent: Color32::from_rgb(0x00, 0x78, 0xD4),   // Win11 Accent Blue
    accent_fg: Color32::from_rgb(0xFF, 0xFF, 0xFF),// Text on accent
    hover: Color32::from_rgb(0xEB, 0xEB, 0xEB),    // Hover background for lists
    danger: Color32::from_rgb(0xC4, 0x2B, 0x1C),   // Delete/Error red
};

pub const DARK: Theme = Theme {
    bg: Color32::from_rgb(0x20, 0x20, 0x20),       // Main window background (Mica dark)
    fg: Color32::from_rgb(0xF0, 0xF0, 0xF0),       // Primary text (soft white)
    field: Color32::from_rgb(0x2D, 0x2D, 0x2D),    // Text inputs & search boxes
    side: Color32::from_rgb(0x1C, 0x1C, 0x1C),     // Sidebar (slightly darker)
    sel_bg: Color32::from_rgb(0x2D, 0x5E, 0x8A),   // Selection (Win11 dark blue)
    sel_fg: Color32::from_rgb(0xFF, 0xFF, 0xFF),   // Text on selection
    head: Color32::from_rgb(0x20, 0x20, 0x20),     // Table headers
    border: Color32::from_rgb(0x33, 0x33, 0x33),   // Subtle dividing lines
    entry: Color32::from_rgb(0x2D, 0x2D, 0x2D),    // List row backgrounds
    tab: Color32::from_rgb(0x1C, 0x1C, 0x1C),      // Inactive tabs
    tab_sel: Color32::from_rgb(0x20, 0x20, 0x20),  // Active tab
    menu_bg: Color32::from_rgb(0x2D, 0x2D, 0x2D),  // Dropdown menus
    menu_active: Color32::from_rgb(0x3A, 0x3A, 0x3A), // Hovered menu item
    dim: Color32::from_rgb(0x9A, 0x9A, 0x9A),      // Secondary text
    scroll: Color32::from_rgb(0x55, 0x55, 0x55),   // Scrollbars
    accent: Color32::from_rgb(0x00, 0x78, 0xD4),   // Win11 Accent Blue
    accent_fg: Color32::from_rgb(0xFF, 0xFF, 0xFF),// Text on accent
    hover: Color32::from_rgb(0x33, 0x33, 0x33),    // Hover background for lists
    danger: Color32::from_rgb(0xFF, 0x4C, 0x4C),   // Delete/Error red
};