//! An asset source for the app's bundled SVGs. GPUI resolves `svg()` paths
//! through this, so icons live next to the binary rather than on disk.
//!
//! There is one global asset source, so ours must also answer for the kit's
//! icons: gpui-component draws its dropdown carets, checkbox ticks and other
//! `IconName`s from `gpui_kit::assets`. Without the fallback those render as
//! nothing at all.

use std::borrow::Cow;

use gpui_kit::assets::Assets as KitAssets;
use gpui_kit::{AssetSource, Result, SharedString};

const ICONS: &[(&str, &[u8])] = &[
    (
        "icons/arrow-right-left.svg",
        include_bytes!("../../../assets/icons/arrow-right-left.svg"),
    ),
    (
        "icons/settings.svg",
        include_bytes!("../../../assets/icons/settings.svg"),
    ),
    (
        "icons/eraser.svg",
        include_bytes!("../../../assets/icons/eraser.svg"),
    ),
    (
        "icons/triangle-alert.svg",
        include_bytes!("../../../assets/icons/triangle-alert.svg"),
    ),
    (
        "icons/copy.svg",
        include_bytes!("../../../assets/icons/copy.svg"),
    ),
    (
        "icons/check.svg",
        include_bytes!("../../../assets/icons/check.svg"),
    ),
    (
        "icons/pencil.svg",
        include_bytes!("../../../assets/icons/pencil.svg"),
    ),
    (
        "icons/trash.svg",
        include_bytes!("../../../assets/icons/trash.svg"),
    ),
    (
        "icons/plus.svg",
        include_bytes!("../../../assets/icons/plus.svg"),
    ),
    (
        "icons/sliders-horizontal.svg",
        include_bytes!("../../../assets/icons/sliders-horizontal.svg"),
    ),
    (
        "icons/refresh-cw.svg",
        include_bytes!("../../../assets/icons/refresh-cw.svg"),
    ),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = ICONS.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(*bytes)));
        }
        KitAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut names: Vec<SharedString> = ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect();
        names.extend(KitAssets.list(path)?);
        Ok(names)
    }
}
