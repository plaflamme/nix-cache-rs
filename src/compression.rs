use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Zstd,
    Xz,
}

impl Compression {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::None => "",
            Self::Zstd => "zst",
            Self::Xz => "xz",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Zstd => "zstd",
            Self::Xz => "xz",
        }
    }
}

impl FromStr for Compression {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Self::None),
            "zstd" | "zst" => Ok(Self::Zstd),
            "xz" => Ok(Self::Xz),
            other => Err(crate::Error::Validation {
                field: "Compression",
                message: format!("unsupported compression {other}"),
            }),
        }
    }
}

impl std::fmt::Display for Compression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl<'de> Deserialize<'de> for Compression {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Serialize for Compression {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.name().serialize(serializer)
    }
}
