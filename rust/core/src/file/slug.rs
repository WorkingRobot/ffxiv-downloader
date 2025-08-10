use std::{
    fmt::{self, Display},
    str::FromStr,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Slug(u32);

impl FromStr for Slug {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let slug = u32::from_str_radix(s, 16).context(format!("Invalid slug format: {s}"))?;
        Ok(Slug(slug))
    }
}

impl Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}
