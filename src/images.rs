//! Inline images: terminal protocol detection, sizing and a cache of encoded images.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ratatui::layout::Size;
use ratatui_image::picker::Picker;
use ratatui_image::sliced::SlicedProtocol;

use crate::layout::ImageSizer;

pub struct Images {
    pub picker: Option<Picker>,
    /// Largest image height in rows. Set from the terminal height.
    pub max_rows: u16,
    dims: HashMap<PathBuf, Option<(u32, u32)>>,
    /// One encoded image per file, for the last size asked for (sixel and kitty data is large).
    encoded: HashMap<PathBuf, (u16, u16, Option<SlicedProtocol>)>,
}

impl Images {
    pub fn new(picker: Option<Picker>) -> Self {
        Images { picker, max_rows: 20, dims: HashMap::new(), encoded: HashMap::new() }
    }

    /// Forgets cached data, for example after the file changed on disk.
    pub fn clear(&mut self) {
        self.dims.clear();
        self.encoded.clear();
    }

    /// Returns the encoded image for this size. Decodes and encodes it on first use.
    pub fn protocol(&mut self, path: &Path, cols: u16, rows: u16) -> Option<&SlicedProtocol> {
        let picker = self.picker.as_ref()?;
        let stale = self.encoded.get(path).is_none_or(|(c, r, _)| (*c, *r) != (cols, rows));
        if stale {
            let proto = decode(path).and_then(|img| SlicedProtocol::new(picker, img, Some(Size::new(cols, rows))).ok());
            self.encoded.insert(path.to_path_buf(), (cols, rows, proto));
        }
        self.encoded.get(path).and_then(|(_, _, p)| p.as_ref())
    }
}

/// Largest image mdr decodes, in pixels per side, and the memory the decoder may use.
const MAX_SIDE: u32 = 8192;
const MAX_ALLOC: u64 = 128 << 20;

/// Decodes a regular image file with size limits, so a small file that claims a huge
/// size cannot use gigabytes of memory.
fn decode(path: &Path) -> Option<image::DynamicImage> {
    if !crate::text::is_regular_file(path) {
        return None;
    }
    let mut reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    reader.decode().ok()
}

impl ImageSizer for Images {
    fn size(&mut self, path: &Path, max_cols: u16) -> Option<(u16, u16)> {
        let picker = self.picker.as_ref()?;
        let (w, h) = (*self
            .dims
            .entry(path.to_path_buf())
            .or_insert_with(|| {
                // Too large to decode: the alt text is shown instead.
                let d = crate::text::is_regular_file(path).then(|| image::image_dimensions(path).ok()).flatten()?;
                (d.0 <= MAX_SIDE && d.1 <= MAX_SIDE).then_some(d)
            }))?;
        if w == 0 || h == 0 {
            return None;
        }
        let font = picker.font_size();
        let (fw, fh) = (font.width.max(1) as f64, font.height.max(1) as f64);
        let mut cols = (w as f64 / fw).ceil();
        let mut rows = (h as f64 / fh).ceil();
        let max_cols = max_cols.max(1) as f64;
        let max_rows = self.max_rows.max(1) as f64;
        let scale = (max_cols / cols).min(max_rows / rows).min(1.0);
        cols = (cols * scale).floor().max(1.0);
        rows = (rows * scale).floor().max(1.0);
        Some((cols as u16, rows as u16))
    }
}
