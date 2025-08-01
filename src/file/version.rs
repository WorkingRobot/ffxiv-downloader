use std::fmt;

use serde::{Deserialize, Serialize};

/// Game version structure that matches FFXIV's versioning scheme
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord)]
pub struct GameVersion {
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub part: i32,
    pub revision: i32,
    pub is_historic: bool,
    pub section: Option<String>,
}

impl GameVersion {
    pub fn new(version_string: &str) -> anyhow::Result<Self> {
        let mut version_string = version_string.to_string();
        let mut is_historic = false;

        if version_string.starts_with('H') {
            is_historic = true;
            version_string = version_string[1..].to_string();
        } else if version_string.starts_with('D') {
            is_historic = false;
            version_string = version_string[1..].to_string();
        }

        let mut section = None;
        while version_string
            .chars()
            .last()
            .map_or(false, |c| c.is_ascii_lowercase())
        {
            let last_char = version_string.pop().unwrap();
            match &mut section {
                Some(s) => *s = format!("{}{}", last_char, s),
                None => section = Some(last_char.to_string()),
            }
        }

        let parts: Vec<&str> = version_string.split('.').collect();
        if parts.len() != 5 {
            return Err(anyhow::anyhow!(
                "Invalid version string: {}",
                version_string
            ));
        }

        Ok(GameVersion {
            year: parts[0].parse()?,
            month: parts[1].parse()?,
            day: parts[2].parse()?,
            part: parts[3].parse()?,
            revision: parts[4].parse()?,
            is_historic,
            section,
        })
    }

    pub fn epoch() -> Self {
        Self::new("2012.01.01.0000.0000").unwrap()
    }
}

impl PartialOrd for GameVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        if self.year != other.year {
            return Some(self.year.cmp(&other.year));
        }
        if self.month != other.month {
            return Some(self.month.cmp(&other.month));
        }
        if self.day != other.day {
            return Some(self.day.cmp(&other.day));
        }
        if self.part != other.part {
            return Some(self.part.cmp(&other.part));
        }
        if self.revision != other.revision {
            return Some(self.revision.cmp(&other.revision));
        }
        if self.is_historic != other.is_historic {
            return Some(self.is_historic.cmp(&other.is_historic).reverse());
        }

        let section = self.section.as_deref().unwrap_or_default();
        let other_section = other.section.as_deref().unwrap_or_default();

        if section.len() != other_section.len() {
            return Some(section.len().cmp(&other_section.len()));
        }

        if section != other_section {
            return Some(section.cmp(other_section));
        }

        Some(std::cmp::Ordering::Equal)
    }
}

impl fmt::Display for GameVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = if self.is_historic { "H" } else { "" };
        let section = self.section.as_deref().unwrap_or("");
        write!(
            f,
            "{}{:04}.{:02}.{:02}.{:04}.{:04}{}",
            prefix, self.year, self.month, self.day, self.part, self.revision, section
        )
    }
}

impl Serialize for GameVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for GameVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let version_string = String::deserialize(deserializer)?;
        GameVersion::new(&version_string).map_err(serde::de::Error::custom)
    }
}

/// Patch version structure that matches FFXIV's patch versioning scheme
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord)]
pub struct PatchVersion {
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub part: i32,
    pub revision: i32,
    pub is_historic: bool,
    pub section: Option<String>,
}

impl PatchVersion {
    pub fn new(version_string: &str) -> anyhow::Result<Self> {
        let mut version_string = version_string.to_string();
        let mut is_historic = false;

        if version_string.starts_with('H') {
            is_historic = true;
            version_string = version_string[1..].to_string();
        } else if version_string.starts_with('D') {
            is_historic = false;
            version_string = version_string[1..].to_string();
        }

        let mut section = None;
        while version_string
            .chars()
            .last()
            .map_or(false, |c| c.is_ascii_lowercase())
        {
            let last_char = version_string.pop().unwrap();
            match &mut section {
                Some(s) => *s = format!("{}{}", last_char, s),
                None => section = Some(last_char.to_string()),
            }
        }

        let parts: Vec<&str> = version_string.split('.').collect();
        if parts.len() != 5 {
            return Err(anyhow::anyhow!(
                "Invalid version string: {}",
                version_string
            ));
        }

        Ok(PatchVersion {
            year: parts[0].parse()?,
            month: parts[1].parse()?,
            day: parts[2].parse()?,
            part: parts[3].parse()?,
            revision: parts[4].parse()?,
            is_historic,
            section,
        })
    }

    pub fn epoch() -> Self {
        Self::new("2012.01.01.0000.0000").unwrap()
    }
}

impl PartialOrd for PatchVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        if self.year != other.year {
            return Some(self.year.cmp(&other.year));
        }
        if self.month != other.month {
            return Some(self.month.cmp(&other.month));
        }
        if self.day != other.day {
            return Some(self.day.cmp(&other.day));
        }
        if self.part != other.part {
            return Some(self.part.cmp(&other.part));
        }
        if self.revision != other.revision {
            return Some(self.revision.cmp(&other.revision));
        }
        if self.is_historic != other.is_historic {
            return Some(self.is_historic.cmp(&other.is_historic).reverse());
        }

        let section = self.section.as_deref().unwrap_or_default();
        let other_section = other.section.as_deref().unwrap_or_default();

        if section.len() != other_section.len() {
            return Some(section.len().cmp(&other_section.len()));
        }

        if section != other_section {
            return Some(section.cmp(other_section));
        }

        Some(std::cmp::Ordering::Equal)
    }
}

impl fmt::Display for PatchVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = if self.is_historic { "H" } else { "D" };
        let section = self.section.as_deref().unwrap_or("");
        write!(
            f,
            "{}{:04}.{:02}.{:02}.{:04}.{:04}{}",
            prefix, self.year, self.month, self.day, self.part, self.revision, section
        )
    }
}
