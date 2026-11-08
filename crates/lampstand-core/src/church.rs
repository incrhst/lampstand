//! The settings that follow a church across all of its devices: its look
//! (name, logo, colours, font, logo bug), which theme is the brand theme
//! and the default theme, and the default idle screen.
//!
//! This is the shape of the synced "church settings" document. Every
//! field is a scalar of its own at the root of the document, so two devices
//! that edit different fields merge, and two that edit the same field keep
//! the later edit. Nothing here is nested, so two devices that each create
//! the document before they meet still merge instead of one hiding the
//! other. The brand, the logo bug and the idle screen are read and written
//! through [`ChurchSettings::brand`], [`ChurchSettings::set_brand`] and
//! the like, and the app's own types (`Brand`, `IdleScreen`) stay what the
//! rest of Lampstand sees.
//!
//! What describes one machine (screens, safe areas, OBS, the clicker) is
//! not here; see `lampstand_api::DeviceSettings`.

use serde::{Deserialize, Serialize};

use crate::brand::{Brand, BugCorner, BugSize, LogoBug};
use crate::geom::Color;
use crate::id::BlobHash;
use crate::output::IdleScreen;
use crate::theme::ThemeId;

/// The one church-settings document's UUID, the same on every device.
pub const CHURCH_SETTINGS_ID: uuid::Uuid =
    uuid::Uuid::from_u128(0x0192_0000_0000_7000_8000_6c61_6d70_6368);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "automerge", derive(autosurgeon::Hydrate))]
pub struct ChurchSettings {
    /// A brand exists once it has a main colour; the other brand fields
    /// are only read when it does.
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub primary: Option<u32>,
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub name: Option<String>,
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub logo: Option<BlobHash>,
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub text: Option<u32>,
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub accent: Option<u32>,
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub font_family: Option<String>,
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub bug_enabled: Option<bool>,
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub bug_corner: Option<String>,
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub bug_size: Option<String>,
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub bug_opacity: Option<u32>,
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub brand_theme: Option<ThemeId>,
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub default_theme: Option<ThemeId>,
    /// The idle screen as JSON, one value: two devices that change it at
    /// once keep one whole screen, not a picture from one and a name from
    /// the other.
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub idle: Option<String>,
    /// The name of the device that last changed any of this, so another
    /// device can say where a file it is waiting for comes from.
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub updated_by: Option<String>,
}

/// Written by hand: a field that is not set is removed from the document
/// only if it is there, never written as a null. A device that has not
/// touched a field therefore cannot overwrite what another device set
/// at the same time, which a null would.
#[cfg(feature = "automerge")]
impl autosurgeon::Reconcile for ChurchSettings {
    type Key<'a> = autosurgeon::reconcile::NoKey;

    fn reconcile<R: autosurgeon::Reconciler>(&self, mut reconciler: R) -> Result<(), R::Error> {
        use autosurgeon::reconcile::MapReconciler;
        fn field<M: MapReconciler, T: autosurgeon::Reconcile>(
            m: &mut M,
            key: &str,
            value: &Option<T>,
        ) -> Result<(), M::Error> {
            match value {
                Some(v) => m.put(key, v),
                None if m.entry(key).is_some() => m.delete(key),
                None => Ok(()),
            }
        }
        let mut m = reconciler.map()?;
        field(&mut m, "primary", &self.primary)?;
        field(&mut m, "name", &self.name)?;
        field(&mut m, "logo", &self.logo)?;
        field(&mut m, "text", &self.text)?;
        field(&mut m, "accent", &self.accent)?;
        field(&mut m, "font_family", &self.font_family)?;
        field(&mut m, "bug_enabled", &self.bug_enabled)?;
        field(&mut m, "bug_corner", &self.bug_corner)?;
        field(&mut m, "bug_size", &self.bug_size)?;
        field(&mut m, "bug_opacity", &self.bug_opacity)?;
        field(&mut m, "brand_theme", &self.brand_theme)?;
        field(&mut m, "default_theme", &self.default_theme)?;
        field(&mut m, "idle", &self.idle)?;
        field(&mut m, "updated_by", &self.updated_by)?;
        Ok(())
    }
}

fn pack(c: Color) -> u32 {
    u32::from_be_bytes([c.r, c.g, c.b, c.a])
}

fn unpack(v: u32) -> Color {
    let [r, g, b, a] = v.to_be_bytes();
    Color { r, g, b, a }
}

impl ChurchSettings {
    /// Nothing set: the document is new, or only holds who touched it.
    pub fn is_empty(&self) -> bool {
        self.primary.is_none()
            && self.brand_theme.is_none()
            && self.default_theme.is_none()
            && self.idle.is_none()
    }

    pub fn brand(&self) -> Option<Brand> {
        let primary = unpack(self.primary?);
        let mut bug = LogoBug::default();
        if let Some(on) = self.bug_enabled {
            bug.enabled = on;
        }
        match self.bug_corner.as_deref() {
            Some("LowerLeft") => bug.corner = BugCorner::LowerLeft,
            Some("LowerRight") => bug.corner = BugCorner::LowerRight,
            _ => {}
        }
        match self.bug_size.as_deref() {
            Some("Small") => bug.size = BugSize::Small,
            Some("Medium") => bug.size = BugSize::Medium,
            Some("Large") => bug.size = BugSize::Large,
            _ => {}
        }
        if let Some(o) = self.bug_opacity {
            bug.opacity = o.clamp(10, 100) as u8;
        }
        Some(Brand {
            name: self.name.clone().unwrap_or_default(),
            logo: self.logo,
            primary,
            text: self.text.map(unpack),
            accent: self.accent.map(unpack).unwrap_or(primary),
            font_family: self.font_family.clone().unwrap_or_default(),
            bug,
        })
    }

    /// Sets every brand field, or clears them all for `None`.
    pub fn set_brand(&mut self, brand: Option<&Brand>) {
        let Some(b) = brand else {
            self.primary = None;
            self.name = None;
            self.logo = None;
            self.text = None;
            self.accent = None;
            self.font_family = None;
            self.bug_enabled = None;
            self.bug_corner = None;
            self.bug_size = None;
            self.bug_opacity = None;
            return;
        };
        self.primary = Some(pack(b.primary));
        self.name = Some(b.name.clone());
        self.logo = b.logo;
        self.text = b.text.map(pack);
        self.accent = Some(pack(b.accent));
        self.font_family = Some(b.font_family.clone());
        self.bug_enabled = Some(b.bug.enabled);
        self.bug_corner = Some(format!("{:?}", b.bug.corner));
        self.bug_size = Some(format!("{:?}", b.bug.size));
        self.bug_opacity = Some(u32::from(b.bug.opacity));
    }

    pub fn idle(&self) -> Option<IdleScreen> {
        serde_json::from_str(self.idle.as_deref()?).ok()
    }

    pub fn set_idle(&mut self, idle: Option<&IdleScreen>) {
        self.idle = idle.and_then(|i| serde_json::to_string(i).ok());
    }

    /// Every file the settings point at, with what each is, for fetching
    /// from another device and for saying what is being waited for.
    pub fn files(&self) -> Vec<(BlobHash, &'static str)> {
        let mut files = Vec::new();
        if let Some(logo) = self.logo {
            files.push((logo, "logo"));
        }
        match self.idle() {
            Some(IdleScreen::Image { blob, .. }) => files.push((blob, "idle picture")),
            Some(IdleScreen::Video { blob, .. }) => files.push((blob, "idle video")),
            _ => {}
        }
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brand() -> Brand {
        Brand {
            name: "Grace Chapel".into(),
            logo: Some(BlobHash::of(b"logo")),
            primary: Color::rgb(10, 20, 30),
            text: None,
            accent: Color::rgb(200, 100, 50),
            font_family: "Lato".into(),
            bug: LogoBug {
                enabled: true,
                corner: BugCorner::LowerLeft,
                size: BugSize::Large,
                opacity: 55,
            },
        }
    }

    #[test]
    fn a_brand_round_trips_through_the_fields() {
        let mut s = ChurchSettings::default();
        assert!(s.brand().is_none() && s.is_empty());
        s.set_brand(Some(&brand()));
        assert_eq!(s.brand(), Some(brand()));
        s.set_brand(None);
        assert_eq!(s, ChurchSettings::default());
    }

    #[test]
    fn an_idle_screen_round_trips() {
        let idle = IdleScreen::Video {
            blob: BlobHash::of(b"loop"),
            name: "Loop.mp4".into(),
        };
        let mut s = ChurchSettings::default();
        s.set_idle(Some(&idle));
        assert_eq!(s.idle(), Some(idle));
        assert_eq!(s.files(), vec![(BlobHash::of(b"loop"), "idle video")]);
        s.set_idle(None);
        assert_eq!(s.idle(), None);
    }
}
