//! The macros the values are made with: ids, text, and text naming one of a few.
/// Ids are UUIDv7, so they sort by creation time.
macro_rules! id_type {
    ($($name:ident),*) => {$(
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
        #[serde(try_from = "Uuid", into = "Uuid")]
        #[ts(export, type = "string")]
        pub struct $name(pub Uuid);

        impl TryFrom<Uuid> for $name {
            type Error = &'static str;

            fn try_from(id: Uuid) -> Result<Self, Self::Error> {
                if id.get_version_num() == 7 && id.get_variant() == uuid::Variant::RFC4122 { Ok(Self(id)) } else { Err("ID 必须是 UUIDv7") }
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Uuid {
                id.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        #[cfg(feature = "sql")]
        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.0.to_string().into())
            }
        }

        #[cfg(feature = "sql")]
        impl rusqlite::types::FromSql for $name {
            fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                Uuid::parse_str(v.as_str()?).map(Self).map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
            }
        }
    )*};
}

/// Validated text stored as is. Values read back from the database were validated when written.
macro_rules! text_type {
    ($(#[$doc:meta])* $name:ident, $check:expr) => {
        $(#[$doc])*
        #[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
        #[serde(try_from = "String", into = "String")]
        #[ts(export, type = "string")]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(s: String) -> Result<Self, String> {
                let check: fn(String) -> Result<String, String> = $check;
                check(s).map(Self)
            }
        }

        impl From<$name> for String {
            fn from(v: $name) -> String {
                v.0
            }
        }

        #[cfg(feature = "sql")]
        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.0.as_str().into())
            }
        }

        #[cfg(feature = "sql")]
        impl rusqlite::types::FromSql for $name {
            fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                Ok(Self(v.as_str()?.to_owned()))
            }
        }
    };
}

/// An enum stored as the text it has in JSON, such as `waiting` or `day`.
macro_rules! sql_text_enum {
    ($name:ident { $($variant:ident = $text:literal),* }) => {
        impl $name {
            pub const ALL: &'static [Self] = &[$($name::$variant),*];

            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),*
                }
            }
        }

        #[cfg(feature = "sql")]
        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.as_str().into())
            }
        }

        #[cfg(feature = "sql")]
        impl rusqlite::types::FromSql for $name {
            fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                match v.as_str()? {
                    $($text => Ok($name::$variant),)*
                    _ => Err(rusqlite::types::FromSqlError::InvalidType),
                }
            }
        }
    };
}

/// An enum stored and sent as text, each value as given.
macro_rules! text_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident = $text:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[ts(export)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),*
        }

        sql_text_enum!($name { $($variant = $text),* });
    };
}
