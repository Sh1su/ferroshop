//! Well-known ids. The system language, currency and live version match Shopware's `Defaults`.

use uuid::{Uuid, uuid};

pub const LANGUAGE_SYSTEM: Uuid = uuid!("2fbb5fe2e29a4d70aa5854ce7ce3e20b");
pub const LANGUAGE_DE: Uuid = uuid!("0191a6f0f4a07c7e9b1c2d3e4f5a6b7c");
pub const CURRENCY: Uuid = uuid!("b7d2554b0ce847cd82f3ac9bd1c0dfca");
pub const LIVE_VERSION: Uuid = uuid!("0fa91ce3e96a4bc2be4bd9ce752c3425");
pub const SALES_CHANNEL_TYPE_STOREFRONT: Uuid = uuid!("8a243080f92e4c719546314b577cf82b");
pub const ROOT_CATEGORY: Uuid = uuid!("0191a6f0f4a07c7e9b1c2d3e4f5a6c01");
pub const TAX_STANDARD: Uuid = uuid!("0191a6f0f4a07c7e9b1c2d3e4f5a6b01");
