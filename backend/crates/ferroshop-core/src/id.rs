//! Shopware-compatible ids: UUIDs rendered as 32 lowercase hex chars without dashes.

use uuid::Uuid;

/// Generates a new time-ordered id.
pub fn new_id() -> Uuid {
    Uuid::now_v7()
}

/// Formats an id the way the API emits it (`0190a1b2...`, 32 chars).
pub fn format(id: Uuid) -> String {
    id.simple().to_string()
}

/// Parses ids in both the Shopware (32 hex) and the dashed format.
pub fn parse(value: &str) -> Option<Uuid> {
    Uuid::try_parse(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let id = new_id();
        let hex = format(id);
        assert_eq!(hex.len(), 32);
        assert_eq!(parse(&hex), Some(id));
        assert_eq!(parse(&id.to_string()), Some(id));
        assert_eq!(parse("nope"), None);
    }
}
