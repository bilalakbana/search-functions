//! The palette: 256-colour indexes, by default those of Bilal's fzf screen (`find_files`'s `f()`), each
//! one overridable in the `[theme]` table of the config file. `init` sets it once at the start; the
//! drawing reads it through one function per role. Tests never call `init`: they see the defaults.

use std::sync::OnceLock;

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

/// The struct, its defaults and one accessor per role, from one table.
macro_rules! palette {
    ($($(#[doc = $doc:literal])* $role:ident = $default:literal,)*) => {
        /// `[theme]` in the config file: each role's 256-colour index.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Theme {
            $($(#[doc = $doc])* pub $role: u8,)*
        }

        impl Default for Theme {
            fn default() -> Self {
                Self { $($role: $default,)* }
            }
        }

        $($(#[doc = $doc])* pub fn $role() -> Color {
            Color::Indexed(current().$role)
        })*
    };
}

palette! {
    /// body text, list text, the typed query
    fg = 252,
    /// the selected row's text
    fg_sel = 252,
    /// the screen's background
    bg = 234,
    /// the selected row's background
    bg_sel = 235,
    /// matched words: the filter's, Ctrl-F's, the preview's
    hl = 5,
    /// matched words in the selected row
    hl_sel = 5,
    /// the counter, key hints, secondary labels, dim explanations
    info = 144,
    /// every border, the top border's fill, faint text
    border = 240,
    /// the `>` sign, the selected tab, titles, the accent
    prompt = 161,
    /// the `▌` before the selected row
    pointer = 161,
    /// a success said in the status
    marker = 118,
    /// the loading `…`
    spinner = 118,
    /// section headers (Help, the popup) and the preview's found title
    header = 168,
    /// the folders in a path
    path_dir = 4,
    /// every `/` in a path
    path_slash = 3,
    /// a path's last part
    path_name = 2,
    /// sf's own errors
    error = 1,
}

impl Theme {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

static THEME: OnceLock<Theme> = OnceLock::new();

/// Sets the palette for the whole run; only the first call counts.
pub fn init(theme: Theme) {
    let _ = THEME.set(theme);
}

fn current() -> &'static Theme {
    THEME.get_or_init(Theme::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_fzf_screens() {
        let t = Theme::default();
        let got = [
            t.fg,
            t.fg_sel,
            t.bg,
            t.bg_sel,
            t.hl,
            t.hl_sel,
            t.info,
            t.border,
            t.prompt,
            t.pointer,
            t.marker,
            t.spinner,
            t.header,
            t.path_dir,
            t.path_slash,
            t.path_name,
            t.error,
        ];
        let want = [
            252, 252, 234, 235, 5, 5, 144, 240, 161, 161, 118, 118, 168, 4, 3, 2, 1,
        ];
        assert_eq!(got, want);
        assert_eq!(border(), Color::Indexed(240), "the accessors read them");
        assert!(t.is_default());
    }
}
