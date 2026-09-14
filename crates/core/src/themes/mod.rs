//! Themes — a reusable look (colours, corner radius, typography, density) that
//! forms opt into by `theme_id`. CRUD + DTOs.
//!
//! A theme is a set of *optional* overrides, never a full palette: an unset
//! colour falls through to the renderer's built-in value (light or dark, per
//! the embed's `data-theme`). That keeps `data-theme` meaningful for whatever a
//! theme leaves alone, and means a token added later needs no migration.
//!
//! The renderer applies these as `--or-theme-*` custom properties, a tier
//! *below* the public `--or-color-*` tokens a host page sets — so a host page
//! that themes its embed keeps winning. See `packages/form-renderer/src/theme.ts`.
//!
//! Every value lands in CSS on a page we don't own, so `service::validate_settings`
//! is a security boundary in the way `validate_redirect_url` is: colours are hex
//! only and a font family is a fixed character set, which rules out `url()`,
//! `var()` and declaration break-out. The renderer re-checks the same rules.

pub mod service;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Colour overrides. Every member is a `#rgb`, `#rgba`, `#rrggbb` or
/// `#rrggbbaa` hex string; `None` keeps the renderer's built-in value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ThemeColors {
    /// Form surface. The built-in value is transparent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// Body text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Help and secondary text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<String>,
    /// Form and divider border.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_border: Option<String>,
    /// Buttons, radio accents and the progress fill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    /// Text drawn on an accent background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_text: Option<String>,
    /// Errors and required marks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Links inside rich-text blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// Star-rating outline and fill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
}

impl ThemeColors {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Every slot, mutably, paired with its wire name — so validation can
    /// name the offending key without a hand-written match per colour.
    pub(crate) fn slots_mut(&mut self) -> [(&'static str, &mut Option<String>); 11] {
        [
            ("background", &mut self.background),
            ("text", &mut self.text),
            ("muted", &mut self.muted),
            ("border", &mut self.border),
            ("input_background", &mut self.input_background),
            ("input_border", &mut self.input_border),
            ("accent", &mut self.accent),
            ("accent_text", &mut self.accent_text),
            ("error", &mut self.error),
            ("link", &mut self.link),
            ("rating", &mut self.rating),
        ]
    }
}

/// Base type scale. Multiplies every font size the renderer draws.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FontSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl FontSize {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Spacing scale. Multiplies the gaps between fields and the padding inside
/// the form, its inputs and its buttons.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    #[default]
    Comfortable,
    Spacious,
}

impl Density {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// What a theme controls. Every member is optional or defaulted, and defaults
/// are skipped on serialisation — so the empty theme is `{}` and the admin's
/// `JSON.stringify` comparisons converge with what the server sends back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ThemeSettings {
    #[serde(default, skip_serializing_if = "ThemeColors::is_empty")]
    pub colors: ThemeColors,
    /// Corner radius for the form and its controls, in pixels (0–32). `None`
    /// keeps the built-in 8px.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<u8>,
    /// A CSS font-family list, e.g. `Inter, sans-serif`. Only names the host
    /// page already loads will render; `None` keeps the built-in system stack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "FontSize::is_default")]
    pub font_size: FontSize,
    #[serde(default, skip_serializing_if = "Density::is_default")]
    pub density: Density,
}

/// Outbound representation of a theme.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ThemeDto {
    pub id: i32,
    pub name: String,
    /// Whether forms with no `theme_id` render with this theme.
    pub is_default: bool,
    pub settings: ThemeSettings,
    /// How many forms name this theme explicitly. Forms that use it only
    /// because it is the default are not counted.
    pub form_count: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct NewTheme {
    pub name: String,
    /// Make this the default theme, unsetting any previous default.
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub settings: ThemeSettings,
}

/// Partial update. `None` means "leave the field alone"; `settings` replaces
/// the whole settings object. `is_default: true` moves the default here;
/// `false` on the current default leaves the workspace with no default theme.
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
pub struct UpdateTheme {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<ThemeSettings>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ThemeList {
    pub items: Vec<ThemeDto>,
    pub total: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_theme_serialises_as_an_empty_object() {
        let json = serde_json::to_value(ThemeSettings::default()).unwrap();
        assert_eq!(json, serde_json::json!({}));
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let err = serde_json::from_value::<ThemeSettings>(serde_json::json!({ "radious": 4 }));
        assert!(err.is_err());
        let err = serde_json::from_value::<ThemeSettings>(
            serde_json::json!({ "colors": { "acent": "#fff" } }),
        );
        assert!(err.is_err());
    }

    #[test]
    fn round_trips_a_full_theme() {
        let json = serde_json::json!({
            "colors": { "accent": "#4f46e5", "text": "#111" },
            "radius": 12,
            "font_family": "Inter, sans-serif",
            "font_size": "large",
            "density": "compact",
        });
        let parsed: ThemeSettings = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), json);
    }
}
