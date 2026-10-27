//! Entity metadata. Every entity is described once with the [`entity!`] macro; reading,
//! writing, criteria compilation and the API schemas are all derived from it.

/// What a field holds and how it is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Primary key.
    Id,
    /// Foreign key to another entity (stored as uuid).
    Fk(&'static str),
    String,
    LongText,
    Int,
    Float,
    Bool,
    DateTime,
    Json,
    /// Shopware price collection: `[{currencyId, gross, net, linked, listPrice?}]`.
    Price,
    /// Argon2-hashed on write, never read.
    Password,
    /// Read-only SQL expression; `{a}` is replaced with the row alias.
    Computed(&'static str),
    ManyToOne {
        entity: &'static str,
        fk: &'static str,
    },
    OneToMany {
        entity: &'static str,
        reference: &'static str,
    },
    ManyToMany {
        entity: &'static str,
        mapping: &'static str,
        local: &'static str,
        reference: &'static str,
    },
}

impl FieldKind {
    pub fn is_association(self) -> bool {
        matches!(self, Self::ManyToOne { .. } | Self::OneToMany { .. } | Self::ManyToMany { .. })
    }

    /// The postgres type used when binding a value for this field.
    pub(crate) fn sql_type(self) -> &'static str {
        match self {
            Self::Id | Self::Fk(_) => "uuid",
            Self::String | Self::LongText | Self::Password => "text",
            Self::Int => "int8",
            Self::Float => "numeric",
            Self::Bool => "bool",
            Self::DateTime => "timestamptz",
            Self::Json | Self::Price => "jsonb",
            Self::Computed(_) => "numeric",
            _ => "text",
        }
    }
}

pub const REQUIRED: u8 = 1;
pub const TRANSLATED: u8 = 2;
pub const INHERITED: u8 = 4;
pub const SEARCHABLE: u8 = 8;
pub const READ_ONLY: u8 = 16;

#[derive(Debug, Clone, Copy)]
pub struct Field {
    /// camelCase name used in the API.
    pub name: &'static str,
    /// Column in the entity (or translation) table.
    pub column: &'static str,
    pub kind: FieldKind,
    pub flags: u8,
}

impl Field {
    pub const fn new(name: &'static str, column: &'static str, kind: FieldKind) -> Self {
        Self { name, column, kind, flags: 0 }
    }

    pub const fn flag(mut self, flag: u8) -> Self {
        self.flags |= flag;
        self
    }

    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    pub fn translated(&self) -> bool {
        self.has(TRANSLATED)
    }

    pub fn required(&self) -> bool {
        self.has(REQUIRED)
    }

    /// Stored as a column of the entity table itself.
    pub fn is_own_column(&self) -> bool {
        !self.translated() && !self.kind.is_association() && !matches!(self.kind, FieldKind::Computed(_))
    }
}

#[derive(Debug)]
pub struct EntityDefinition {
    /// snake_case entity name, e.g. `product_manufacturer`.
    pub name: &'static str,
    pub table: &'static str,
    /// Translation table; its foreign key column is `<name>_id`.
    pub translation_table: Option<&'static str>,
    /// Fk field that points at the parent for inheritance (`parentId` on products).
    pub inherit_from: Option<&'static str>,
    pub fields: &'static [Field],
}

impl EntityDefinition {
    pub fn field(&self, name: &str) -> Option<&'static Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn translation_fk(&self) -> String {
        format!("{}_id", self.name)
    }

    /// The API name used in URLs: `product-manufacturer`.
    pub fn api_name(&self) -> String {
        self.name.replace('_', "-")
    }

    pub fn translated_fields(&self) -> impl Iterator<Item = &'static Field> {
        self.fields.iter().filter(|f| f.translated())
    }
}

/// Declares an entity definition as a `static`.
///
/// ```ignore
/// entity! {
///     pub static TAX = "tax" in "tax" {
///         id("id"),
///         float("taxRate", "tax_rate") [REQUIRED],
///     }
/// }
/// ```
#[macro_export]
macro_rules! entity {
    (
        $vis:vis static $ident:ident = $name:literal in $table:literal
        $(translations $tr:literal)?
        $(inherit $inherit:literal)?
        { $( $kind:ident ( $($arg:expr),* ) $([ $($flag:ident)|+ ])? ),* $(,)? }
    ) => {
        $vis static $ident: $crate::definition::EntityDefinition = $crate::definition::EntityDefinition {
            name: $name,
            table: $table,
            translation_table: $crate::entity!(@opt $($tr)?),
            inherit_from: $crate::entity!(@opt $($inherit)?),
            fields: &[
                $(
                    $crate::entity!(@field $kind ( $($arg),* ))
                        .flag(0 $($(| $crate::definition::$flag)+)?)
                ),*
            ],
        };
    };
    (@opt) => { None };
    (@opt $v:literal) => { Some($v) };

    (@field id ($col:expr)) => { $crate::definition::Field::new("id", $col, $crate::definition::FieldKind::Id) };
    (@field fk ($name:expr, $col:expr, $target:expr)) => { $crate::definition::Field::new($name, $col, $crate::definition::FieldKind::Fk($target)) };
    (@field string ($name:expr, $col:expr)) => { $crate::definition::Field::new($name, $col, $crate::definition::FieldKind::String) };
    (@field text ($name:expr, $col:expr)) => { $crate::definition::Field::new($name, $col, $crate::definition::FieldKind::LongText) };
    (@field int ($name:expr, $col:expr)) => { $crate::definition::Field::new($name, $col, $crate::definition::FieldKind::Int) };
    (@field float ($name:expr, $col:expr)) => { $crate::definition::Field::new($name, $col, $crate::definition::FieldKind::Float) };
    (@field bool ($name:expr, $col:expr)) => { $crate::definition::Field::new($name, $col, $crate::definition::FieldKind::Bool) };
    (@field datetime ($name:expr, $col:expr)) => { $crate::definition::Field::new($name, $col, $crate::definition::FieldKind::DateTime) };
    (@field json ($name:expr, $col:expr)) => { $crate::definition::Field::new($name, $col, $crate::definition::FieldKind::Json) };
    (@field price ($name:expr, $col:expr)) => { $crate::definition::Field::new($name, $col, $crate::definition::FieldKind::Price) };
    (@field password ($name:expr, $col:expr)) => { $crate::definition::Field::new($name, $col, $crate::definition::FieldKind::Password) };
    (@field computed ($name:expr, $sql:expr)) => { $crate::definition::Field::new($name, "", $crate::definition::FieldKind::Computed($sql)) };
    (@field many_to_one ($name:expr, $target:expr, $fk:expr)) => {
        $crate::definition::Field::new($name, "", $crate::definition::FieldKind::ManyToOne { entity: $target, fk: $fk })
    };
    (@field one_to_many ($name:expr, $target:expr, $reference:expr)) => {
        $crate::definition::Field::new($name, "", $crate::definition::FieldKind::OneToMany { entity: $target, reference: $reference })
    };
    (@field many_to_many ($name:expr, $target:expr, $mapping:expr, $local:expr, $reference:expr)) => {
        $crate::definition::Field::new($name, "", $crate::definition::FieldKind::ManyToMany {
            entity: $target, mapping: $mapping, local: $local, reference: $reference,
        })
    };
}
