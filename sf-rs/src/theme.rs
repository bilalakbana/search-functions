//! Catppuccin Macchiato, as in Ghostty, tmux, Obsidian and the fzf prototype. Backgrounds stay the
//! terminal's own (`Color::Reset`) except where a block is drawn on purpose.

use ratatui::style::Color;

pub const TEXT: Color = Color::Rgb(0xca, 0xd3, 0xf5);
/// subtext0
pub const SUB: Color = Color::Rgb(0xa5, 0xad, 0xcb);
/// overlay1
pub const DIM: Color = Color::Rgb(0x80, 0x87, 0xa2);
/// overlay0
pub const FAINT: Color = Color::Rgb(0x6e, 0x73, 0x8d);
/// surface0: the selected row
pub const SURFACE: Color = Color::Rgb(0x36, 0x3a, 0x4f);
/// base: text on a coloured block
pub const BASE: Color = Color::Rgb(0x24, 0x27, 0x3a);
pub const MAUVE: Color = Color::Rgb(0xc6, 0xa0, 0xf6);
pub const BLUE: Color = Color::Rgb(0x8a, 0xad, 0xf4);
pub const GREEN: Color = Color::Rgb(0xa6, 0xda, 0x95);
pub const RED: Color = Color::Rgb(0xed, 0x87, 0x96);
