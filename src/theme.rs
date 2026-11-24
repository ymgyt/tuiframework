//! Palettes and widget styles shared across screens.

use ratatui::style::{Color, Modifier, Style};

/// Colors from which an application can derive additional widget styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub name: &'static str,
    pub background: Color,
    pub foreground: Color,
    pub muted: Color,
    pub accent: Color,
    pub error: Color,
}

impl Palette {
    pub const fn dracula() -> Self {
        Self {
            name: "dracula",
            background: Color::Rgb(0x28, 0x2a, 0x36),
            foreground: Color::Rgb(0xf8, 0xf8, 0xf2),
            muted: Color::Rgb(0x62, 0x72, 0xa4),
            accent: Color::Rgb(0xff, 0x79, 0xc6),
            error: Color::Rgb(0xff, 0x55, 0x55),
        }
    }

    pub const fn eldritch() -> Self {
        Self {
            name: "eldritch",
            background: Color::Rgb(0x21, 0x23, 0x37),
            foreground: Color::Rgb(0xeb, 0xfa, 0xfa),
            muted: Color::Rgb(0x70, 0x81, 0xd0),
            accent: Color::Rgb(0x37, 0xf4, 0x99),
            error: Color::Rgb(0xf1, 0x6c, 0x75),
        }
    }

    pub const fn helix() -> Self {
        Self {
            name: "helix",
            background: Color::Rgb(0x3b, 0x22, 0x4c),
            foreground: Color::Rgb(0xa4, 0xa0, 0xe8),
            muted: Color::Rgb(0x69, 0x7c, 0x81),
            accent: Color::Rgb(0xff, 0xff, 0xff),
            error: Color::Rgb(0xf4, 0x78, 0x68),
        }
    }

    pub const fn ferra() -> Self {
        Self {
            name: "ferra",
            background: Color::Rgb(0x2b, 0x29, 0x2d),
            foreground: Color::Rgb(0xfe, 0xcd, 0xb2),
            muted: Color::Rgb(0x6F, 0x5D, 0x63),
            accent: Color::Rgb(0xff, 0xa0, 0x7a),
            error: Color::Rgb(0xe0, 0x6b, 0x75),
        }
    }

    pub const fn solarized_dark() -> Self {
        Self {
            name: "solarized_dark",
            background: Color::Rgb(0x00, 0x2b, 0x36),
            foreground: Color::Rgb(0x93, 0xa1, 0xa1),
            muted: Color::Rgb(0x58, 0x6e, 0x75),
            accent: Color::Rgb(0x26, 0x8b, 0xd2),
            error: Color::Rgb(0xdc, 0x32, 0x2f),
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::ferra()
    }
}

/// Common styles for a screen, its labels, and its selected item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub palette: Palette,
    pub base: Style,
    pub title: Style,
    pub header: Style,
    pub selected: Style,
    pub muted: Style,
    pub error: Style,
}

impl From<Palette> for Theme {
    fn from(palette: Palette) -> Self {
        let base = Style::new().fg(palette.foreground).bg(palette.background);
        Self {
            palette,
            base,
            title: base.add_modifier(Modifier::BOLD),
            header: Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            selected: Style::new().fg(palette.accent).add_modifier(Modifier::BOLD),
            muted: base.fg(palette.muted),
            error: base.fg(palette.error),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Palette::default().into()
    }
}
