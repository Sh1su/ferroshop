use uuid::Uuid;

use crate::defaults;

/// Per-request context: which language to read/write and who is acting.
#[derive(Debug, Clone)]
pub struct Context {
    pub language_id: Uuid,
    /// Resolve inherited fields of variants from their parent (`sw-inheritance` header).
    pub inheritance: bool,
    pub actor_id: Option<Uuid>,
}

impl Default for Context {
    fn default() -> Self {
        Self { language_id: defaults::LANGUAGE_SYSTEM, inheritance: false, actor_id: None }
    }
}
