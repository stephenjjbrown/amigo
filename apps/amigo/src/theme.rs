//! Design tokens, carried over from the egui app so the rewrite looks the same.

use std::borrow::Cow;

use gpui_kit::component::{Theme, ThemeColor, ThemeMode};
use gpui_kit::{px, rgb, rgba, App, Rgba, WindowAppearance};

/// The app's typeface, registered by `install` and named at every window root.
pub const FONT_FAMILY: &str = "Inter";

/// Height of every interactive control: buttons, inputs, pickers.
pub const CONTROL_HEIGHT: f32 = 32.0;

pub struct Radii {
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
}

pub const RADIUS: Radii = Radii {
    sm: 4.0,
    md: 6.0,
    lg: 8.0,
};

pub struct TypeScale {
    pub micro: f32,
    pub caption: f32,
    pub small: f32,
    pub body: f32,
    pub heading: f32,
    pub title: f32,
}

pub const TYPE: TypeScale = TypeScale {
    micro: 9.5,
    caption: 10.5,
    small: 11.5,
    body: 12.5,
    heading: 13.5,
    title: 15.5,
};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Rgba,
    pub surface: Rgba,
    pub surface_muted: Rgba,
    pub surface_strong: Rgba,
    pub border: Rgba,
    pub border_strong: Rgba,
    pub text: Rgba,
    pub text_secondary: Rgba,
    pub text_muted: Rgba,
    pub text_faint: Rgba,
    pub accent: Rgba,
    pub primary: Rgba,
    pub primary_contrast: Rgba,
    pub danger: Rgba,
    pub danger_soft: Rgba,
    pub danger_border: Rgba,
    pub success: Rgba,
    pub success_soft: Rgba,
    pub warning: Rgba,
}

impl Palette {
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            _ => Self::light(),
        }
    }

    fn dark() -> Self {
        Self {
            bg: rgb(0x131417),
            surface: rgb(0x1d1f24),
            surface_muted: rgb(0x191b20),
            surface_strong: rgb(0x26282e),
            border: rgba(0xffffff14),
            border_strong: rgba(0xffffff28),
            text: rgb(0xeef1f6),
            text_secondary: rgb(0xb6bdc9),
            text_muted: rgb(0x8d95a3),
            text_faint: rgb(0x6b7380),
            accent: rgb(0x4c9bff),
            primary: rgb(0x4ade80),
            primary_contrast: rgb(0x061a0f),
            danger: rgb(0xff8d82),
            danger_soft: rgba(0xb4231828),
            danger_border: rgba(0xff8d824c),
            success: rgb(0x4ade80),
            success_soft: rgba(0x4ade8022),
            warning: rgb(0xf0b45a),
        }
    }

    fn light() -> Self {
        Self {
            bg: rgb(0xedf0f5),
            surface: rgb(0xffffff),
            surface_muted: rgb(0xf6f7fa),
            surface_strong: rgb(0xecf0f4),
            border: rgba(0x0f172a17),
            border_strong: rgba(0x0f172a29),
            text: rgb(0x101828),
            text_secondary: rgb(0x475467),
            text_muted: rgb(0x667085),
            text_faint: rgb(0x98a2b3),
            accent: rgb(0x0f3d63),
            primary: rgb(0x067647),
            primary_contrast: rgb(0xffffff),
            danger: rgb(0xb42318),
            danger_soft: rgb(0xfef3f2),
            danger_border: rgba(0xb4231838),
            success: rgb(0x067647),
            success_soft: rgba(0x0676471a),
            warning: rgb(0xb54708),
        }
    }
}

/// Register the Interface font, as the egui app did, so type and metrics match.
pub fn install(cx: &mut App) {
    const INTER_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
    const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

    let _ = cx.text_system().add_fonts(vec![
        Cow::Borrowed(INTER_REGULAR),
        Cow::Borrowed(INTER_SEMIBOLD),
    ]);
}

/// Install gpui-component's theme from our palette, so the crate's controls and
/// our own surfaces share one set of colours. Heights come from `Size::Medium`
/// (32px), which is our control height, so nothing needs pinning.
pub fn install_component_theme(cx: &mut App) {
    gpui_kit::init(cx);

    let appearance = cx.window_appearance();

    // Everything the components read must go through `Theme::update`: it is what
    // reconciles the derived token set with the colours. Assigning `colors`
    // through `global_mut` leaves the tokens on the default theme, which shows up
    // as foreign palette colours and black text on our dark surfaces.
    Theme::update(cx, |theme| {
        theme.font_family = FONT_FAMILY.into();
        theme.font_size = px(16.0);
        theme.radius = px(RADIUS.sm);
        theme.radius_lg = px(RADIUS.lg);
        theme.shadow = true;
    });

    Theme::change(ThemeMode::from(appearance), None, cx);

    // Applied after the mode resolves, because `Theme::colors` is the active
    // palette while the per-mode `ThemeConfig` uses its own colour type.
    let palette = Palette::for_appearance(appearance);
    Theme::update(cx, |theme| {
        theme.colors = colors(&palette);
    });
}

/// Map our palette onto the crate's semantic tokens. Only the tokens our screens
/// use today are set; the rest keep the crate's defaults until a component needs
/// them.
///
/// `ThemeColor` is `#[non_exhaustive]`, so it cannot be built with a struct
/// literal from here — hence the field-by-field assignment.
#[allow(clippy::field_reassign_with_default)]
fn colors(palette: &Palette) -> ThemeColor {
    let mut colors = ThemeColor::default();

    colors.background = palette.bg.into();
    colors.foreground = palette.text.into();
    colors.border = palette.border.into();
    colors.muted = palette.surface_muted.into();
    colors.muted_foreground = palette.text_muted.into();
    colors.popover = palette.surface.into();
    colors.popover_foreground = palette.text.into();
    colors.overlay = rgba(0x00000080).into();
    colors.window_border = palette.border.into();
    colors.title_bar = palette.surface.into();
    colors.title_bar_border = palette.border.into();
    colors.sidebar = palette.surface_muted.into();
    colors.sidebar_border = palette.border.into();
    colors.sidebar_foreground = palette.text_secondary.into();
    colors.caret = palette.text.into();
    colors.selection = palette.surface_strong.into();
    colors.ring = palette.accent.into();
    colors.scrollbar_thumb = palette.border_strong.into();
    colors.scrollbar_thumb_hover = palette.text_faint.into();

    // Primary actions.
    colors.primary = palette.primary.into();
    colors.primary_foreground = palette.primary_contrast.into();
    colors.primary_hover = palette.primary.into();
    colors.primary_active = palette.primary.into();
    colors.button_primary = palette.primary.into();
    colors.button_primary_hover = palette.primary.into();
    colors.button_primary_active = palette.primary.into();
    colors.button_primary_foreground = palette.primary_contrast.into();

    // Secondary buttons, which is what most controls in the app are.
    colors.button = palette.surface_muted.into();
    colors.button_hover = palette.surface_strong.into();
    colors.button_active = palette.surface_strong.into();
    colors.button_foreground = palette.text_secondary.into();
    colors.secondary = palette.surface_muted.into();
    colors.secondary_hover = palette.surface_strong.into();
    colors.secondary_active = palette.surface_strong.into();
    colors.secondary_foreground = palette.text_secondary.into();

    // Destructive actions.
    colors.button_danger = palette.danger_soft.into();
    colors.button_danger_hover = palette.danger_border.into();
    colors.button_danger_active = palette.danger_border.into();
    colors.button_danger_foreground = palette.danger.into();
    colors.danger = palette.danger.into();
    colors.danger_foreground = palette.primary_contrast.into();

    // Inputs and lists.
    colors.input = palette.surface_muted.into();
    colors.list = palette.surface.into();
    colors.list_hover = palette.surface_strong.into();
    colors.list_active = palette.surface_strong.into();
    colors.list_active_border = palette.border_strong.into();
    colors.list_even = palette.surface.into();

    colors.accent = palette.surface_strong.into();
    colors.accent_foreground = palette.text.into();
    colors.link = palette.accent.into();
    colors.switch = palette.primary.into();
    colors.switch_thumb = palette.primary_contrast.into();
    colors.slider_bar = palette.surface_strong.into();
    colors.slider_thumb = palette.primary.into();
    colors.progress_bar = palette.primary.into();
    colors.warning = palette.warning.into();
    colors.success = palette.primary.into();

    colors
}
