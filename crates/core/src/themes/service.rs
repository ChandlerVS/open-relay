//! Theme persistence + validation.
//!
//! All functions take `&impl ConnectionTrait` so callers can pass either a
//! `DatabaseConnection` or a `DatabaseTransaction`. Writes that move the
//! default (`create`/`update` with `is_default: true`) touch two rows and
//! should run inside a transaction.

use std::collections::HashMap;

use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect,
};

use super::{NewTheme, ThemeDto, ThemeList, ThemeSettings, UpdateTheme};
use crate::error::{CoreError, CoreResult};

const MAX_NAME_LEN: usize = 200;
/// Largest corner radius a theme may set, in pixels. Mirrored by
/// `MAX_RADIUS` in `packages/form-renderer/src/theme.ts`.
pub const MAX_RADIUS: u8 = 32;
const MAX_FONT_FAMILY_LEN: usize = 200;

fn validate_name(name: &str) -> CoreResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_NAME_LEN {
        return Err(CoreError::BadRequest(format!(
            "name must be 1..={MAX_NAME_LEN} characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`. Nothing else — no named colours,
/// no functions — because the value is interpolated into CSS on a host page
/// and a function is where `url()` (a network request) and `var()` live.
pub fn is_hex_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A font-family list drawn from a fixed character set: letters, digits,
/// spaces, commas, hyphens, underscores and balanced quotes. That covers every
/// real family list (`"Helvetica Neue", Arial, sans-serif`) and excludes `;`,
/// `(`, `)`, `\`, `/` and `:` — everything a declaration break-out, a function
/// or an escape needs.
pub fn is_safe_font_family(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_FONT_FAMILY_LEN
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | ',' | '-' | '_' | '\'' | '"'))
        && value.matches('"').count() % 2 == 0
        && value.matches('\'').count() % 2 == 0
}

/// Normalise and check a settings object. Blank colours and a blank font
/// collapse to `None` (the built-in value); hex is lower-cased so an
/// equivalent edit doesn't read as a change.
pub fn validate_settings(mut settings: ThemeSettings) -> CoreResult<ThemeSettings> {
    for (key, slot) in settings.colors.slots_mut() {
        let Some(raw) = slot.take() else { continue };
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        if !is_hex_color(value) {
            return Err(CoreError::BadRequest(format!(
                "colors.{key} must be a hex colour such as #4f46e5"
            )));
        }
        *slot = Some(value.to_ascii_lowercase());
    }
    if let Some(radius) = settings.radius
        && radius > MAX_RADIUS
    {
        return Err(CoreError::BadRequest(format!(
            "radius must be 0..={MAX_RADIUS} pixels"
        )));
    }
    if let Some(raw) = settings.font_family.take() {
        let value = raw.trim();
        if !value.is_empty() {
            if !is_safe_font_family(value) {
                return Err(CoreError::BadRequest(format!(
                    "font_family must be at most {MAX_FONT_FAMILY_LEN} characters of letters, \
                     digits, spaces, commas, hyphens, underscores and balanced quotes"
                )));
            }
            settings.font_family = Some(value.to_string());
        }
    }
    Ok(settings)
}

/// Decode a stored settings column. Never fails: a row that doesn't parse
/// (written by a newer build, say, then rolled back) is logged and reads as
/// the empty theme, because a theme must never be able to 500 a form.
fn parse_settings(m: &entity::theme::Model) -> ThemeSettings {
    serde_json::from_value(m.settings.clone()).unwrap_or_else(|e| {
        tracing::warn!(theme_id = m.id, error = %e, "stored theme settings failed to parse");
        ThemeSettings::default()
    })
}

fn dto(m: entity::theme::Model, form_count: u64) -> ThemeDto {
    let settings = parse_settings(&m);
    ThemeDto {
        id: m.id,
        name: m.name,
        is_default: m.is_default,
        settings,
        form_count,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

fn settings_json(settings: &ThemeSettings) -> CoreResult<sea_orm::JsonValue> {
    serde_json::to_value(settings)
        .map_err(|e| CoreError::Internal(anyhow::anyhow!("json serialize failed: {e}")))
}

/// Forms naming each theme explicitly, in one grouped query.
async fn form_counts<C: ConnectionTrait>(conn: &C) -> CoreResult<HashMap<i32, u64>> {
    let rows: Vec<(Option<i32>, i64)> = entity::form::Entity::find()
        .select_only()
        .column(entity::form::Column::ThemeId)
        .column_as(entity::form::Column::Id.count(), "n")
        .filter(entity::form::Column::ThemeId.is_not_null())
        .group_by(entity::form::Column::ThemeId)
        .into_tuple()
        .all(conn)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, n)| id.map(|id| (id, n.max(0) as u64)))
        .collect())
}

async fn form_count<C: ConnectionTrait>(conn: &C, id: i32) -> CoreResult<u64> {
    Ok(entity::form::Entity::find()
        .filter(entity::form::Column::ThemeId.eq(id))
        .count(conn)
        .await?)
}

pub async fn list<C: ConnectionTrait>(conn: &C) -> CoreResult<ThemeList> {
    let rows = entity::theme::Entity::find()
        .order_by_asc(entity::theme::Column::Name)
        .all(conn)
        .await?;
    let counts = form_counts(conn).await?;
    let total = rows.len() as u64;
    let items = rows
        .into_iter()
        .map(|m| {
            let n = counts.get(&m.id).copied().unwrap_or(0);
            dto(m, n)
        })
        .collect();
    Ok(ThemeList { items, total })
}

pub async fn find_by_id<C: ConnectionTrait>(
    conn: &C,
    id: i32,
) -> CoreResult<Option<entity::theme::Model>> {
    Ok(entity::theme::Entity::find_by_id(id).one(conn).await?)
}

pub async fn get<C: ConnectionTrait>(conn: &C, id: i32) -> CoreResult<ThemeDto> {
    let row = find_by_id(conn, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("theme not found".into()))?;
    let n = form_count(conn, id).await?;
    Ok(dto(row, n))
}

/// Whether `id` names a theme. Used by form validation.
pub async fn exists<C: ConnectionTrait>(conn: &C, id: i32) -> CoreResult<bool> {
    Ok(find_by_id(conn, id).await?.is_some())
}

/// Unset `is_default` on every theme but `except`.
async fn clear_default<C: ConnectionTrait>(conn: &C, except: Option<i32>) -> CoreResult<()> {
    let mut q = entity::theme::Entity::update_many()
        .col_expr(entity::theme::Column::IsDefault, Expr::value(false))
        .filter(entity::theme::Column::IsDefault.eq(true));
    if let Some(id) = except {
        q = q.filter(entity::theme::Column::Id.ne(id));
    }
    q.exec(conn).await?;
    Ok(())
}

pub async fn create<C: ConnectionTrait>(conn: &C, input: NewTheme) -> CoreResult<ThemeDto> {
    let name = validate_name(&input.name)?;
    let settings = validate_settings(input.settings)?;
    if input.is_default {
        clear_default(conn, None).await?;
    }
    let row = entity::theme::ActiveModel {
        name: ActiveValue::Set(name),
        is_default: ActiveValue::Set(input.is_default),
        settings: ActiveValue::Set(settings_json(&settings)?),
        ..Default::default()
    }
    .insert(conn)
    .await?;
    Ok(dto(row, 0))
}

pub async fn update<C: ConnectionTrait>(
    conn: &C,
    id: i32,
    input: UpdateTheme,
) -> CoreResult<ThemeDto> {
    let existing = find_by_id(conn, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("theme not found".into()))?;
    let mut active: entity::theme::ActiveModel = existing.into();

    if let Some(name) = input.name {
        active.name = ActiveValue::Set(validate_name(&name)?);
    }
    if let Some(settings) = input.settings {
        active.settings = ActiveValue::Set(settings_json(&validate_settings(settings)?)?);
    }
    if let Some(is_default) = input.is_default {
        if is_default {
            clear_default(conn, Some(id)).await?;
        }
        active.is_default = ActiveValue::Set(is_default);
    }

    let row = active.update(conn).await?;
    let n = form_count(conn, id).await?;
    Ok(dto(row, n))
}

/// Delete a theme. Forms that named it go back to `NULL`, i.e. the default
/// theme — the same set-null cleanup `reps::service::delete` does, since there
/// are no DB-level foreign keys. Deleting the default leaves no default, and
/// those forms render with the built-in look.
pub async fn delete<C: ConnectionTrait>(conn: &C, id: i32) -> CoreResult<()> {
    if find_by_id(conn, id).await?.is_none() {
        return Err(CoreError::NotFound("theme not found".into()));
    }
    entity::form::Entity::update_many()
        .col_expr(
            entity::form::Column::ThemeId,
            Expr::value(sea_orm::Value::Int(None)),
        )
        .filter(entity::form::Column::ThemeId.eq(id))
        .exec(conn)
        .await?;
    entity::theme::Entity::delete_by_id(id).exec(conn).await?;
    Ok(())
}

/// The settings a form renders with: its own theme, else the workspace
/// default, else `None` (the built-in look).
///
/// Resolved here, on the server, so the renderer never needs to know what
/// "default" means. The default is picked `ORDER BY id` so that if a race ever
/// left two rows flagged, every read still agrees on one of them.
pub async fn resolve_settings<C: ConnectionTrait>(
    conn: &C,
    theme_id: Option<i32>,
) -> CoreResult<Option<ThemeSettings>> {
    let own = match theme_id {
        Some(id) => find_by_id(conn, id).await?,
        None => None,
    };
    let row = match own {
        Some(row) => Some(row),
        None => {
            entity::theme::Entity::find()
                .filter(entity::theme::Column::IsDefault.eq(true))
                .order_by_asc(entity::theme::Column::Id)
                .one(conn)
                .await?
        }
    };
    Ok(row.map(|m| parse_settings(&m)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::{Density, ThemeColors};

    #[test]
    fn hex_colours() {
        for ok in ["#fff", "#FFFF", "#4f46e5", "#4f46e580"] {
            assert!(is_hex_color(ok), "{ok}");
        }
        for bad in [
            "fff",
            "#ffff0",
            "#ggg",
            "red",
            "url(https://x.test/a.png)",
            "var(--x)",
            "#fff;background:red",
            "",
        ] {
            assert!(!is_hex_color(bad), "{bad}");
        }
    }

    #[test]
    fn font_families() {
        for ok in [
            "Inter",
            "\"Helvetica Neue\", Arial, sans-serif",
            "'IBM Plex Sans'",
        ] {
            assert!(is_safe_font_family(ok), "{ok}");
        }
        for bad in [
            "Inter; color: red",
            "url(x)",
            "var(--x)",
            "\"Unbalanced, serif",
            "a\\62 c",
            "",
        ] {
            assert!(!is_safe_font_family(bad), "{bad}");
        }
    }

    #[test]
    fn validation_normalises_and_rejects() {
        let settings = ThemeSettings {
            colors: ThemeColors {
                accent: Some(" #4F46E5 ".into()),
                text: Some("   ".into()),
                ..Default::default()
            },
            font_family: Some("  ".into()),
            radius: Some(MAX_RADIUS),
            density: Density::Compact,
            ..Default::default()
        };
        let out = validate_settings(settings).unwrap();
        assert_eq!(out.colors.accent.as_deref(), Some("#4f46e5"));
        assert_eq!(out.colors.text, None);
        assert_eq!(out.font_family, None);

        let bad_colour = ThemeSettings {
            colors: ThemeColors {
                background: Some("url(https://x.test)".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(matches!(
            validate_settings(bad_colour),
            Err(CoreError::BadRequest(m)) if m.contains("colors.background")
        ));

        let bad_radius = ThemeSettings {
            radius: Some(MAX_RADIUS + 1),
            ..Default::default()
        };
        assert!(validate_settings(bad_radius).is_err());
    }
}
