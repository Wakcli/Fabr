use alloc::collections::BTreeMap;
use core::fmt;
use std::path::Path;

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

/// Straight alpha color in sRGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel.
    pub a: u8,
}

impl Rgba {
    /// Parses `#RGB`, `#RRGGBB`, or `#RRGGBBAA`; the `#` is optional.
    pub fn parse_hex(hex: &str) -> Result<Self, ThemeError> {
        let src = hex.strip_prefix('#').unwrap_or(hex);
        if !src.is_ascii() {
            return Err(ThemeError::Hex(hex.into()));
        }
        let bytes = src.as_bytes();
        match bytes.len() {
            3 => Ok(Self {
                r: nib(bytes[0]).ok_or_else(|| ThemeError::Hex(hex.into()))? * 17,
                g: nib(bytes[1]).ok_or_else(|| ThemeError::Hex(hex.into()))? * 17,
                b: nib(bytes[2]).ok_or_else(|| ThemeError::Hex(hex.into()))? * 17,
                a: 255,
            }),
            6 | 8 => Ok(Self {
                r: pair(bytes, 0).ok_or_else(|| ThemeError::Hex(hex.into()))?,
                g: pair(bytes, 2).ok_or_else(|| ThemeError::Hex(hex.into()))?,
                b: pair(bytes, 4).ok_or_else(|| ThemeError::Hex(hex.into()))?,
                a: if bytes.len() == 8 {
                    pair(bytes, 6).ok_or_else(|| ThemeError::Hex(hex.into()))?
                } else {
                    255
                },
            }),
            _ => Err(ThemeError::Hex(hex.into())),
        }
    }

    /// Uppercase `#RRGGBB`, or `#RRGGBBAA` when not fully opaque.
    pub fn to_hex(&self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }
}

fn nib(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn pair(bytes: &[u8], at: usize) -> Option<u8> {
    Some(nib(bytes[at])? << 4 | nib(bytes[at + 1])?)
}

impl Serialize for Rgba {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let hex = String::deserialize(deserializer)?;
        Self::parse_hex(&hex).map_err(de::Error::custom)
    }
}

/// A named token palette layered over a base palette.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Theme {
    /// Palette the tokens are designed against, for example `dark`.
    pub base: String,
    /// Semantic token to color.
    pub colors: BTreeMap<String, Rgba>,
}

impl Theme {
    /// Parses a theme from RON.
    pub fn load_from_str(src: &str) -> Result<Self, ThemeError> {
        ron::from_str(src).map_err(|e| ThemeError::Ron(e.to_string()))
    }

    /// Reads and parses a theme file.
    pub fn load_from_path(path: &Path) -> Result<Self, ThemeError> {
        let src = std::fs::read_to_string(path).map_err(|e| ThemeError::Io(e.to_string()))?;
        Self::load_from_str(&src)
    }

    /// Color for a token.
    pub fn get(&self, token: &str) -> Option<Rgba> {
        self.colors.get(token).copied()
    }

    /// Copies every color from `other` over this theme, leaving `base` alone.
    pub fn overlay(&mut self, other: &Theme) {
        self.colors
            .extend(other.colors.iter().map(|(k, v)| (k.clone(), *v)));
    }
}

/// Failure to load or parse a theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeError {
    /// Malformed color literal.
    Hex(String),
    /// Malformed RON document.
    Ron(String),
    /// Unreadable theme file.
    Io(String),
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeError::Hex(hex) => write!(f, "invalid hex color `{hex}`"),
            ThemeError::Ron(err) => write!(f, "invalid theme ron: {err}"),
            ThemeError::Io(err) => write!(f, "could not read theme: {err}"),
        }
    }
}

impl std::error::Error for ThemeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_documented_hex_forms() {
        assert_eq!(
            Rgba::parse_hex("#E8743B").unwrap(),
            Rgba {
                r: 0xE8,
                g: 0x74,
                b: 0x3B,
                a: 255
            }
        );
        assert_eq!(
            Rgba::parse_hex("e8743b80").unwrap(),
            Rgba {
                r: 0xE8,
                g: 0x74,
                b: 0x3B,
                a: 0x80
            }
        );
        assert_eq!(
            Rgba::parse_hex("#f0a").unwrap(),
            Rgba {
                r: 0xFF,
                g: 0x00,
                b: 0xAA,
                a: 255
            }
        );
    }

    #[test]
    fn rejects_malformed_hex() {
        for bad in [
            "",
            "#",
            "#12",
            "#1234",
            "#1234567",
            "#123456789",
            "#ggg",
            "zzzzzz",
            "ééé",
        ] {
            assert!(Rgba::parse_hex(bad).is_err(), "{bad} parsed");
        }
    }

    #[test]
    fn hex_round_trips() {
        for hex in ["#000000", "#E8743B", "#E8743B80", "#FFFFFF"] {
            let color = Rgba::parse_hex(hex).unwrap();
            assert_eq!(color.to_hex(), hex);
            assert_eq!(Rgba::parse_hex(&color.to_hex()).unwrap(), color);
        }
    }

    #[test]
    fn loads_ron_and_reads_tokens() {
        let theme = Theme::load_from_str(
            r##"(
                base: "dark",
                colors: {
                    "surface.pane_body": "#141414",
                    "fill.accent": "#E8743B80",
                },
            )"##,
        )
        .unwrap();

        assert_eq!(theme.base, "dark");
        assert_eq!(
            theme.get("surface.pane_body"),
            Some(Rgba {
                r: 0x14,
                g: 0x14,
                b: 0x14,
                a: 255
            })
        );
        assert_eq!(theme.get("fill.accent").unwrap().a, 0x80);
        assert_eq!(theme.get("missing"), None);
    }

    #[test]
    fn overlay_prefers_other_colors_and_keeps_base() {
        let mut theme = Theme::load_from_str(
            r##"(base: "dark", colors: { "text.default": "#FFFFFF", "fill.field": "#111111" })"##,
        )
        .unwrap();
        let other = Theme::load_from_str(
            r##"(base: "light", colors: { "text.default": "#000000", "focus.ring": "#E8743B" })"##,
        )
        .unwrap();

        theme.overlay(&other);

        assert_eq!(theme.base, "dark");
        assert_eq!(theme.get("text.default"), Rgba::parse_hex("#000000").ok());
        assert_eq!(theme.get("focus.ring"), Rgba::parse_hex("#E8743B").ok());
        assert_eq!(theme.get("fill.field"), Rgba::parse_hex("#111111").ok());
    }

    #[test]
    fn bad_ron_is_reported() {
        let err = Theme::load_from_str("not ron").unwrap_err();
        assert!(matches!(err, ThemeError::Ron(_)));
        assert!(!err.to_string().is_empty());
    }
}
