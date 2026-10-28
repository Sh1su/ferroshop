//! ferroshop core: entity definitions, criteria and the generic data layer.

pub mod acl;
pub mod context;
pub mod criteria;
pub mod defaults;
pub mod definition;
pub mod entities;
pub mod error;
pub mod id;
pub mod media;
pub mod order_state;
pub mod password;
pub mod price;
pub mod read;
pub mod schema;
pub mod seo;
mod sql;
pub mod variants;
pub mod write;

pub use context::Context;
pub use criteria::Criteria;
pub use definition::{EntityDefinition, Field, FieldKind};
pub use error::{Error, Result, WriteError};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub type Db = sqlx::PgPool;
