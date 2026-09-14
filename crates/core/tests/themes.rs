//! Read-after-write check for themes and `form.theme_id`.
//!
//! Pins the three behaviours the renderer and admin rely on without being able
//! to see: settings are normalised on write and read back verbatim, only one
//! theme is ever the default, and resolution walks own theme → default →
//! built-in — including after the theme a form named is deleted.
//!
//! Everything runs inside one transaction that is rolled back, because the
//! default is workspace-wide state: this must not clobber the default theme of
//! whatever dev database it points at.
//!
//! Needs a live MySQL and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test themes -- --ignored --nocapture
//! ```

use open_relay_core::backend::BackendRegistry;
use open_relay_core::error::CoreError;
use open_relay_core::forms::{NewForm, UpdateForm, service as forms};
use open_relay_core::themes::{
    Density, NewTheme, ThemeColors, ThemeSettings, UpdateTheme, service as themes,
};
use sea_orm::{Database, TransactionTrait};

fn registry() -> BackendRegistry {
    let mut r = BackendRegistry::new();
    r.register_static(std::sync::Arc::new(
        open_relay_core::backend::openrelay::OpenRelayBackend,
    ));
    r
}

fn new_form(slug: &str, theme_id: Option<i32>) -> NewForm {
    NewForm {
        name: "Theme integration".into(),
        display_name: None,
        slug: Some(slug.into()),
        standard_fields: None,
        custom_fields: vec![],
        layout: None,
        backends: None,
        tags: vec![],
        reps: vec![],
        source_params: vec![],
        post_submission_action: Default::default(),
        progress_indicator: Default::default(),
        theme_id,
        metadata: None,
    }
}

fn accent(hex: &str) -> ThemeSettings {
    ThemeSettings {
        colors: ThemeColors {
            accent: Some(hex.into()),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn new_theme(name: &str, is_default: bool, settings: ThemeSettings) -> NewTheme {
    NewTheme {
        name: name.into(),
        is_default,
        settings,
    }
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn themes_round_trip_resolve_and_clean_up() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let tx = db.begin().await.expect("begin");
    let reg = registry();
    let pid = std::process::id();

    // 1. Settings are normalised on write — name and font trimmed, hex
    //    lower-cased, a blank colour dropped — and read back verbatim.
    let brand = themes::create(
        &tx,
        new_theme(
            "  Brand  ",
            false,
            ThemeSettings {
                colors: ThemeColors {
                    accent: Some("#4F46E5".into()),
                    text: Some("   ".into()),
                    ..Default::default()
                },
                radius: Some(12),
                font_family: Some(" Inter, sans-serif ".into()),
                density: Density::Compact,
                ..Default::default()
            },
        ),
    )
    .await
    .expect("create brand");
    assert_eq!(brand.name, "Brand");
    assert_eq!(brand.settings.colors.accent.as_deref(), Some("#4f46e5"));
    assert_eq!(brand.settings.colors.text, None);
    assert_eq!(
        brand.settings.font_family.as_deref(),
        Some("Inter, sans-serif")
    );
    assert_eq!(brand.form_count, 0);
    assert!(!brand.is_default);
    assert_eq!(
        themes::get(&tx, brand.id).await.expect("get").settings,
        brand.settings
    );

    // 2. A value that could make a request from the host page is a 400.
    let err = themes::create(
        &tx,
        new_theme("Hostile", false, accent("url(https://x.test)")),
    )
    .await
    .expect_err("url() colour must be rejected");
    assert!(matches!(err, CoreError::BadRequest(_)), "{err:?}");

    // 3. One default at a time, whether set on create or on update.
    let house = themes::create(&tx, new_theme("House", true, accent("#111111")))
        .await
        .expect("create house");
    let other = themes::create(&tx, new_theme("Other", true, accent("#222222")))
        .await
        .expect("create other");
    let defaults = |list: &open_relay_core::themes::ThemeList| -> Vec<i32> {
        list.items
            .iter()
            .filter(|t| t.is_default)
            .map(|t| t.id)
            .collect()
    };
    assert_eq!(
        defaults(&themes::list(&tx).await.expect("list")),
        vec![other.id]
    );
    let house = themes::update(
        &tx,
        house.id,
        UpdateTheme {
            is_default: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("make house default");
    assert!(house.is_default);
    assert_eq!(
        defaults(&themes::list(&tx).await.expect("list")),
        vec![house.id]
    );

    // 4. A form may only name a theme that exists.
    let err = forms::create_form(
        &tx,
        &reg,
        1,
        new_form(&format!("theme-it-bad-{pid}"), Some(i32::MAX)),
    )
    .await
    .expect_err("unknown theme must be rejected");
    assert!(matches!(err, CoreError::BadRequest(_)), "{err:?}");

    // 5. A form's own theme wins over the default, and is counted.
    let form = forms::create_form(
        &tx,
        &reg,
        1,
        new_form(&format!("theme-it-{pid}"), Some(brand.id)),
    )
    .await
    .expect("create form");
    assert_eq!(form.theme_id, Some(brand.id));
    assert_eq!(
        forms::dto_from_model(&tx, form.clone())
            .await
            .expect("dto")
            .theme_id,
        Some(brand.id)
    );
    assert_eq!(
        themes::resolve_settings(&tx, form.theme_id)
            .await
            .expect("resolve"),
        Some(brand.settings.clone())
    );
    assert_eq!(themes::get(&tx, brand.id).await.expect("get").form_count, 1);
    let listed = themes::list(&tx).await.expect("list");
    let listed_brand = listed
        .items
        .iter()
        .find(|t| t.id == brand.id)
        .expect("brand listed");
    assert_eq!(listed_brand.form_count, 1);

    // 6. An unrelated PATCH leaves the theme alone.
    let form = forms::update_form(
        &tx,
        &reg,
        form.id,
        UpdateForm {
            name: Some("Renamed".into()),
            ..Default::default()
        },
    )
    .await
    .expect("rename");
    assert_eq!(form.theme_id, Some(brand.id));

    // 7. `0` clears back to NULL, which resolves to the default.
    let form = forms::update_form(
        &tx,
        &reg,
        form.id,
        UpdateForm {
            theme_id: Some(0),
            ..Default::default()
        },
    )
    .await
    .expect("clear theme");
    assert_eq!(form.theme_id, None);
    assert_eq!(
        themes::resolve_settings(&tx, form.theme_id)
            .await
            .expect("resolve"),
        Some(house.settings.clone())
    );

    // 8. Deleting a form's theme resets it to NULL rather than blocking, and
    //    it falls back to the default.
    let form = forms::update_form(
        &tx,
        &reg,
        form.id,
        UpdateForm {
            theme_id: Some(brand.id),
            ..Default::default()
        },
    )
    .await
    .expect("set theme again");
    assert_eq!(form.theme_id, Some(brand.id));
    themes::delete(&tx, brand.id).await.expect("delete brand");
    let form = forms::find_by_id(&tx, form.id)
        .await
        .expect("find")
        .expect("form still exists");
    assert_eq!(form.theme_id, None);
    assert_eq!(
        themes::resolve_settings(&tx, form.theme_id)
            .await
            .expect("resolve"),
        Some(house.settings.clone())
    );
    // A dangling id reads the same way, should one ever exist.
    assert_eq!(
        themes::resolve_settings(&tx, Some(brand.id))
            .await
            .expect("resolve"),
        Some(house.settings.clone())
    );

    // 9. Deleting the default leaves no default: the built-in look.
    themes::delete(&tx, house.id).await.expect("delete house");
    assert_eq!(
        themes::resolve_settings(&tx, None).await.expect("resolve"),
        None
    );
    assert!(matches!(
        themes::delete(&tx, house.id).await,
        Err(CoreError::NotFound(_))
    ));

    tx.rollback().await.expect("rollback");
}
