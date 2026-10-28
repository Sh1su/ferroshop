//! All entity definitions known to ferroshop.

use crate::definition::EntityDefinition;
use crate::entity;

entity! {
    pub static LANGUAGE = "language" in "language" {
        id("id"),
        fk("parentId", "parent_id", "language"),
        string("name", "name") [REQUIRED | SEARCHABLE],
        string("localeCode", "locale_code") [REQUIRED],
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_one("parent", "language", "parentId"),
    }
}

entity! {
    pub static CURRENCY = "currency" in "currency" translations "currency_translation" {
        id("id"),
        string("isoCode", "iso_code") [REQUIRED | SEARCHABLE],
        float("factor", "factor") [REQUIRED],
        string("symbol", "symbol") [REQUIRED],
        int("decimalPrecision", "decimal_precision"),
        int("position", "position"),
        string("shortName", "short_name") [TRANSLATED | REQUIRED],
        string("name", "name") [TRANSLATED | REQUIRED | SEARCHABLE],
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
    }
}

entity! {
    pub static TAX = "tax" in "tax" {
        id("id"),
        float("taxRate", "tax_rate") [REQUIRED],
        string("name", "name") [REQUIRED | SEARCHABLE],
        int("position", "position"),
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        one_to_many("products", "product", "taxId"),
    }
}

entity! {
    pub static CATEGORY = "category" in "category" translations "category_translation" {
        id("id"),
        fk("parentId", "parent_id", "category"),
        int("position", "position"),
        bool("active", "active"),
        bool("visible", "visible"),
        string("type", "type"),
        string("name", "name") [TRANSLATED | REQUIRED | SEARCHABLE],
        text("description", "description") [TRANSLATED],
        string("metaTitle", "meta_title") [TRANSLATED],
        text("metaDescription", "meta_description") [TRANSLATED],
        text("keywords", "keywords") [TRANSLATED],
        json("customFields", "custom_fields") [TRANSLATED],
        computed("childCount", "(SELECT count(*) FROM category cc WHERE cc.parent_id = {a}.id)"),
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_one("parent", "category", "parentId"),
        one_to_many("children", "category", "parentId"),
        many_to_many("products", "product", "product_category", "category_id", "product_id"),
        one_to_many("seoUrls", "seo_url", "foreignKey"),
    }
}

entity! {
    pub static SALES_CHANNEL = "sales_channel" in "sales_channel" translations "sales_channel_translation" {
        id("id"),
        fk("typeId", "type_id", "sales_channel_type"),
        string("accessKey", "access_key") [REQUIRED],
        fk("languageId", "language_id", "language") [REQUIRED],
        fk("currencyId", "currency_id", "currency") [REQUIRED],
        fk("navigationCategoryId", "navigation_category_id", "category"),
        fk("customerGroupId", "customer_group_id", "customer_group"),
        fk("countryId", "country_id", "country"),
        fk("paymentMethodId", "payment_method_id", "payment_method"),
        fk("shippingMethodId", "shipping_method_id", "shipping_method"),
        bool("active", "active"),
        string("name", "name") [TRANSLATED | REQUIRED | SEARCHABLE],
        json("customFields", "custom_fields") [TRANSLATED],
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_one("language", "language", "languageId"),
        many_to_one("currency", "currency", "currencyId"),
        many_to_one("navigationCategory", "category", "navigationCategoryId"),
        many_to_one("customerGroup", "customer_group", "customerGroupId"),
        many_to_one("country", "country", "countryId"),
        many_to_one("paymentMethod", "payment_method", "paymentMethodId"),
        many_to_one("shippingMethod", "shipping_method", "shippingMethodId"),
    }
}

entity! {
    pub static PRODUCT_MANUFACTURER = "product_manufacturer" in "product_manufacturer"
        translations "product_manufacturer_translation" {
        id("id"),
        string("link", "link"),
        fk("mediaId", "media_id", "media"),
        many_to_one("media", "media", "mediaId"),
        string("name", "name") [TRANSLATED | REQUIRED | SEARCHABLE],
        text("description", "description") [TRANSLATED],
        json("customFields", "custom_fields") [TRANSLATED],
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        one_to_many("products", "product", "manufacturerId"),
    }
}

entity! {
    pub static PRODUCT = "product" in "product" translations "product_translation" inherit "parentId" {
        id("id"),
        fk("parentId", "parent_id", "product"),
        string("productNumber", "product_number") [REQUIRED | SEARCHABLE],
        bool("active", "active") [INHERITED],
        int("stock", "stock") [REQUIRED],
        fk("taxId", "tax_id", "tax") [REQUIRED | INHERITED],
        fk("manufacturerId", "manufacturer_id", "product_manufacturer") [INHERITED],
        price("price", "price") [REQUIRED | INHERITED],
        string("ean", "ean") [INHERITED | SEARCHABLE],
        string("manufacturerNumber", "manufacturer_number") [INHERITED | SEARCHABLE],
        float("weight", "weight") [INHERITED],
        float("width", "width") [INHERITED],
        float("height", "height") [INHERITED],
        float("length", "length") [INHERITED],
        int("minPurchase", "min_purchase") [INHERITED],
        int("maxPurchase", "max_purchase") [INHERITED],
        int("purchaseSteps", "purchase_steps") [INHERITED],
        bool("isCloseout", "is_closeout") [INHERITED],
        bool("shippingFree", "shipping_free") [INHERITED],
        bool("markAsTopseller", "mark_as_topseller") [INHERITED],
        datetime("releaseDate", "release_date") [INHERITED],
        string("name", "name") [TRANSLATED | REQUIRED | INHERITED | SEARCHABLE],
        text("description", "description") [TRANSLATED | INHERITED],
        text("keywords", "keywords") [TRANSLATED | INHERITED],
        string("metaTitle", "meta_title") [TRANSLATED | INHERITED],
        text("metaDescription", "meta_description") [TRANSLATED | INHERITED],
        json("customFields", "custom_fields") [TRANSLATED | INHERITED],
        computed("childCount", "(SELECT count(*) FROM product pc WHERE pc.parent_id = {a}.id)"),
        // Lowest gross price in the default currency over the product and its variants;
        // drives price sorting and filtering in Store-API listings.
        computed("cheapestGross", "(SELECT min((p->>'gross')::numeric) FROM product cp, \
            jsonb_array_elements(COALESCE(cp.price, (SELECT pp.price FROM product pp WHERE pp.id = cp.parent_id))) p \
            WHERE (cp.id = {a}.id OR cp.parent_id = {a}.id) AND p->>'currencyId' = 'b7d2554b0ce847cd82f3ac9bd1c0dfca')"),
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_one("parent", "product", "parentId"),
        many_to_one("tax", "tax", "taxId"),
        many_to_one("manufacturer", "product_manufacturer", "manufacturerId"),
        one_to_many("children", "product", "parentId"),
        many_to_many("categories", "category", "product_category", "product_id", "category_id"),
        fk("coverId", "cover_id", "product_media"),
        many_to_one("cover", "product_media", "coverId"),
        one_to_many("media", "product_media", "productId"),
        one_to_many("seoUrls", "seo_url", "foreignKey"),
    }
}

entity! {
    pub static USER = "user" in "admin_user" {
        id("id"),
        string("username", "username") [REQUIRED | SEARCHABLE],
        string("firstName", "first_name") [SEARCHABLE],
        string("lastName", "last_name") [SEARCHABLE],
        string("email", "email") [REQUIRED | SEARCHABLE],
        password("password", "password") [REQUIRED],
        bool("admin", "admin"),
        bool("active", "active"),
        string("localeCode", "locale_code"),
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_many("aclRoles", "acl_role", "acl_user_role", "user_id", "acl_role_id"),
    }
}

entity! {
    pub static ACL_ROLE = "acl_role" in "acl_role" {
        id("id"),
        string("name", "name") [REQUIRED | SEARCHABLE],
        text("description", "description"),
        json("privileges", "privileges"),
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_many("users", "user", "acl_user_role", "acl_role_id", "user_id"),
        many_to_many("integrations", "integration", "integration_role", "acl_role_id", "integration_id"),
    }
}

entity! {
    pub static INTEGRATION = "integration" in "integration" {
        id("id"),
        string("label", "label") [REQUIRED | SEARCHABLE],
        string("accessKey", "access_key") [REQUIRED],
        password("secretAccessKey", "secret_access_key") [REQUIRED],
        bool("admin", "admin"),
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_many("aclRoles", "acl_role", "integration_role", "integration_id", "acl_role_id"),
    }
}

entity! {
    pub static COUNTRY = "country" in "country" translations "country_translation" {
        id("id"),
        string("iso", "iso") [REQUIRED | SEARCHABLE],
        string("iso3", "iso3"),
        bool("active", "active"),
        int("position", "position"),
        string("name", "name") [TRANSLATED | REQUIRED | SEARCHABLE],
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
    }
}

entity! {
    pub static CUSTOMER_GROUP = "customer_group" in "customer_group" translations "customer_group_translation" {
        id("id"),
        bool("displayGross", "display_gross"),
        string("name", "name") [TRANSLATED | REQUIRED | SEARCHABLE],
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
    }
}

entity! {
    pub static PAYMENT_METHOD = "payment_method" in "payment_method" translations "payment_method_translation" {
        id("id"),
        string("technicalName", "technical_name") [REQUIRED | SEARCHABLE],
        bool("active", "active"),
        int("position", "position"),
        string("name", "name") [TRANSLATED | REQUIRED | SEARCHABLE],
        text("description", "description") [TRANSLATED],
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
    }
}

entity! {
    pub static SHIPPING_METHOD = "shipping_method" in "shipping_method" translations "shipping_method_translation" {
        id("id"),
        string("technicalName", "technical_name") [REQUIRED | SEARCHABLE],
        bool("active", "active"),
        int("position", "position"),
        price("price", "price"),
        string("name", "name") [TRANSLATED | REQUIRED | SEARCHABLE],
        text("description", "description") [TRANSLATED],
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
    }
}

entity! {
    pub static CUSTOMER = "customer" in "customer" {
        id("id"),
        string("customerNumber", "customer_number") [SEARCHABLE],
        fk("groupId", "group_id", "customer_group") [REQUIRED],
        fk("salesChannelId", "sales_channel_id", "sales_channel") [REQUIRED],
        fk("languageId", "language_id", "language") [REQUIRED],
        fk("defaultPaymentMethodId", "default_payment_method_id", "payment_method"),
        fk("defaultBillingAddressId", "default_billing_address_id", "customer_address"),
        fk("defaultShippingAddressId", "default_shipping_address_id", "customer_address"),
        string("title", "title"),
        string("firstName", "first_name") [REQUIRED | SEARCHABLE],
        string("lastName", "last_name") [REQUIRED | SEARCHABLE],
        string("company", "company") [SEARCHABLE],
        string("email", "email") [REQUIRED | SEARCHABLE],
        password("password", "password"),
        bool("active", "active"),
        bool("guest", "guest"),
        datetime("birthday", "birthday"),
        datetime("lastLogin", "last_login") [READ_ONLY],
        computed("orderCount", "(SELECT count(*) FROM sales_order so WHERE so.customer_id = {a}.id)"),
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_one("group", "customer_group", "groupId"),
        many_to_one("salesChannel", "sales_channel", "salesChannelId"),
        many_to_one("language", "language", "languageId"),
        many_to_one("defaultPaymentMethod", "payment_method", "defaultPaymentMethodId"),
        many_to_one("defaultBillingAddress", "customer_address", "defaultBillingAddressId"),
        many_to_one("defaultShippingAddress", "customer_address", "defaultShippingAddressId"),
        one_to_many("addresses", "customer_address", "customerId"),
        one_to_many("orders", "order", "customerId"),
    }
}

entity! {
    pub static CUSTOMER_ADDRESS = "customer_address" in "customer_address" {
        id("id"),
        fk("customerId", "customer_id", "customer") [REQUIRED],
        fk("countryId", "country_id", "country") [REQUIRED],
        string("firstName", "first_name") [REQUIRED | SEARCHABLE],
        string("lastName", "last_name") [REQUIRED | SEARCHABLE],
        string("company", "company"),
        string("street", "street") [REQUIRED | SEARCHABLE],
        string("additionalAddressLine1", "additional_address_line1"),
        string("zipcode", "zipcode") [SEARCHABLE],
        string("city", "city") [REQUIRED | SEARCHABLE],
        string("phoneNumber", "phone_number"),
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_one("customer", "customer", "customerId"),
        many_to_one("country", "country", "countryId"),
    }
}

entity! {
    pub static ORDER = "order" in "sales_order" {
        id("id"),
        string("orderNumber", "order_number") [SEARCHABLE],
        datetime("orderDateTime", "order_date_time"),
        fk("salesChannelId", "sales_channel_id", "sales_channel") [REQUIRED],
        fk("currencyId", "currency_id", "currency") [REQUIRED],
        float("currencyFactor", "currency_factor"),
        fk("languageId", "language_id", "language") [REQUIRED],
        fk("customerId", "customer_id", "customer"),
        string("customerNumber", "customer_number") [SEARCHABLE],
        string("customerEmail", "customer_email") [REQUIRED | SEARCHABLE],
        string("customerFirstName", "customer_first_name") [REQUIRED | SEARCHABLE],
        string("customerLastName", "customer_last_name") [REQUIRED | SEARCHABLE],
        string("customerCompany", "customer_company") [SEARCHABLE],
        fk("billingAddressId", "billing_address_id", "order_address"),
        fk("shippingAddressId", "shipping_address_id", "order_address"),
        fk("paymentMethodId", "payment_method_id", "payment_method"),
        fk("shippingMethodId", "shipping_method_id", "shipping_method"),
        float("amountTotal", "amount_total"),
        float("amountNet", "amount_net"),
        float("positionPrice", "position_price"),
        float("shippingTotal", "shipping_total"),
        string("taxStatus", "tax_status"),
        string("state", "state") [READ_ONLY],
        string("paymentState", "payment_state") [READ_ONLY],
        string("deliveryState", "delivery_state") [READ_ONLY],
        text("customerComment", "customer_comment"),
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_one("salesChannel", "sales_channel", "salesChannelId"),
        many_to_one("currency", "currency", "currencyId"),
        many_to_one("language", "language", "languageId"),
        many_to_one("customer", "customer", "customerId"),
        many_to_one("billingAddress", "order_address", "billingAddressId"),
        many_to_one("shippingAddress", "order_address", "shippingAddressId"),
        many_to_one("paymentMethod", "payment_method", "paymentMethodId"),
        many_to_one("shippingMethod", "shipping_method", "shippingMethodId"),
        one_to_many("addresses", "order_address", "orderId"),
        one_to_many("lineItems", "order_line_item", "orderId"),
    }
}

entity! {
    pub static ORDER_ADDRESS = "order_address" in "order_address" {
        id("id"),
        fk("orderId", "order_id", "order") [REQUIRED],
        fk("countryId", "country_id", "country") [REQUIRED],
        string("firstName", "first_name") [REQUIRED],
        string("lastName", "last_name") [REQUIRED],
        string("company", "company"),
        string("street", "street") [REQUIRED],
        string("zipcode", "zipcode"),
        string("city", "city") [REQUIRED],
        string("phoneNumber", "phone_number"),
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_one("country", "country", "countryId"),
    }
}

entity! {
    pub static ORDER_LINE_ITEM = "order_line_item" in "order_line_item" {
        id("id"),
        fk("orderId", "order_id", "order") [REQUIRED],
        fk("productId", "product_id", "product"),
        string("type", "type"),
        string("label", "label") [REQUIRED | SEARCHABLE],
        int("quantity", "quantity") [REQUIRED],
        float("unitPrice", "unit_price") [REQUIRED],
        float("totalPrice", "total_price") [REQUIRED],
        float("taxRate", "tax_rate"),
        int("position", "position"),
        json("payload", "payload"),
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_one("product", "product", "productId"),
    }
}

entity! {
    pub static MEDIA = "media" in "media" translations "media_translation" {
        id("id"),
        string("fileName", "file_name") [READ_ONLY | SEARCHABLE],
        string("fileExtension", "file_extension") [READ_ONLY],
        string("mimeType", "mime_type") [READ_ONLY],
        int("fileSize", "file_size") [READ_ONLY],
        string("path", "path") [READ_ONLY],
        int("width", "width") [READ_ONLY],
        int("height", "height") [READ_ONLY],
        json("thumbnails", "thumbnails") [READ_ONLY],
        datetime("uploadedAt", "uploaded_at") [READ_ONLY],
        computed_text("url", "('/media/' || {a}.path)"),
        string("title", "title") [TRANSLATED | SEARCHABLE],
        string("alt", "alt") [TRANSLATED],
        json("customFields", "custom_fields") [TRANSLATED],
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
    }
}

entity! {
    pub static PRODUCT_MEDIA = "product_media" in "product_media" {
        id("id"),
        fk("productId", "product_id", "product") [REQUIRED],
        fk("mediaId", "media_id", "media") [REQUIRED],
        int("position", "position"),
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_one("media", "media", "mediaId"),
        many_to_one("product", "product", "productId"),
    }
}

entity! {
    pub static SEO_URL = "seo_url" in "seo_url" {
        id("id"),
        fk("salesChannelId", "sales_channel_id", "sales_channel") [REQUIRED],
        fk("languageId", "language_id", "language") [REQUIRED],
        // A category or product id, depending on `routeName`; hence no single target entity.
        fk("foreignKey", "foreign_key", "") [REQUIRED],
        string("routeName", "route_name") [REQUIRED],
        string("pathInfo", "path_info") [REQUIRED],
        string("seoPathInfo", "seo_path_info") [REQUIRED | SEARCHABLE],
        bool("isCanonical", "is_canonical"),
        bool("isModified", "is_modified"),
        bool("isDeleted", "is_deleted"),
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_one("salesChannel", "sales_channel", "salesChannelId"),
        many_to_one("language", "language", "languageId"),
    }
}

pub static ALL: &[&EntityDefinition] = &[
    &LANGUAGE,
    &CURRENCY,
    &TAX,
    &CATEGORY,
    &SALES_CHANNEL,
    &PRODUCT_MANUFACTURER,
    &PRODUCT,
    &USER,
    &ACL_ROLE,
    &INTEGRATION,
    &COUNTRY,
    &CUSTOMER_GROUP,
    &PAYMENT_METHOD,
    &SHIPPING_METHOD,
    &CUSTOMER,
    &CUSTOMER_ADDRESS,
    &ORDER,
    &ORDER_ADDRESS,
    &ORDER_LINE_ITEM,
    &MEDIA,
    &PRODUCT_MEDIA,
    &SEO_URL,
];

/// Looks an entity up by its snake_case or kebab-case name.
pub fn get(name: &str) -> Option<&'static EntityDefinition> {
    let name = name.replace('-', "_");
    ALL.iter().copied().find(|d| d.name == name)
}

pub(crate) fn require(name: &str) -> crate::Result<&'static EntityDefinition> {
    get(name).ok_or_else(|| crate::Error::UnknownEntity(name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::FieldKind;

    /// Every association and fk must point at a registered entity and an existing fk field.
    #[test]
    fn associations_are_consistent() {
        for def in ALL {
            for field in def.fields {
                match field.kind {
                    FieldKind::ManyToOne { entity, fk } => {
                        assert!(get(entity).is_some(), "{}.{}", def.name, field.name);
                        assert!(def.field(fk).is_some(), "{}.{} fk", def.name, field.name);
                    }
                    FieldKind::OneToMany { entity, reference } => {
                        let target = get(entity).expect("target");
                        assert!(target.field(reference).is_some(), "{}.{}", def.name, field.name);
                    }
                    FieldKind::ManyToMany { entity, .. } => {
                        assert!(get(entity).is_some(), "{}.{}", def.name, field.name);
                    }
                    _ => {}
                }
            }
        }
    }
}
