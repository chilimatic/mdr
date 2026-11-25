//! Settings from `~/.config/mdr/config.toml`. Command line options win over the file.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::theme::ThemeChoice;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(Default, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// `auto`, `dark` or `light`.
    pub theme: Option<ThemeChoice>,
    /// Maximum text width in columns.
    pub width: Option<u16>,
    /// Show images inline.
    pub images: Option<bool>,
    /// Colors in printed output.
    pub color: Option<ColorChoice>,
    /// Editor command for `e`. Default: `$VISUAL`, `$EDITOR`, `vi`.
    pub editor: Option<String>,
    /// Remember the reading position of each file.
    pub resume: Option<bool>,
    /// Show the row with the keys that work now.
    pub key_bar: Option<bool>,
}

impl Config {
    /// `$MDR_CONFIG`, else `$XDG_CONFIG_HOME/mdr/config.toml`, else `~/.config/mdr/config.toml`.
    pub fn path() -> Option<PathBuf> {
        let nonempty = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        if let Some(p) = nonempty("MDR_CONFIG") {
            return Some(p);
        }
        let dir = nonempty("XDG_CONFIG_HOME").or_else(|| nonempty("HOME").map(|h| h.join(".config")))?;
        Some(dir.join("mdr").join("config.toml"))
    }

    /// Reads the config file. A missing file is an empty config; a broken file is an error.
    pub fn load() -> Result<Self> {
        let Some(path) = Self::path() else { return Ok(Config::default()) };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        };
        Self::parse(&text).with_context(|| format!("error in {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        Ok(toml::from_str(text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_settings() {
        let c = Config::parse(
            "theme = \"light\"\nwidth = 90\nimages = false\ncolor = \"never\"\neditor = \"nvim -n\"\nresume = false\nkey_bar = false\n",
        )
        .unwrap();
        assert_eq!(c.theme, Some(ThemeChoice::Light));
        assert_eq!(c.width, Some(90));
        assert_eq!(c.images, Some(false));
        assert_eq!(c.color, Some(ColorChoice::Never));
        assert_eq!(c.editor.as_deref(), Some("nvim -n"));
        assert_eq!(c.resume, Some(false));
        assert_eq!(c.key_bar, Some(false));
    }

    #[test]
    fn rejects_unknown_keys_and_values() {
        assert!(Config::parse("colour = \"never\"").is_err());
        assert!(Config::parse("theme = \"blue\"").is_err());
        assert!(Config::parse("").is_ok());
    }
}
