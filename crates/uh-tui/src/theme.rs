//! The extension's colours (`vscode/webview/styles.css`, the dark theme): a
//! neutral grey page, surfaces a step lighter or deeper, near-white ink at a
//! few strengths, and the cyan as the only colour.

use ratatui::style::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// `--uh-bg`: the page.
pub const BG: Rgb = Rgb(0x1a, 0x1a, 0x1b);
/// `--uh-raised`: what floats on the page (the message box, the kernel's panel).
pub const RAISED: Rgb = Rgb(0x22, 0x22, 0x23);
/// `--uh-raised-2`: the user's message.
pub const RAISED_2: Rgb = Rgb(0x2a, 0x2a, 0x2c);
/// `--uh-text`.
pub const INK: Rgb = Rgb(236, 236, 238);
/// `--uh-accent`: the live state, the logo.
pub const ACCENT: Rgb = Rgb(134, 222, 252);
/// `--uh-danger`.
pub const DANGER: Rgb = Rgb(255, 163, 163);
pub const WHITE: Rgb = Rgb(255, 255, 255);
/// Python's own blue, for the mark beside the cells it runs.
pub const PYTHON_BLUE: Rgb = Rgb(75, 139, 190);
const BLACK: Rgb = Rgb(0, 0, 0);

/// `--uh-muted` and `--uh-faint`: the ink's strength over what it sits on.
pub const MUTED: f32 = 0.56;
pub const FAINT: f32 = 0.34;

impl Rgb {
    /// `over` laid on this colour at `alpha`.
    #[must_use]
    pub fn mix(self, over: Rgb, alpha: f32) -> Rgb {
        let alpha = alpha.clamp(0.0, 1.0);
        let channel = |under: u8, over: u8| {
            (f32::from(under) + (f32::from(over) - f32::from(under)) * alpha).round() as u8
        };
        Rgb(
            channel(self.0, over.0),
            channel(self.1, over.1),
            channel(self.2, over.2),
        )
    }
}

/// `--uh-code`: a code block's face.
#[must_use]
pub fn code() -> Rgb {
    BG.mix(INK, 0.0225)
}

/// `--uh-code-head`: its band on top.
#[must_use]
pub fn code_head() -> Rgb {
    BG.mix(INK, 0.07)
}

/// `--uh-sunk`: what a cell printed, a tone deeper than the page.
#[must_use]
pub fn sunk() -> Rgb {
    BG.mix(BLACK, 0.22)
}

/// How many colours the terminal shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    /// 24 bits: the colours as they are.
    True,
    /// The 256-colour palette: the nearest of its cube and its greys.
    Indexed,
}

impl Depth {
    /// What the terminal says of itself (`COLORTERM`).
    #[must_use]
    pub fn detect() -> Depth {
        match std::env::var("COLORTERM").as_deref() {
            Ok("truecolor" | "24bit") => Depth::True,
            _ => Depth::Indexed,
        }
    }

    #[must_use]
    pub fn color(self, rgb: Rgb) -> Color {
        match self {
            Depth::True => Color::Rgb(rgb.0, rgb.1, rgb.2),
            Depth::Indexed => Color::Indexed(indexed(rgb)),
        }
    }
}

/// The 6 levels of the palette's colour cube.
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The palette entry nearest to `rgb`: of the cube (16..=231) or the grey ramp (232..=255).
fn indexed(rgb: Rgb) -> u8 {
    let level = |value: u8| {
        (0..6u8)
            .min_by_key(|&at| (i32::from(CUBE[usize::from(at)]) - i32::from(value)).abs())
            .unwrap_or(0)
    };
    let (r, g, b) = (level(rgb.0), level(rgb.1), level(rgb.2));
    let cube = Rgb(
        CUBE[usize::from(r)],
        CUBE[usize::from(g)],
        CUBE[usize::from(b)],
    );
    let mean = (u16::from(rgb.0) + u16::from(rgb.1) + u16::from(rgb.2)) / 3;
    let step = ((mean.saturating_sub(8) + 5) / 10).min(23) as u8;
    let grey = 8 + 10 * step;
    let far = |of: Rgb| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(of.0, rgb.0) + d(of.1, rgb.1) + d(of.2, rgb.2)
    };
    if far(Rgb(grey, grey, grey)) < far(cube) {
        232 + step
    } else {
        16 + 36 * r + 6 * g + b
    }
}
