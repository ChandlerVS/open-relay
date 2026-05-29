use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserDto {
    pub id: i32,
    pub email: String,
    pub display_name: Option<String>,
}

impl From<entity::user::Model> for UserDto {
    fn from(m: entity::user::Model) -> Self {
        Self {
            id: m.id,
            email: m.email,
            display_name: m.display_name,
        }
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct NewUser {
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
}
