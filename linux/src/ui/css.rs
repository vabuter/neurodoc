//! Оформление: токены Windows-версии (App.xaml) и палитра Android (Theme.kt),
//! переложенные на CSS GTK. Акцент — teal из иконки приложения, как на Android.
//! Именованные цвета через @define-color: они есть и в libadwaita 1.5 (Ubuntu 24.04),
//! а CSS-переменные появились только в 1.6.

use gtk::gdk;

/// Акцент приложения: teal из иконки (Android: Teal / TealLight).
pub const ACCENT_DARK: u32 = 0x4DD6C7;
pub const ACCENT_LIGHT: u32 = 0x12A094;
/// Веб-ссылка — синяя, как в браузере (MainWindow.xaml.cs: GetWebLinkColor).
pub const WEB_DARK: u32 = 0x4DA3FF;
pub const WEB_LIGHT: u32 = 0x0B62C4;
/// Уже прочитанная ссылка — намеренно нейтральный серый, а не акцент.
pub const VISITED_DARK: u32 = 0x86868E;
pub const VISITED_LIGHT: u32 = 0x767680;
pub const EDITOR_BG_DARK: u32 = 0x1A1A1A;
pub const EDITOR_BG_LIGHT: u32 = 0xFFFFFF;

pub fn rgba(hex: u32, alpha: f32) -> gdk::RGBA {
    gdk::RGBA::new(
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
        alpha,
    )
}

/// Приглушённый вариант цвета — непрозрачный, в отличие от снижения альфы.
pub fn dim(hex: u32, factor: f32) -> gdk::RGBA {
    let c = rgba(hex, 1.0);
    gdk::RGBA::new(c.red() * factor, c.green() * factor, c.blue() * factor, 1.0)
}

pub fn accent(dark: bool) -> u32 {
    if dark { ACCENT_DARK } else { ACCENT_LIGHT }
}

/// CSS приложения для тёмной или светлой темы.
pub fn stylesheet(dark: bool) -> String {
    let (window_bg, editor_bg, pane_bg, border, dim_text, accent, accent_fg, kbd_bg) = if dark {
        ("#111111", "#1A1A1A", "rgba(255,255,255,0.043)", "rgba(255,255,255,0.078)", "rgba(255,255,255,0.55)", "#4DD6C7", "#00201C", "rgba(255,255,255,0.06)")
    } else {
        ("#F3F3F3", "#FFFFFF", "rgba(255,255,255,0.6)", "rgba(0,0,0,0.09)", "rgba(0,0,0,0.55)", "#12A094", "#FFFFFF", "rgba(0,0,0,0.04)")
    };
    format!(
        r#"
@define-color window_bg_color {window_bg};
@define-color window_fg_color {fg};
@define-color headerbar_bg_color {window_bg};
@define-color headerbar_fg_color {fg};
@define-color view_bg_color {editor_bg};
@define-color accent_bg_color {accent};
@define-color accent_fg_color {accent_fg};
@define-color accent_color {accent};
@define-color dialog_bg_color {dialog_bg};
@define-color popover_bg_color {dialog_bg};
@define-color card_bg_color {pane_bg};

.nd-editor-card {{ background-color: {editor_bg}; border: 1px solid {border}; border-radius: 8px; }}
.nd-pane-card {{ background-color: {pane_bg}; border: 1px solid {border}; border-radius: 8px; padding: 12px 8px 10px 14px; }}
.nd-card-title {{ font-size: 13px; font-weight: 600; }}
.nd-dim {{ color: {dim_text}; }}
.nd-mono {{ font-family: monospace; }}
.nd-size-10 {{ font-size: 10px; }}
.nd-size-11 {{ font-size: 11px; }}
.nd-size-12 {{ font-size: 12px; }}
.nd-size-13 {{ font-size: 13px; }}
.nd-size-16 {{ font-size: 16px; }}
.nd-broken-target {{ color: #FF6B6B; font-family: monospace; font-size: 10px; }}

.nd-tool {{ padding: 7px 9px; border-radius: 6px; min-height: 0; min-width: 0; }}
.nd-tool label {{ font-size: 13px; }}
.nd-separator {{ background-color: {border}; min-width: 1px; margin: 0 4px; }}
.nd-format-toggle {{ font-size: 11px; padding: 3px 9px; border-radius: 4px; min-height: 0; min-width: 0; }}
.nd-format-toggle:checked {{ background-color: {accent}; color: {accent_fg}; }}
.nd-small-button {{ font-size: 12px; padding: 4px 10px; border-radius: 4px; min-height: 0; }}
.nd-small-button:checked {{ background-color: {accent}; color: {accent_fg}; }}
.nd-crumb {{ font-size: 12px; padding: 2px 5px; min-height: 0; color: {accent}; border-radius: 4px; }}
.nd-crumb-last {{ font-size: 12px; padding: 2px 5px; color: {dim_text}; }}
.nd-chevron {{ color: {dim_text}; margin: 0 2px; }}

.nd-editor, .nd-editor text {{ background-color: transparent; color: {fg}; caret-color: {fg}; }}
.nd-findbar {{ background-color: {editor_bg}; border: 1px solid {border}; border-radius: 6px; padding: 5px 8px; }}
.nd-list {{ background-color: transparent; }}
.nd-list row {{ padding: 7px 4px; border-radius: 4px; }}
.nd-list row:hover {{ background-color: {kbd_bg}; }}
.nd-kbd {{ background-color: {kbd_bg}; border: 1px solid {border}; border-radius: 4px; padding: 3px 8px; font-family: "JetBrains Mono", monospace; font-size: 12px; }}
.nd-progress progressbar, .nd-progress trough, .nd-progress progress {{ min-height: 3px; }}
.nd-badge {{ background-color: {accent}; color: {accent_fg}; border-radius: 9px; padding: 3px 9px; font-size: 11px; }}
.nd-tip-title {{ font-weight: 600; }}
.nd-error {{ color: #FF6B6B; }}
.nd-success {{ color: {accent}; }}
"#,
        fg = if dark { "#E8E8E8" } else { "#1B1B1B" },
        dialog_bg = if dark { "#1E1E1E" } else { "#FAFAFA" },
    )
}
