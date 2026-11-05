//! Native Explorer icons.
//!
//! On Windows the icons come straight from the shell (`SHGetFileInfoW`), so
//! folders, drives and every file type look exactly like they do in Explorer.
//! They are converted to RGBA and cached as egui textures.
//!
//! On other platforms `IconCache::get` always returns `None` and the caller
//! draws its own fallback.

use std::path::Path;

/// What to fetch an icon for.
#[derive(Clone, Copy)]
pub enum IconKind<'a> {
    /// The generic folder icon.
    Folder,
    /// A file. `name` decides the type (extension); `path` is only used for
    /// files that have their own icon (.exe, .lnk, .ico ...).
    File { name: &'a str, path: &'a Path },
    /// A drive root such as `C:\`.
    Drive(&'a Path),
}

pub struct IconCache {
    #[cfg(windows)]
    map: std::collections::HashMap<String, Option<egui::TextureHandle>>,
    /// How many per-file icons (which touch the disk) may still load this frame.
    #[cfg(windows)]
    budget: u32,
}

impl Default for IconCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(windows))]
impl IconCache {
    pub fn new() -> Self {
        Self {}
    }

    pub fn begin_frame(&mut self) {}

    pub fn get(&mut self, _ctx: &egui::Context, _kind: IconKind<'_>) -> Option<egui::TextureHandle> {
        None
    }
}

#[cfg(windows)]
struct Query {
    key: String,
    path: String,
    attrs: u32,
    /// true = describe the icon by name/attributes only (never touches the disk)
    use_attrs: bool,
    overlays: bool,
}

/// Extensions whose icon is stored in (or computed from) the file itself.
#[cfg(windows)]
const PER_FILE_EXTS: &[&str] = &["exe", "lnk", "ico", "cur", "ani", "scr", "url", "appref-ms"];

#[cfg(windows)]
impl IconCache {
    pub fn new() -> Self {
        Self { map: std::collections::HashMap::new(), budget: 0 }
    }

    /// Call once per frame.
    pub fn begin_frame(&mut self) {
        self.budget = 6;
        if self.map.len() > 3000 {
            self.map.clear();
        }
    }

    pub fn get(&mut self, ctx: &egui::Context, kind: IconKind<'_>) -> Option<egui::TextureHandle> {
        let (generic, specific): (Query, Option<Query>) = match kind {
            IconKind::Folder => (
                Query {
                    key: "dir".to_string(),
                    path: "folder".to_string(),
                    attrs: 0x10, // FILE_ATTRIBUTE_DIRECTORY
                    use_attrs: true,
                    overlays: false,
                },
                None,
            ),
            IconKind::Drive(p) => (
                Query {
                    key: format!("drive:{}", p.display()),
                    path: p.to_string_lossy().to_string(),
                    attrs: 0,
                    use_attrs: false,
                    overlays: false,
                },
                None,
            ),
            IconKind::File { name, path } => {
                let ext = Path::new(name)
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let generic = Query {
                    key: format!("ext:{}", ext),
                    path: if ext.is_empty() { "file".to_string() } else { format!("file.{}", ext) },
                    attrs: 0x80, // FILE_ATTRIBUTE_NORMAL
                    use_attrs: true,
                    overlays: false,
                };
                let specific = if PER_FILE_EXTS.contains(&ext.as_str()) {
                    Some(Query {
                        key: format!("file:{}", path.display()),
                        path: path.to_string_lossy().to_string(),
                        attrs: 0,
                        use_attrs: false,
                        overlays: ext == "lnk", // show the shortcut arrow
                    })
                } else {
                    None
                };
                (generic, specific)
            }
        };

        if let Some(q) = specific {
            match self.map.get(&q.key).cloned() {
                Some(Some(tex)) => return Some(tex),
                Some(None) => {} // failed before: use the generic icon
                None => {
                    if self.budget > 0 {
                        self.budget -= 1;
                        let tex = load(ctx, &q);
                        self.map.insert(q.key.clone(), tex.clone());
                        if tex.is_some() {
                            return tex;
                        }
                    } else {
                        // Out of budget this frame: show the generic icon and come back.
                        ctx.request_repaint();
                    }
                }
            }
        }
        self.lookup(ctx, &generic)
    }

    fn lookup(&mut self, ctx: &egui::Context, q: &Query) -> Option<egui::TextureHandle> {
        if let Some(cached) = self.map.get(&q.key) {
            return cached.clone();
        }
        let tex = load(ctx, q);
        self.map.insert(q.key.clone(), tex.clone());
        tex
    }
}

#[cfg(windows)]
fn load(ctx: &egui::Context, q: &Query) -> Option<egui::TextureHandle> {
    let (w, h, rgba) = win::fetch_icon(&q.path, q.attrs, q.use_attrs, q.overlays)?;
    let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
    Some(ctx.load_texture(format!("icon:{}", q.key), img, egui::TextureOptions::LINEAR))
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::ptr::null_mut;

    const SHGFI_ICON: u32 = 0x0000_0100;
    const SHGFI_SMALLICON: u32 = 0x0000_0001;
    const SHGFI_USEFILEATTRIBUTES: u32 = 0x0000_0010;
    const SHGFI_ADDOVERLAYS: u32 = 0x0000_0020;

    // pshpack1 on 32-bit Windows, natural alignment on 64-bit.
    #[repr(C)]
    #[cfg_attr(target_pointer_width = "32", repr(packed(1)))]
    struct SHFILEINFOW {
        h_icon: *mut c_void,
        i_icon: i32,
        dw_attributes: u32,
        sz_display_name: [u16; 260],
        sz_type_name: [u16; 80],
    }

    #[repr(C)]
    struct ICONINFO {
        f_icon: i32,
        x_hotspot: u32,
        y_hotspot: u32,
        hbm_mask: *mut c_void,
        hbm_color: *mut c_void,
    }

    #[repr(C)]
    struct BITMAP {
        bm_type: i32,
        bm_width: i32,
        bm_height: i32,
        bm_width_bytes: i32,
        bm_planes: u16,
        bm_bits_pixel: u16,
        bm_bits: *mut c_void,
    }

    #[repr(C)]
    struct BITMAPINFOHEADER {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_x_pels_per_meter: i32,
        bi_y_pels_per_meter: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    #[repr(C)]
    struct BITMAPINFO {
        header: BITMAPINFOHEADER,
        colors: [u32; 1],
    }

    #[link(name = "shell32")]
    extern "system" {
        fn SHGetFileInfoW(
            path: *const u16,
            file_attributes: u32,
            psfi: *mut SHFILEINFOW,
            cb_file_info: u32,
            flags: u32,
        ) -> usize;
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetIconInfo(hicon: *mut c_void, piconinfo: *mut ICONINFO) -> i32;
        fn DestroyIcon(hicon: *mut c_void) -> i32;
        fn GetDC(hwnd: *mut c_void) -> *mut c_void;
        fn ReleaseDC(hwnd: *mut c_void, hdc: *mut c_void) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn GetObjectW(h: *mut c_void, c: i32, pv: *mut c_void) -> i32;
        fn GetDIBits(
            hdc: *mut c_void,
            hbm: *mut c_void,
            start: u32,
            lines: u32,
            bits: *mut c_void,
            bmi: *mut BITMAPINFO,
            usage: u32,
        ) -> i32;
        fn DeleteObject(ho: *mut c_void) -> i32;
    }

    /// Returns (width, height, RGBA pixels) of the small shell icon for `path`.
    pub fn fetch_icon(
        path: &str,
        attrs: u32,
        use_attrs: bool,
        overlays: bool,
    ) -> Option<(usize, usize, Vec<u8>)> {
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut info: SHFILEINFOW = unsafe { zeroed() };
        let mut flags = SHGFI_ICON | SHGFI_SMALLICON;
        if use_attrs {
            flags |= SHGFI_USEFILEATTRIBUTES;
        }
        if overlays {
            flags |= SHGFI_ADDOVERLAYS;
        }
        let rc = unsafe {
            SHGetFileInfoW(wide.as_ptr(), attrs, &mut info, size_of::<SHFILEINFOW>() as u32, flags)
        };
        let hicon = info.h_icon;
        if rc == 0 || hicon.is_null() {
            return None;
        }
        let out = unsafe { hicon_to_rgba(hicon) };
        unsafe {
            DestroyIcon(hicon);
        }
        out
    }

    unsafe fn hicon_to_rgba(hicon: *mut c_void) -> Option<(usize, usize, Vec<u8>)> {
        let mut ii: ICONINFO = zeroed();
        if GetIconInfo(hicon, &mut ii) == 0 {
            return None;
        }
        let (color, mask) = (ii.hbm_color, ii.hbm_mask);
        let out = bitmaps_to_rgba(color, mask);
        // GetIconInfo hands us ownership of both bitmaps.
        if !color.is_null() {
            DeleteObject(color);
        }
        if !mask.is_null() {
            DeleteObject(mask);
        }
        out
    }

    unsafe fn bitmaps_to_rgba(color: *mut c_void, mask: *mut c_void) -> Option<(usize, usize, Vec<u8>)> {
        if color.is_null() {
            return None; // old monochrome icon: not worth supporting
        }
        let mut bm: BITMAP = zeroed();
        if GetObjectW(color, size_of::<BITMAP>() as i32, &mut bm as *mut BITMAP as *mut c_void) == 0 {
            return None;
        }
        let (w, h) = (bm.bm_width, bm.bm_height);
        if w <= 0 || h <= 0 || w > 256 || h > 256 {
            return None;
        }
        let hdc = GetDC(null_mut());
        if hdc.is_null() {
            return None;
        }
        let color_px = read_bits(hdc, color, w, h);
        let mask_px = if mask.is_null() { None } else { read_bits(hdc, mask, w, h) };
        ReleaseDC(null_mut(), hdc);

        let color_px = color_px?;
        let n = (w * h) as usize;
        // Icons without an alpha channel use the mask bitmap for transparency.
        let has_alpha = color_px.chunks_exact(4).any(|p| p[3] != 0);
        let mut rgba = Vec::with_capacity(n * 4);
        for i in 0..n {
            let p = &color_px[i * 4..i * 4 + 4]; // BGRA
            let a = if has_alpha {
                p[3]
            } else if mask_px.as_ref().map_or(false, |m| m[i * 4] != 0) {
                0
            } else {
                255
            };
            rgba.extend_from_slice(&[p[2], p[1], p[0], a]);
        }
        Some((w as usize, h as usize, rgba))
    }

    /// Reads a bitmap as top-down 32-bit BGRA.
    unsafe fn read_bits(hdc: *mut c_void, bmp: *mut c_void, w: i32, h: i32) -> Option<Vec<u8>> {
        let mut bmi: BITMAPINFO = zeroed();
        bmi.header.bi_size = size_of::<BITMAPINFOHEADER>() as u32;
        bmi.header.bi_width = w;
        bmi.header.bi_height = -h; // negative = top-down
        bmi.header.bi_planes = 1;
        bmi.header.bi_bit_count = 32;
        bmi.header.bi_compression = 0; // BI_RGB
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(hdc, bmp, 0, h as u32, buf.as_mut_ptr() as *mut c_void, &mut bmi, 0);
        if lines == 0 {
            None
        } else {
            Some(buf)
        }
    }
}
