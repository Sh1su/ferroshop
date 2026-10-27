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
        bool("active", "active"),
        string("name", "name") [TRANSLATED | REQUIRED | SEARCHABLE],
        json("customFields", "custom_fields") [TRANSLATED],
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_one("language", "language", "languageId"),
        many_to_one("currency", "currency", "currencyId"),
        many_to_one("navigationCategory", "category", "navigationCategoryId"),
    }
}

entity! {
    pub static PRODUCT_MANUFACTURER = "product_manufacturer" in "product_manufacturer"
        translations "product_manufacturer_translation" {
        id("id"),
        string("link", "link"),
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
        datetime("createdAt", "created_at") [READ_ONLY],
        datetime("updatedAt", "updated_at") [READ_ONLY],
        many_to_one("parent", "product", "parentId"),
        many_to_one("tax", "tax", "taxId"),
        many_to_one("manufacturer", "product_manufacturer", "manufacturerId"),
        one_to_many("children", "product", "parentId"),
        many_to_many("categories", "category", "product_category", "product_id", "category_id"),
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
