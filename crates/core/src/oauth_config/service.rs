//! Persistence + validation for the (singleton) active OAuth provider config.

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};

use super::{
    DiscoveryPrefill, DiscoveryRequest, OAuthConfigPublicDto, UpsertOAuthConfig,
};
use crate::error::{CoreError, CoreResult};
use crate::oauth::discovery::fetch_discovery;

const DEFAULT_EMAIL_CLAIM: &str = "email";
const DEFAULT_SUBJECT_CLAIM: &str = "sub";
const DEFAULT_SCOPES: &str = "openid email profile";

pub async fn get_active<C: ConnectionTrait>(
    conn: &C,
) -> CoreResult<Option<entity::oauth_provider_config::Model>> {
    Ok(entity::oauth_provider_config::Entity::find()
        .filter(entity::oauth_provider_config::Column::IsActive.eq(true))
        .one(conn)
        .await?)
}

pub async fn get_public<C: ConnectionTrait>(conn: &C) -> CoreResult<OAuthConfigPublicDto> {
    Ok(match get_active(conn).await? {
        Some(m) => OAuthConfigPublicDto {
            enabled: true,
            display_name: Some(m.display_name),
        },
        None => OAuthConfigPublicDto {
            enabled: false,
            display_name: None,
        },
    })
}

pub async fn upsert<C: ConnectionTrait>(
    conn: &C,
    input: UpsertOAuthConfig,
) -> CoreResult<entity::oauth_provider_config::Model> {
    let display_name = input.display_name.trim().to_string();
    if display_name.is_empty() {
        return Err(CoreError::BadRequest("display_name is required".into()));
    }
    let client_id = input.client_id.trim().to_string();
    if client_id.is_empty() {
        return Err(CoreError::BadRequest("client_id is required".into()));
    }
    validate_url("authorize_url", &input.authorize_url)?;
    validate_url("token_url", &input.token_url)?;
    if let Some(u) = input.userinfo_url.as_deref() {
        if !u.is_empty() {
            validate_url("userinfo_url", u)?;
        }
    }
    if let Some(u) = input.jwks_url.as_deref() {
        if !u.is_empty() {
            validate_url("jwks_url", u)?;
        }
    }
    let scopes = input.scopes.trim();
    let scopes = if scopes.is_empty() {
        DEFAULT_SCOPES.to_string()
    } else {
        scopes.to_string()
    };
    let email_claim = input
        .email_claim
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_EMAIL_CLAIM)
        .to_string();
    let subject_claim = input
        .subject_claim
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_SUBJECT_CLAIM)
        .to_string();

    // Validate default_role_id refers to an actual role.
    if let Some(role_id) = input.default_role_id {
        let exists = entity::role::Entity::find_by_id(role_id)
            .one(conn)
            .await?
            .is_some();
        if !exists {
            return Err(CoreError::BadRequest(format!(
                "default_role_id {role_id} does not exist"
            )));
        }
    }

    let existing = get_active(conn).await?;
    let now_secret = input
        .client_secret
        .as_deref()
        .map(str::trim)
        .map(|s| s.to_string());

    match existing {
        Some(model) => {
            let secret_to_store = match now_secret {
                Some(s) if !s.is_empty() => s,
                Some(_) => return Err(CoreError::BadRequest("client_secret cannot be empty".into())),
                None => model.client_secret.clone(),
            };
            let mut active: entity::oauth_provider_config::ActiveModel = model.into();
            active.display_name = ActiveValue::Set(display_name);
            active.discovery_url = ActiveValue::Set(input.discovery_url);
            active.issuer = ActiveValue::Set(input.issuer);
            active.client_id = ActiveValue::Set(client_id);
            active.client_secret = ActiveValue::Set(secret_to_store);
            active.authorize_url = ActiveValue::Set(input.authorize_url);
            active.token_url = ActiveValue::Set(input.token_url);
            active.userinfo_url = ActiveValue::Set(input.userinfo_url);
            active.jwks_url = ActiveValue::Set(input.jwks_url);
            active.scopes = ActiveValue::Set(scopes);
            active.default_role_id = ActiveValue::Set(input.default_role_id);
            active.email_claim = ActiveValue::Set(email_claim);
            active.subject_claim = ActiveValue::Set(subject_claim);
            Ok(active.update(conn).await?)
        }
        None => {
            let secret = match now_secret {
                Some(s) if !s.is_empty() => s,
                _ => {
                    return Err(CoreError::BadRequest(
                        "client_secret is required when creating a new OAuth config".into(),
                    ));
                }
            };
            let active = entity::oauth_provider_config::ActiveModel {
                kind: ActiveValue::Set("oidc".to_string()),
                display_name: ActiveValue::Set(display_name),
                is_active: ActiveValue::Set(true),
                discovery_url: ActiveValue::Set(input.discovery_url),
                issuer: ActiveValue::Set(input.issuer),
                client_id: ActiveValue::Set(client_id),
                client_secret: ActiveValue::Set(secret),
                authorize_url: ActiveValue::Set(input.authorize_url),
                token_url: ActiveValue::Set(input.token_url),
                userinfo_url: ActiveValue::Set(input.userinfo_url),
                jwks_url: ActiveValue::Set(input.jwks_url),
                scopes: ActiveValue::Set(scopes),
                default_role_id: ActiveValue::Set(input.default_role_id),
                email_claim: ActiveValue::Set(email_claim),
                subject_claim: ActiveValue::Set(subject_claim),
                ..Default::default()
            };
            Ok(active.insert(conn).await?)
        }
    }
}

pub async fn delete_active<C: ConnectionTrait>(conn: &C) -> CoreResult<()> {
    let Some(model) = get_active(conn).await? else {
        return Err(CoreError::NotFound("no active oauth config".into()));
    };
    entity::oauth_provider_config::Entity::delete_by_id(model.id)
        .exec(conn)
        .await?;
    Ok(())
}

pub async fn discover(req: DiscoveryRequest) -> CoreResult<DiscoveryPrefill> {
    let doc = fetch_discovery(req.discovery_url.trim()).await?;
    Ok(DiscoveryPrefill {
        issuer: doc.issuer,
        authorize_url: doc.authorization_endpoint,
        token_url: doc.token_endpoint,
        userinfo_url: doc.userinfo_endpoint,
        jwks_url: doc.jwks_uri,
        scopes_supported: doc.scopes_supported,
    })
}

fn validate_url(field: &str, url: &str) -> CoreResult<()> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(CoreError::BadRequest(format!("{field} is required")));
    }
    if let Some(rest) = trimmed.strip_prefix("https://") {
        if rest.is_empty() {
            return Err(CoreError::BadRequest(format!("{field} is not a valid URL")));
        }
        return Ok(());
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        // Permit http only for localhost / loopback during development.
        let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let host = &rest[..host_end];
        let host_no_port = host.split(':').next().unwrap_or("");
        let is_localhost = matches!(host_no_port, "localhost" | "127.0.0.1" | "[::1]" | "::1");
        if is_localhost {
            return Ok(());
        }
        return Err(CoreError::BadRequest(format!("{field} must use HTTPS")));
    }
    Err(CoreError::BadRequest(format!(
        "{field} must use HTTPS scheme"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_url_accepts_https() {
        assert!(validate_url("x", "https://example.com/auth").is_ok());
    }

    #[test]
    fn validate_url_rejects_http_except_localhost() {
        assert!(validate_url("x", "http://example.com/auth").is_err());
        assert!(validate_url("x", "http://localhost:8080/auth").is_ok());
        assert!(validate_url("x", "http://127.0.0.1/auth").is_ok());
    }

    #[test]
    fn validate_url_rejects_garbage() {
        assert!(validate_url("x", "not a url").is_err());
        assert!(validate_url("x", "").is_err());
    }
}
