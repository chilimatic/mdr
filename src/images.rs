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
    encoded: HashMap<(PathBuf, u16, u16), Option<SlicedProtocol>>,
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
        self.encoded
            .entry((path.to_path_buf(), cols, rows))
            .or_insert_with(|| {
                let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
                SlicedProtocol::new(picker, img, Some(Size::new(cols, rows))).ok()
            })
            .as_ref()
    }
}

impl ImageSizer for Images {
    fn size(&mut self, path: &Path, max_cols: u16) -> Option<(u16, u16)> {
        let picker = self.picker.as_ref()?;
        let (w, h) = (*self
            .dims
            .entry(path.to_path_buf())
            .or_insert_with(|| image::image_dimensions(path).ok()))?;
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
