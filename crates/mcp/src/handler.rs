//! The MCP server: tools over `open_relay_core::forms`.
//!
//! # What these tools are, and what they deliberately are not
//!
//! Every tool is a thin projection over `forms::service`, in exactly the way
//! `apps/server/src/routes/forms.rs` is. Nothing here validates a layout, sorts
//! a field list, recomputes the legacy columns, or checks a visibility rule —
//! all of that belongs to `service::update_form`, which every write path here
//! funnels through. A tool that re-implemented any of it would be a second copy
//! of a specification that has one owner.
//!
//! # Authorization
//!
//! A tool call carries no session. The actor arrives one of two ways:
//!
//! * **stdio** resolves one API key at startup and holds it in `fixed_actor`.
//! * **HTTP** has middleware authenticate per request and stash an [`ApiActor`]
//!   in the request extensions; rmcp forwards the whole `http::request::Parts`
//!   into the tool's `RequestContext`, so the actor comes back out one level
//!   down. See [`OpenRelayMcp::actor`].
//!
//! Permissions are the same `forms:read` / `forms:write` / `forms:delete` the
//! REST handlers require, resolved live from the owner's roles on every call.

use open_relay_core::api_keys::ApiActor;
use open_relay_core::backend::BackendRegistry;
use open_relay_core::error::CoreError;
use open_relay_core::forms::schema as core_schema;
use open_relay_core::forms::{
    FormElement, ListQuery, NewForm, PostSubmissionAction, ProgressIndicator, UpdateForm, service,
};
use open_relay_core::forms::edit::{self, ElementRef};
use open_relay_core::permissions::Permission;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, schemars, tool, tool_handler, tool_router,
};
use sea_orm::{DatabaseConnection, EntityTrait, QuerySelect, TransactionTrait};
use serde_json::json;

use crate::instructions::INSTRUCTIONS;
use crate::schema;

/// URI of the resource mirroring `describe_form_schema`. Some clients surface
/// resources to the user for browsing while tools are model-facing only, so the
/// same content is offered both ways.
pub const SCHEMA_RESOURCE_URI: &str = "openrelay://schema/form-layout";

#[derive(Clone)]
pub struct OpenRelayMcp {
    db: DatabaseConnection,
    backends: BackendRegistry,
    /// `Some` for stdio, where one key is resolved at startup. `None` for HTTP,
    /// where the actor is per request.
    fixed_actor: Option<ApiActor>,
    tool_router: ToolRouter<OpenRelayMcp>,
}

// ---------------------------------------------------------------- error mapping

/// Map a domain error onto the JSON-RPC error the agent sees.
///
/// `Internal`, `Db` and `Crypto` are collapsed to a bare "internal error" for
/// the same reason `AppError::into_response` does it: the detail is for the
/// operator's logs, not for a caller who might be relaying it onward. Everything
/// else is a caller mistake, and its message is the actionable part — a layout
/// rejection names the offending key, which is the whole value of surfacing it.
fn to_mcp_error(err: CoreError) -> McpError {
    match err {
        CoreError::BadRequest(m) => McpError::invalid_params(m, None),
        CoreError::NotFound(m) => McpError::invalid_params(m, None),
        CoreError::Conflict(m) => McpError::invalid_params(m, None),
        CoreError::Forbidden(m) => McpError::invalid_request(m, None),
        CoreError::Unauthorized => McpError::invalid_request("unauthorized", None),
        other => {
            tracing::error!(?other, "mcp tool failed");
            McpError::internal_error("internal error", None)
        }
    }
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> McpError {
    match err {
        sea_orm::TransactionError::Connection(db) => to_mcp_error(CoreError::Db(db)),
        sea_orm::TransactionError::Transaction(core) => to_mcp_error(core),
    }
}

/// Render a serializable result as the tool's text output.
///
/// Pretty-printed on purpose: the consumer is a language model, and the
/// indentation is what makes a 300-element layout legible enough to edit.
fn ok_json<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| McpError::internal_error(format!("serializing result: {e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

// ---------------------------------------------------------------- tool params
//
// Structured members are `serde_json::Value` here and get their real schema
// from `crate::schema` via each tool's `input_schema`. Deserialization into the
// concrete type happens in the tool body, so a malformed element is reported as
// a parameter error naming the field rather than as a whole-request parse
// failure with no context.

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct ListParams {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct GetFormParams {
    pub id: Option<i32>,
    pub slug: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct CreateFormParams {
    #[serde(default)]
    pub form: serde_json::Value,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct UpdateFormParams {
    pub id: i32,
    #[serde(default)]
    pub changes: serde_json::Value,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct FormIdParams {
    pub id: i32,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct AddElementParams {
    pub form_id: i32,
    #[serde(default)]
    pub element: serde_json::Value,
    pub at: Option<usize>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct UpdateElementParams {
    pub form_id: i32,
    pub key: Option<String>,
    pub index: Option<usize>,
    #[serde(default)]
    pub element: serde_json::Value,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct TargetParams {
    pub form_id: i32,
    pub key: Option<String>,
    pub index: Option<usize>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct MoveElementParams {
    pub form_id: i32,
    pub key: Option<String>,
    pub index: Option<usize>,
    pub to: usize,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct SetActionParams {
    pub form_id: i32,
    #[serde(default)]
    pub action: serde_json::Value,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct SetIndicatorParams {
    pub form_id: i32,
    #[serde(default)]
    pub indicator: serde_json::Value,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct NoParams {}

/// Deserialize a structured parameter, naming it in the error.
///
/// `deny_unknown_fields` is on most of these types, so a typo'd key inside a
/// layout element surfaces here with the offending name — which is the single
/// most useful thing to tell an agent that is guessing at a shape.
fn parse_param<T: serde::de::DeserializeOwned>(
    name: &str,
    value: serde_json::Value,
) -> Result<T, McpError> {
    if value.is_null() {
        return Err(McpError::invalid_params(
            format!("`{name}` is required"),
            None,
        ));
    }
    serde_json::from_value(value)
        .map_err(|e| McpError::invalid_params(format!("invalid `{name}`: {e}"), None))
}

fn element_ref(key: Option<String>, index: Option<usize>) -> Result<ElementRef, McpError> {
    match (key, index) {
        (Some(k), None) => Ok(ElementRef::Key(k)),
        (None, Some(i)) => Ok(ElementRef::Index(i)),
        (Some(_), Some(_)) => Err(McpError::invalid_params(
            "give `key` or `index`, not both".to_string(),
            None,
        )),
        (None, None) => Err(McpError::invalid_params(
            "give `key` (a field's key) or `index` (for headings, dividers and row markers)"
                .to_string(),
            None,
        )),
    }
}

// ---------------------------------------------------------------- schema shorthands

fn int(desc: &str) -> serde_json::Value {
    json!({ "type": "integer", "description": desc })
}
fn opt_int(desc: &str) -> serde_json::Value {
    json!({ "type": ["integer", "null"], "description": desc })
}
fn opt_str(desc: &str) -> serde_json::Value {
    json!({ "type": ["string", "null"], "description": desc })
}

impl OpenRelayMcp {
    /// The HTTP flavour: no baked-in actor, one is read per request.
    pub fn http(db: DatabaseConnection, backends: BackendRegistry) -> Self {
        Self {
            db,
            backends,
            fixed_actor: None,
            tool_router: Self::tool_router(),
        }
    }

    /// The stdio flavour: one actor for the life of the process.
    pub fn stdio(db: DatabaseConnection, backends: BackendRegistry, actor: ApiActor) -> Self {
        Self {
            db,
            backends,
            fixed_actor: Some(actor),
            tool_router: Self::tool_router(),
        }
    }

    /// Resolve the caller.
    ///
    /// rmcp forwards the inbound request as a whole `http::request::Parts` —
    /// not as individual extension types — so the actor our middleware inserted
    /// is one level down, inside `parts.extensions`. Reaching for
    /// `ctx.extensions.get::<ApiActor>()` directly would compile and always
    /// return `None`, which is why this is a named method rather than an inline
    /// lookup at each of a dozen call sites.
    fn actor(&self, ctx: &RequestContext<RoleServer>) -> Result<ApiActor, McpError> {
        if let Some(actor) = &self.fixed_actor {
            return Ok(actor.clone());
        }
        ctx.extensions
            .get::<http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<ApiActor>())
            .cloned()
            .ok_or_else(|| {
                McpError::invalid_request(
                    "unauthenticated: present an OpenRelay API key as `Authorization: Bearer orl_...`",
                    None,
                )
            })
    }

    fn authorize(
        &self,
        ctx: &RequestContext<RoleServer>,
        needed: Permission,
    ) -> Result<ApiActor, McpError> {
        let actor = self.actor(ctx)?;
        require(&actor, needed)?;
        Ok(actor)
    }

    /// Read a form's current layout, apply `edit`, and write the whole layout
    /// back through the normal update path.
    ///
    /// Two things make this safe under concurrency and under a bad edit:
    ///
    /// * the read takes the row under `lock_exclusive`, inside the same
    ///   transaction as the write, so two agents editing one form serialize
    ///   instead of losing an update;
    /// * the result goes through `service::update_form`, which runs the same
    ///   normalize → validate → recompute-legacy-columns pipeline a REST layout
    ///   write does. A helper therefore cannot produce a layout the API would
    ///   have rejected.
    pub async fn edit_layout<F>(
        &self,
        form_id: i32,
        edit: F,
    ) -> Result<open_relay_core::forms::FormDto, McpError>
    where
        F: FnOnce(Vec<FormElement>) -> Result<Vec<FormElement>, CoreError> + Send + 'static,
    {
        let registry = self.backends.clone();
        self.db
            .transaction::<_, open_relay_core::forms::FormDto, CoreError>(move |tx| {
                Box::pin(async move {
                    let model = entity::form::Entity::find_by_id(form_id)
                        .lock_exclusive()
                        .one(tx)
                        .await?
                        .ok_or_else(|| CoreError::NotFound("form not found".into()))?;
                    let layout = service::layout_from_model(&model)?;
                    let next = edit(layout)?;
                    let updated = service::update_form(
                        tx,
                        &registry,
                        form_id,
                        UpdateForm {
                            layout: Some(next),
                            ..Default::default()
                        },
                    )
                    .await?;
                    service::dto_from_model(tx, updated).await
                })
            })
            .await
            .map_err(unwrap_tx)
    }
}

/// Permission check, split out from actor resolution so it can be exercised
/// without a live transport to build a `RequestContext` from.
pub fn require(actor: &ApiActor, needed: Permission) -> Result<(), McpError> {
    if !actor.has(needed) {
        return Err(McpError::invalid_request(
            format!("missing permission: {}", needed.slug()),
            None,
        ));
    }
    Ok(())
}

#[tool_router]
impl OpenRelayMcp {
    #[tool(
        description = "List forms, newest ids last. Returns full form objects including their layout.",
        input_schema = schema::object(
            vec![
                ("limit", opt_int("Max forms to return (1-200, default 50).")),
                ("offset", opt_int("How many to skip, for paging.")),
            ],
            &[],
        )
    )]
    async fn list_forms(
        &self,
        Parameters(p): Parameters<ListParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.authorize(&ctx, Permission::FormsRead)?;
        let list = service::list_forms(
            &self.db,
            &ListQuery {
                limit: p.limit,
                offset: p.offset,
            },
        )
        .await
        .map_err(to_mcp_error)?;
        ok_json(&list)
    }

    #[tool(
        description = "Fetch one form by id or slug, including its full layout. Read this before editing.",
        input_schema = schema::object(
            vec![
                ("id", opt_int("The form's numeric id.")),
                ("slug", opt_str("The form's slug. Give this or `id`.")),
            ],
            &[],
        )
    )]
    async fn get_form(
        &self,
        Parameters(p): Parameters<GetFormParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.authorize(&ctx, Permission::FormsRead)?;
        let model = match (p.id, p.slug.as_deref()) {
            (Some(id), None) => service::find_by_id(&self.db, id).await,
            (None, Some(slug)) => service::find_by_slug(&self.db, slug).await,
            (Some(_), Some(_)) => {
                return Err(McpError::invalid_params(
                    "give `id` or `slug`, not both".to_string(),
                    None,
                ));
            }
            (None, None) => {
                return Err(McpError::invalid_params(
                    "give `id` or `slug`".to_string(),
                    None,
                ));
            }
        }
        .map_err(to_mcp_error)?
        .ok_or_else(|| McpError::invalid_params("form not found".to_string(), None))?;
        let dto = service::dto_from_model(&self.db, model)
            .await
            .map_err(to_mcp_error)?;
        ok_json(&dto)
    }

    #[tool(
        description = "Create a form. Only `name` is required; supply `layout` to set its shape. \
                       The caller becomes the form's owner.",
        input_schema = schema::object(
            vec![("form", schema::schema_ref(core_schema::NEW_FORM))],
            &["form"],
        )
    )]
    async fn create_form(
        &self,
        Parameters(p): Parameters<CreateFormParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let actor = self.authorize(&ctx, Permission::FormsWrite)?;
        let input: NewForm = parse_param("form", p.form)?;
        let registry = self.backends.clone();
        let owner_id = actor.user_id;
        let dto = self
            .db
            .transaction::<_, open_relay_core::forms::FormDto, CoreError>(move |tx| {
                Box::pin(async move {
                    let created = service::create_form(tx, &registry, owner_id, input).await?;
                    service::dto_from_model(tx, created).await
                })
            })
            .await
            .map_err(unwrap_tx)?;
        ok_json(&dto)
    }

    #[tool(
        description = "Update a form. Every field is optional; omitted fields are left alone. \
                       NOTE: sending `layout` REPLACES the whole layout — to change one element, \
                       use add_element/update_element/remove_element/move_element instead.",
        input_schema = schema::object(
            vec![
                ("id", int("The form's numeric id.")),
                ("changes", schema::schema_ref(core_schema::UPDATE_FORM)),
            ],
            &["id", "changes"],
        )
    )]
    async fn update_form(
        &self,
        Parameters(p): Parameters<UpdateFormParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.authorize(&ctx, Permission::FormsWrite)?;
        let input: UpdateForm = parse_param("changes", p.changes)?;
        let registry = self.backends.clone();
        let id = p.id;
        let dto = self
            .db
            .transaction::<_, open_relay_core::forms::FormDto, CoreError>(move |tx| {
                Box::pin(async move {
                    let updated = service::update_form(tx, &registry, id, input).await?;
                    service::dto_from_model(tx, updated).await
                })
            })
            .await
            .map_err(unwrap_tx)?;
        ok_json(&dto)
    }

    #[tool(
        description = "Delete a form permanently, along with its submissions. This cannot be undone.",
        input_schema = schema::object(vec![("id", int("The form's numeric id."))], &["id"])
    )]
    async fn delete_form(
        &self,
        Parameters(p): Parameters<FormIdParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.authorize(&ctx, Permission::FormsDelete)?;
        let id = p.id;
        self.db
            .transaction::<_, (), CoreError>(move |tx| {
                Box::pin(async move { service::delete_form(tx, id).await })
            })
            .await
            .map_err(unwrap_tx)?;
        ok_json(&json!({ "deleted": id }))
    }

    #[tool(
        description = "Insert one element into a form's layout, leaving the rest untouched. \
                       Appends when `at` is omitted.",
        input_schema = schema::object(
            vec![
                ("form_id", int("The form's numeric id.")),
                ("element", schema::schema_ref(core_schema::FORM_ELEMENT)),
                ("at", opt_int("0-based position to insert at. Omit to append.")),
            ],
            &["form_id", "element"],
        )
    )]
    async fn add_element(
        &self,
        Parameters(p): Parameters<AddElementParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.authorize(&ctx, Permission::FormsWrite)?;
        let element: FormElement = parse_param("element", p.element)?;
        let at = p.at;
        let dto = self
            .edit_layout(p.form_id, move |layout| {
                edit::add_element(layout, element, at)
            })
            .await?;
        ok_json(&dto)
    }

    #[tool(
        description = "Replace one element in a form's layout. Address it by `key` (a field's key) \
                       or `index` (for headings, paragraphs, dividers and row markers).",
        input_schema = schema::object(
            vec![
                ("form_id", int("The form's numeric id.")),
                ("key", opt_str("Key of the field to replace.")),
                ("index", opt_int("0-based index of the element to replace.")),
                ("element", schema::schema_ref(core_schema::FORM_ELEMENT)),
            ],
            &["form_id", "element"],
        )
    )]
    async fn update_element(
        &self,
        Parameters(p): Parameters<UpdateElementParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.authorize(&ctx, Permission::FormsWrite)?;
        let target = element_ref(p.key, p.index)?;
        let element: FormElement = parse_param("element", p.element)?;
        let dto = self
            .edit_layout(p.form_id, move |layout| {
                edit::update_element(layout, &target, element)
            })
            .await?;
        ok_json(&dto)
    }

    #[tool(
        description = "Remove one element from a form's layout. Naming either marker of a row \
                       removes the whole row, contents included.",
        input_schema = schema::object(
            vec![
                ("form_id", int("The form's numeric id.")),
                ("key", opt_str("Key of the field to remove.")),
                ("index", opt_int("0-based index of the element to remove.")),
            ],
            &["form_id"],
        )
    )]
    async fn remove_element(
        &self,
        Parameters(p): Parameters<TargetParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.authorize(&ctx, Permission::FormsWrite)?;
        let target = element_ref(p.key, p.index)?;
        let dto = self
            .edit_layout(p.form_id, move |layout| {
                edit::remove_element(layout, &target)
            })
            .await?;
        ok_json(&dto)
    }

    #[tool(
        description = "Move one element to a new position. `to` is read against the layout with \
                       the element removed. Naming either marker of a row moves the whole row.",
        input_schema = schema::object(
            vec![
                ("form_id", int("The form's numeric id.")),
                ("key", opt_str("Key of the field to move.")),
                ("index", opt_int("0-based index of the element to move.")),
                ("to", int("0-based destination index.")),
            ],
            &["form_id", "to"],
        )
    )]
    async fn move_element(
        &self,
        Parameters(p): Parameters<MoveElementParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.authorize(&ctx, Permission::FormsWrite)?;
        let target = element_ref(p.key, p.index)?;
        let to = p.to;
        let dto = self
            .edit_layout(p.form_id, move |layout| {
                edit::move_element(layout, &target, to)
            })
            .await?;
        ok_json(&dto)
    }

    #[tool(
        description = "Set what a visitor sees after submitting: a thank-you message, or a \
                       redirect to an absolute http(s) URL.",
        input_schema = schema::object(
            vec![
                ("form_id", int("The form's numeric id.")),
                ("action", schema::schema_ref(core_schema::POST_SUBMISSION_ACTION)),
            ],
            &["form_id", "action"],
        )
    )]
    async fn set_post_submission_action(
        &self,
        Parameters(p): Parameters<SetActionParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.authorize(&ctx, Permission::FormsWrite)?;
        let action: PostSubmissionAction = parse_param("action", p.action)?;
        self.update_settings(
            p.form_id,
            UpdateForm {
                post_submission_action: Some(action),
                ..Default::default()
            },
        )
        .await
    }

    #[tool(
        description = "Set the multi-step progress indicator (bar, steps, or none). Only visible \
                       on a form whose layout contains page breaks.",
        input_schema = schema::object(
            vec![
                ("form_id", int("The form's numeric id.")),
                ("indicator", schema::schema_ref(core_schema::PROGRESS_INDICATOR)),
            ],
            &["form_id", "indicator"],
        )
    )]
    async fn set_progress_indicator(
        &self,
        Parameters(p): Parameters<SetIndicatorParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.authorize(&ctx, Permission::FormsWrite)?;
        let indicator: ProgressIndicator = parse_param("indicator", p.indicator)?;
        self.update_settings(
            p.form_id,
            UpdateForm {
                progress_indicator: Some(indicator),
                ..Default::default()
            },
        )
        .await
    }

    #[tool(
        description = "The JSON Schema for every layout element type, plus the form create/update \
                       shapes. Read this before building a layout.",
        input_schema = schema::object(vec![], &[])
    )]
    async fn describe_form_schema(
        &self,
        Parameters(_): Parameters<NoParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        // Deliberately unauthenticated: it is a static description of the wire
        // format, identical for every caller and already public in
        // `/openapi.json`. Requiring a permission would only stop an agent from
        // learning what it may ask for.
        ok_json(&schema::all_definitions())
    }
}

impl OpenRelayMcp {
    /// Shared tail for the two whole-form setting tools.
    async fn update_settings(
        &self,
        form_id: i32,
        changes: UpdateForm,
    ) -> Result<CallToolResult, McpError> {
        let registry = self.backends.clone();
        let dto = self
            .db
            .transaction::<_, open_relay_core::forms::FormDto, CoreError>(move |tx| {
                Box::pin(async move {
                    let updated = service::update_form(tx, &registry, form_id, changes).await?;
                    service::dto_from_model(tx, updated).await
                })
            })
            .await
            .map_err(unwrap_tx)?;
        ok_json(&dto)
    }
}

// `router = self.tool_router` is load-bearing, not decoration. Left to its
// default the macro expands to `Self::tool_router()` — i.e. it rebuilds the
// whole router, and with it all twelve input schemas, on *every* `tools/list`
// and `tools/call`. Our schemas embed the form type definitions, so that would
// be a few hundred KB of `serde_json` cloning per request. Pointing it at the
// field built once in `new` makes both calls a lookup.
#[tool_handler(router = self.tool_router)]
impl ServerHandler for OpenRelayMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        // Spelled out rather than `Implementation::from_build_env()`, which
        // reads `CARGO_PKG_*` at its own expansion site inside rmcp and so
        // reports the SDK's name and version as the server's.
        .with_server_info(Implementation::new(
            "open-relay-mcp",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(INSTRUCTIONS)
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                Resource::new(SCHEMA_RESOURCE_URI, "Form layout JSON Schema")
                    .with_description(
                        "Every layout element type, and the form create/update shapes.",
                    )
                    .with_mime_type("application/json"),
            ],
            ..Default::default()
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        if request.uri == SCHEMA_RESOURCE_URI {
            let text = serde_json::to_string_pretty(&schema::all_definitions())
                .map_err(|e| McpError::internal_error(format!("serializing schema: {e}"), None))?;
            return Ok(
                ReadResourceResult::new(vec![ResourceContents::text(text, request.uri)]).into(),
            );
        }
        Err(McpError::resource_not_found(
            "resource not found",
            Some(json!({ "uri": request.uri })),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> Vec<Tool> {
        // The registry is irrelevant to the listing; only the router is read.
        OpenRelayMcp::http(DatabaseConnection::default(), BackendRegistry::new())
            .tool_router
            .list_all()
    }

    #[test]
    fn the_expected_tools_are_published() {
        let mut names: Vec<String> = tools().iter().map(|t| t.name.to_string()).collect();
        names.sort();
        assert_eq!(
            names,
            [
                "add_element",
                "create_form",
                "delete_form",
                "describe_form_schema",
                "get_form",
                "list_forms",
                "move_element",
                "remove_element",
                "set_post_submission_action",
                "set_progress_indicator",
                "update_element",
                "update_form",
            ]
        );
    }

    /// The reason `schema.rs` exists: an agent's client must be able to resolve
    /// every reference in a published tool schema without fetching anything.
    /// A `$ref` that dangles is invisible at runtime — validators generally
    /// ignore what they cannot resolve — so it has to be caught here.
    #[test]
    fn every_published_tool_schema_is_self_contained() {
        fn collect(value: &serde_json::Value, out: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(map) => {
                    for (k, v) in map {
                        if k == "$ref" {
                            if let Some(s) = v.as_str() {
                                out.push(s.to_string());
                            }
                        }
                        collect(v, out);
                    }
                }
                serde_json::Value::Array(a) => a.iter().for_each(|v| collect(v, out)),
                _ => {}
            }
        }

        for tool in tools() {
            let value = serde_json::Value::Object((*tool.input_schema).clone());
            let defs = value
                .get("$defs")
                .and_then(serde_json::Value::as_object)
                .cloned()
                .unwrap_or_default();
            let mut refs = Vec::new();
            collect(&value, &mut refs);
            for r in refs {
                let name = r.strip_prefix("#/$defs/").unwrap_or_else(|| {
                    panic!("tool {}: $ref {r:?} is not document-local", tool.name)
                });
                assert!(
                    defs.contains_key(name),
                    "tool {}: dangling $ref {r:?}",
                    tool.name
                );
            }
        }
    }

    /// Structured parameters must carry the real form schema, not the
    /// `serde_json::Value` placeholder the param struct declares. If the
    /// `input_schema` override were ever dropped, the tool would still work but
    /// the agent would be told "any JSON accepted" — which is how you get a
    /// model guessing at element shapes.
    #[test]
    fn structured_params_expose_the_real_shape() {
        for (tool_name, prop, expected) in [
            ("add_element", "element", "FormElement"),
            ("update_element", "element", "FormElement"),
            ("create_form", "form", "NewForm"),
            ("update_form", "changes", "UpdateForm"),
            ("set_post_submission_action", "action", "PostSubmissionAction"),
            ("set_progress_indicator", "indicator", "ProgressIndicator"),
        ] {
            let tool = tools()
                .into_iter()
                .find(|t| t.name == tool_name)
                .unwrap_or_else(|| panic!("no tool {tool_name}"));
            let value = serde_json::Value::Object((*tool.input_schema).clone());
            let actual = value
                .pointer(&format!("/properties/{prop}/$ref"))
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("{tool_name}.{prop} has no $ref"));
            assert_eq!(actual, format!("#/$defs/{expected}"));
        }
    }

    #[test]
    fn the_server_identifies_itself_not_the_sdk() {
        let info =
            OpenRelayMcp::http(DatabaseConnection::default(), BackendRegistry::new()).get_info();
        assert_eq!(info.server_info.name, "open-relay-mcp");
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn instructions_are_served_at_initialize() {
        let info =
            OpenRelayMcp::http(DatabaseConnection::default(), BackendRegistry::new()).get_info();
        let text = info.instructions.expect("instructions");
        // The invariants an agent cannot infer from a schema.
        assert!(text.contains("BACKWARDS"));
        assert!(text.contains("row_start"));
        assert!(text.contains("page_break"));
    }
}
