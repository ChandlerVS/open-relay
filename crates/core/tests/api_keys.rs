//! Round-trip guard for the API-key credential.
//!
//! The properties that matter here are security properties, and each one is a
//! thing that would fail silently if it regressed:
//!
//! * the plaintext is never recoverable from the row,
//! * a revoked or expired key stops working,
//! * scopes **narrow** and can never widen — including when the owner's roles
//!   change after the key was issued.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test api_keys -- --ignored --nocapture
//! ```

use open_relay_core::api_keys::{NewApiKey, TOKEN_PREFIX, service};
use open_relay_core::permissions::Permission;
use open_relay_core::rbac::{NewRole, service as rbac_service};
use open_relay_core::users::{NewUser, service as users_service};
use sea_orm::{ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter};

/// A throwaway user with exactly `perms`, plus the role id so it can be
/// re-pointed mid-test.
async fn seed_user(db: &DatabaseConnection, tag: &str, perms: Vec<Permission>) -> (i32, i32) {
    let unique = format!("{tag}-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default());
    let role = rbac_service::create_role(
        db,
        NewRole {
            name: format!("role-{unique}"),
            description: None,
            permissions: perms,
        },
    )
    .await
    .expect("create role");
    let user = users_service::create_user(
        db,
        NewUser {
            email: format!("{unique}@example.test"),
            display_name: None,
            password: "correct-horse-battery-staple".into(),
            role_ids: vec![role.id],
        },
    )
    .await
    .expect("create user");
    // `create_user` does not honour `NewUser::role_ids` — the route assigns
    // separately, so the test has to as well.
    rbac_service::assign_roles_to_user(
        db,
        user.id,
        &[role.id],
        -1,
        &open_relay_core::rbac::AssignActor::system(),
    )
    .await
    .expect("assign role");
    (user.id, role.id)
}

async fn cleanup(db: &DatabaseConnection, user_id: i32, role_id: i32) {
    // Goes through the real cascade, which is also how we assert that deleting
    // a user takes their keys with them.
    let _ = users_service::delete_user(db, -1, user_id, -1).await;
    let _ = rbac_service::delete_role(db, role_id).await;
}

async fn connect() -> DatabaseConnection {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    Database::connect(&url).await.expect("connect")
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn a_key_authenticates_once_issued_and_stops_when_revoked() {
    let db = connect().await;
    let (user_id, role_id) = seed_user(&db, "ak-basic", vec![Permission::FormsRead]).await;

    let created = service::issue(
        &db,
        user_id,
        NewApiKey {
            name: "  Claude Desktop  ".into(),
            scopes: None,
            expires_in_days: None,
        },
    )
    .await
    .expect("issue");

    assert!(created.token.starts_with(TOKEN_PREFIX));
    assert_eq!(created.key.name, "Claude Desktop", "name is trimmed");
    assert!(created.key.revoked_at.is_none());
    assert!(created.key.last_used_at.is_none());

    // The plaintext must not be recoverable from storage.
    let row = entity::api_key::Entity::find_by_id(created.key.id)
        .one(&db)
        .await
        .expect("query")
        .expect("row");
    assert_ne!(row.token_hash, created.token);
    assert!(
        !row.token_hash.contains(&created.token[TOKEN_PREFIX.len()..]),
        "the stored hash must not embed the secret"
    );

    let actor = service::authenticate(&db, &created.token)
        .await
        .expect("authenticate");
    assert_eq!(actor.user_id, user_id);
    assert!(actor.has(Permission::FormsRead));
    assert!(!actor.has(Permission::FormsWrite));

    // `last_used_at` is stamped by a successful auth.
    let keys = service::list_for_user(&db, user_id).await.expect("list");
    assert_eq!(keys.len(), 1);
    assert!(keys[0].last_used_at.is_some(), "auth stamps last_used_at");

    service::revoke(&db, user_id, created.key.id)
        .await
        .expect("revoke");
    assert!(
        service::authenticate(&db, &created.token).await.is_err(),
        "a revoked key must stop authenticating"
    );

    // Revoking twice is a 404, not a silent success — the row is no longer
    // active, so there is nothing to revoke.
    assert!(service::revoke(&db, user_id, created.key.id).await.is_err());

    cleanup(&db, user_id, role_id).await;
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn scopes_narrow_and_track_the_owners_live_permissions() {
    let db = connect().await;
    let (user_id, role_id) = seed_user(
        &db,
        "ak-scope",
        vec![
            Permission::FormsRead,
            Permission::FormsWrite,
            Permission::UsersRead,
        ],
    )
    .await;

    // Scoping to a permission the owner lacks is refused up front, rather than
    // quietly minting a key whose scope is inert.
    let refused = service::issue(
        &db,
        user_id,
        NewApiKey {
            name: "too broad".into(),
            scopes: Some(vec![Permission::RolesDelete]),
            expires_in_days: None,
        },
    )
    .await;
    assert!(refused.is_err(), "cannot scope beyond what the owner holds");

    // An explicitly empty scope list is a mistake, not a lockdown.
    let empty = service::issue(
        &db,
        user_id,
        NewApiKey {
            name: "empty".into(),
            scopes: Some(vec![]),
            expires_in_days: None,
        },
    )
    .await;
    assert!(empty.is_err(), "an empty scope list is rejected");

    let created = service::issue(
        &db,
        user_id,
        NewApiKey {
            name: "forms only".into(),
            scopes: Some(vec![Permission::FormsRead, Permission::FormsWrite]),
            expires_in_days: Some(30),
        },
    )
    .await
    .expect("issue");
    assert!(created.key.expires_at.is_some());

    let actor = service::authenticate(&db, &created.token)
        .await
        .expect("authenticate");
    assert!(actor.has(Permission::FormsRead));
    assert!(actor.has(Permission::FormsWrite));
    assert!(
        !actor.has(Permission::UsersRead),
        "a scope must narrow the owner's set, not pass it through"
    );

    // Now take a permission away from the owner's role. The key must narrow
    // with it — permissions are resolved per request, never frozen at issue.
    rbac_service::update_role(
        &db,
        role_id,
        open_relay_core::rbac::UpdateRole {
            name: None,
            description: None,
            permissions: Some(vec![Permission::FormsRead, Permission::UsersRead]),
        },
    )
    .await
    .expect("update role");

    let actor = service::authenticate(&db, &created.token)
        .await
        .expect("authenticate");
    assert!(actor.has(Permission::FormsRead));
    assert!(
        !actor.has(Permission::FormsWrite),
        "revoking the owner's permission must narrow the key immediately"
    );

    cleanup(&db, user_id, role_id).await;
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn keys_are_scoped_to_their_owner_and_die_with_them() {
    let db = connect().await;
    let (alice, alice_role) = seed_user(&db, "ak-alice", vec![Permission::FormsRead]).await;
    let (bob, bob_role) = seed_user(&db, "ak-bob", vec![Permission::FormsRead]).await;

    let alice_key = service::issue(
        &db,
        alice,
        NewApiKey {
            name: "alice".into(),
            scopes: None,
            expires_in_days: None,
        },
    )
    .await
    .expect("issue");

    // Bob cannot revoke Alice's key, and gets a not-found rather than a
    // forbidden — the id is not worth confirming.
    let err = service::revoke(&db, bob, alice_key.key.id).await.unwrap_err();
    assert!(
        matches!(err, open_relay_core::error::CoreError::NotFound(_)),
        "another user's key id must be indistinguishable from a missing one, got {err:?}"
    );
    assert!(service::list_for_user(&db, bob).await.expect("list").is_empty());

    // Garbage never authenticates, and a JWT-shaped string is rejected without
    // a query.
    assert!(service::authenticate(&db, "not-a-key").await.is_err());
    assert!(service::authenticate(&db, "eyJhbGciOiJIUzI1NiJ9.e30.x").await.is_err());

    // Deleting the owner takes the key with it.
    users_service::delete_user(&db, bob, alice, -1)
        .await
        .expect("delete alice");
    assert!(
        service::authenticate(&db, &alice_key.token).await.is_err(),
        "a deleted user's key must stop working"
    );
    let orphans = entity::api_key::Entity::find()
        .filter(entity::api_key::Column::UserId.eq(alice))
        .all(&db)
        .await
        .expect("query");
    assert!(orphans.is_empty(), "user deletion must cascade to api keys");

    cleanup(&db, bob, bob_role).await;
    let _ = rbac_service::delete_role(&db, alice_role).await;
}
