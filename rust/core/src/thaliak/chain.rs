use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::{Context, Result, ensure};
use reqwest::Client;

use crate::file::version::{GameVersion, PatchVersion};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    pub url: String,
    pub size: i64,
}

impl Patch {
    pub fn version(&self) -> Result<PatchVersion> {
        let name = self
            .url
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        ensure!(!stem.is_empty(), "no version in patch url {}", self.url);
        PatchVersion::new(stem)
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    pub version: GameVersion,
    pub is_active: bool,
    pub prerequisites: Vec<GameVersion>,
    pub patches: Vec<Patch>,
}

/// `[(slug, [(version, replacement or end-of-chain)])]`.
type OverrideTable = &'static [(
    &'static str,
    &'static [(&'static str, Option<&'static str>)],
)];

/// `[(slug, [(version, patch url, patch size)])]`.
type ExtraTable = &'static [(&'static str, &'static [(&'static str, &'static str, i64)])];

/// Thaliak's prerequisite edges are wrong or absent for these versions. The value
/// replaces whichever predecessor the graph would otherwise pick, and `None` ends the
/// chain there.
const OVERRIDES: OverrideTable = &[
    // Global
    (
        "4e9a232b",
        &[
            // Thaliak lists no prerequisite at all for these three, which is provably wrong: every
            // one of their patches is an `FHDR`/`DIFF`, and a differential patch has something to
            // apply on top of. Only four of the repository's 448 versions state none, and the
            // fourth is the 2024.05.31 entry below, so this is the same defect recurring. Left
            // unpatched, the forest for each is a single node and the CLUT built from it indexes
            // that one delta: 8 KB and about 160 paths against the 7 MB a whole install takes.
            ("2026.08.19.0000.0000", Some("2026.08.11.0000.0000")),
            ("2026.08.31.0000.0000", Some("2026.08.19.0000.0000")),
            ("2026.09.01.0000.0000", Some("2026.08.31.0000.0000")),
            // Thaliak incorrectly orders these hist patches.
            // aa comes after z. It's not lexicographically sorted.
            ("2024.05.31.0000.0000", Some("H2024.05.31.0000.0000ag")),
            ("H2024.05.31.0000.0000b", Some("H2024.05.31.0000.0000a")),
            ("H2024.05.31.0000.0000aa", Some("H2024.05.31.0000.0000z")),
            // Spooky unseen patches o.o
            ("2024.04.23.0000.0000", Some("2024.04.22.0000.0001")),
            ("2024.04.22.0000.0001", Some("2024.03.27.0000.0000")),
            ("2023.06.14.0000.0000", Some("2023.06.13.0000.0001")),
            ("2023.06.13.0000.0001", Some("2023.05.11.0000.0001")),
            ("2017.06.06.0000.0001", Some("H2017.06.06.0000.0001m")),
            ("H2017.06.06.0000.0001a", None),
            // Thaliak's record of this repository begins at a HIST reset that discarded
            // everything before it, and half of what it does list no longer downloads. These
            // are the surviving patches, recovered by sweeping the CDN; the chain each one
            // names was verified by folding it into a CLUT and checking that every entry the
            // sqpack indexes point at is supplied.
            ("2013.06.29.0000.0000", None),
            ("2013.06.29.0001.0000", Some("2013.06.29.0000.0000")),
            ("2013.06.29.0002.0000", Some("2013.06.29.0001.0000")),
            ("2013.06.29.0003.0000", Some("2013.06.29.0002.0000")),
            ("2013.06.29.0004.0000", Some("2013.06.29.0003.0000")),
            ("2013.08.09.0000.0000", Some("2013.06.29.0004.0000")),
            ("2013.08.09.0001.0000", Some("2013.08.09.0000.0000")),
            ("2013.11.26.0000.0002", Some("2013.08.09.0001.0000")),
            ("2013.12.05.0000.0000", Some("2013.11.26.0000.0002")),
            ("2013.12.05.0001.0000", Some("2013.12.05.0000.0000")),
            ("2014.03.06.0000.0002", Some("2013.12.05.0001.0000")),
            ("2014.05.12.0000.0002", Some("2014.03.06.0000.0002")),
            // The from-scratch install of this version, which every later version roots at
            // instead of replaying the line before it. Its indexes come out byte-identical
            // to the delta route's.
            ("2014.05.27.0000.0000", Some("H2014.05.27.0000.0000g")),
            ("H2014.05.27.0000.0000g", Some("H2014.05.27.0000.0000f")),
            ("H2014.05.27.0000.0000f", Some("H2014.05.27.0000.0000e")),
            ("H2014.05.27.0000.0000e", Some("H2014.05.27.0000.0000d")),
            ("H2014.05.27.0000.0000d", Some("H2014.05.27.0000.0000c")),
            ("H2014.05.27.0000.0000c", Some("H2014.05.27.0000.0000b")),
            ("H2014.05.27.0000.0000b", Some("H2014.05.27.0000.0000a")),
            ("H2014.05.27.0000.0000a", None),
            ("2014.06.21.0000.0001", Some("2014.05.27.0000.0000")),
            ("2014.06.25.0000.0000", Some("2014.06.21.0000.0001")),
            ("2014.06.25.0001.0000", Some("2014.06.25.0000.0000")),
            ("2014.10.03.0000.0002", Some("2014.06.25.0001.0000")),
            ("2014.10.17.0000.0000", Some("2014.10.03.0000.0002")),
            ("2014.10.17.0001.0000", Some("2014.10.17.0000.0000")),
            ("2015.02.10.0000.0001", Some("2014.10.17.0001.0000")),
            ("2015.05.20.0000.0002", Some("2015.02.10.0000.0001")),
            ("2015.05.26.0000.0000", Some("2015.05.20.0000.0002")),
            ("2015.05.26.0001.0000", Some("2015.05.26.0000.0000")),
            ("2015.05.26.0002.0000", Some("2015.05.26.0001.0000")),
            ("2015.07.25.0000.0002", Some("2015.05.26.0002.0000")),
            ("2015.10.27.0000.0001", Some("2015.07.25.0000.0002")),
            ("2015.11.14.0000.0001", Some("2015.10.27.0000.0001")),
            ("2015.11.28.0000.0001", Some("2015.11.14.0000.0001")),
            ("2016.02.08.0000.0001", Some("2015.11.28.0000.0001")),
            ("2016.03.16.0000.0001", Some("2016.02.08.0000.0001")),
            ("2016.03.25.0000.0001", Some("2016.03.16.0000.0001")),
            ("2016.05.21.0000.0001", Some("2016.03.25.0000.0001")),
            ("2016.07.05.0000.0001", Some("2016.05.21.0000.0001")),
            ("2016.07.21.0000.0001", Some("2016.07.05.0000.0001")),
            ("2016.07.22.0000.0000", Some("2016.07.21.0000.0001")),
            ("2016.08.07.0000.0001", Some("2016.07.22.0000.0000")),
            ("2016.08.08.0000.0000", Some("2016.08.07.0000.0001")),
            ("2016.09.10.0000.0000", Some("2016.08.08.0000.0000")),
            ("2016.09.20.0000.0001", Some("2016.09.10.0000.0000")),
            ("2016.09.21.0000.0000", Some("2016.09.20.0000.0001")),
            ("2016.10.02.0000.0001", Some("2016.09.21.0000.0000")),
            ("2016.10.03.0000.0000", Some("2016.10.02.0000.0001")),
            ("2016.10.10.0000.0001", Some("2016.10.03.0000.0000")),
            ("2016.10.11.0000.0000", Some("2016.10.10.0000.0001")),
            ("2016.10.21.0000.0000", Some("2016.10.11.0000.0000")),
            ("2016.10.25.0000.0001", Some("2016.10.21.0000.0000")),
            ("2016.10.26.0000.0000", Some("2016.10.25.0000.0001")),
            ("2016.11.10.0000.0000", Some("2016.10.26.0000.0000")),
            ("2016.11.11.0000.0000", Some("2016.11.10.0000.0000")),
            ("2016.12.08.0000.0000", Some("2016.11.11.0000.0000")),
            ("2016.12.17.0000.0000", Some("2016.12.08.0000.0000")),
            ("2017.01.10.0000.0001", Some("2016.12.17.0000.0000")),
            ("2017.01.11.0000.0000", Some("2017.01.10.0000.0001")),
            ("2017.01.24.0000.0000", Some("2017.01.11.0000.0000")),
            ("2017.01.31.0000.0000", Some("2017.01.24.0000.0000")),
            ("2017.02.09.0000.0000", Some("2017.01.31.0000.0000")),
            ("2017.02.14.0000.0001", Some("2017.02.09.0000.0000")),
            ("2017.02.22.0000.0001", Some("2017.02.14.0000.0001")),
            ("2017.02.22.0001.0000", Some("2017.02.22.0000.0001")),
            ("2017.03.03.0000.0000", Some("2017.02.22.0001.0000")),
            ("2017.03.04.0000.0000", Some("2017.03.03.0000.0000")),
            ("2017.03.12.0000.0001", Some("2017.03.04.0000.0000")),
            ("2017.03.13.0000.0000", Some("2017.03.12.0000.0001")),
            ("2017.03.17.0000.0000", Some("2017.03.13.0000.0000")),
            ("2017.03.22.0000.0001", Some("2017.03.17.0000.0000")),
            ("2017.03.23.0000.0000", Some("2017.03.22.0000.0001")),
            ("2017.04.13.0000.0001", Some("2017.03.23.0000.0000")),
            ("2017.05.26.0000.0000", Some("2017.04.13.0000.0001")),
            ("2017.05.26.0001.0000", Some("2017.05.26.0000.0000")),
        ],
    ),
    (
        "6b936f08",
        &[
            ("2024.05.31.0000.0000", Some("H2024.05.31.0000.0000d")),
            // Thaliak points this at the reset install's first part, so a chain through it
            // applies the first and last and skips b and c.
            ("2017.06.01.0000.0001", Some("H2017.06.01.0000.0001c")),
            // Thaliak's record of this repository begins at a HIST reset that discarded
            // everything before it, and half of what it does list no longer downloads. These
            // are the surviving patches, recovered by sweeping the CDN; the chain each one
            // names was verified by folding it into a CLUT and checking that every entry the
            // sqpack indexes point at is supplied.
            ("2015.03.16.0000.0000", None),
            ("2015.05.26.0000.0000", Some("2015.03.16.0000.0000")),
            ("2015.05.26.0001.0000", Some("2015.05.26.0000.0000")),
            ("2015.07.03.0000.0002", Some("2015.05.26.0001.0000")),
            ("2015.08.20.0000.0001", Some("2015.07.03.0000.0002")),
            // The from-scratch install of this version, which every later version roots at
            // instead of replaying the line before it. Its indexes come out byte-identical
            // to the delta route's.
            ("2015.10.27.0000.0000", Some("H2015.10.27.0000.0000b")),
            ("H2015.10.27.0000.0000b", Some("H2015.10.27.0000.0000a")),
            ("H2015.10.27.0000.0000a", None),
            ("2016.02.08.0000.0001", Some("2015.10.27.0000.0000")),
            ("2016.05.21.0000.0001", Some("2016.02.08.0000.0001")),
            ("2016.09.10.0000.0001", Some("2016.05.21.0000.0001")),
            ("2016.12.17.0000.0001", Some("2016.09.10.0000.0001")),
                    // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2022.03.25.0000.0000", Some("2021.11.21.0000.0001")),
            ("2022.03.27.0000.0000", Some("2022.03.25.0000.0000")),
        ],
    ),
    (
        "f29a3eb2",
        &[
            ("2024.05.31.0000.0000", Some("H2024.05.31.0000.0000e")),
            // Thaliak's record of this repository begins at a HIST reset that discarded
            // everything before it, and half of what it does list no longer downloads. These
            // are the surviving patches, recovered by sweeping the CDN; the chain each one
            // names was verified by folding it into a CLUT and checking that every entry the
            // sqpack indexes point at is supplied.
            ("2017.03.18.0000.0000", None),
            ("2017.05.26.0000.0000", Some("2017.03.18.0000.0000")),
            ("2017.05.26.0001.0000", Some("2017.05.26.0000.0000")),
            ("2017.05.26.0002.0000", Some("2017.05.26.0001.0000")),
            ("2017.06.27.0000.0001", Some("2017.05.26.0002.0000")),
            ("2017.09.24.0000.0001", Some("2017.06.27.0000.0001")),
            ("2018.01.12.0000.0001", Some("2017.09.24.0000.0001")),
            ("2018.02.23.0000.0001", Some("2018.01.12.0000.0001")),
            ("2018.04.27.0000.0001", Some("2018.02.23.0000.0001")),
            ("2018.07.18.0000.0001", Some("2018.04.27.0000.0001")),
            ("2018.09.05.0000.0001", Some("2018.07.18.0000.0001")),
            ("2018.12.14.0000.0001", Some("2018.09.05.0000.0001")),
            ("2019.01.26.0000.0001", Some("2018.12.14.0000.0001")),
            ("2019.03.12.0000.0001", Some("2019.01.26.0000.0001")),
            ("2019.05.29.0000.0001", Some("2019.03.12.0000.0001")),
            ("2019.12.04.0000.0001", Some("2019.05.29.0000.0001")),
            ("2020.09.17.0000.0001", Some("2019.12.04.0000.0001")),
            ("2021.01.15.0000.0001", Some("2020.09.17.0000.0001")),
            ("2021.11.17.0000.0001", Some("2021.01.15.0000.0001")),
            ("2022.05.26.0000.0001", Some("2021.11.17.0000.0001")),
            ("2022.08.16.0000.0001", Some("2022.05.26.0000.0001")),
            ("2022.12.23.0000.0001", Some("2022.08.16.0000.0001")),
            ("2023.04.28.0000.0001", Some("2022.12.23.0000.0001")),
            ("2023.12.12.0000.0000", Some("2023.04.28.0000.0001")),
            ("2024.04.23.0000.0001", Some("2023.12.12.0000.0000")),
            ("2024.05.25.0000.0000", Some("2024.04.23.0000.0001")),
            ("2024.05.25.0001.0000", Some("2024.05.25.0000.0000")),
            ("2024.05.25.0002.0000", Some("2024.05.25.0001.0000")),
            ("2024.05.25.0003.0000", Some("2024.05.25.0002.0000")),
            ("2024.05.30.0000.0001", Some("2024.05.25.0003.0000")),
                    // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2021.12.14.0000.0001", Some("2021.11.17.0000.0001")),
            ("2022.03.25.0000.0000", Some("2021.12.14.0000.0001")),
        ],
    ),
    (
        "859d0e24",
        &[
            ("2024.05.31.0000.0000", Some("H2024.05.31.0000.0000g")),
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2022.03.27.0000.0000", Some("2022.03.25.0000.0001")),
            ("2022.03.25.0000.0001", Some("2021.11.21.0000.0001")),
            ("2019.04.02.0000.0000", None),  // install
        ],
    ),
    (
        "1bf99b87",
        &[
            ("2024.05.31.0000.0000", Some("H2024.05.31.0000.0000i")),
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2022.04.02.0000.0000", Some("2022.04.01.0000.0001")),
            ("2022.04.19.0000.0000", Some("2022.04.02.0000.0000")),
            ("2022.04.01.0000.0001", Some("2022.03.25.0001.0000")),
            ("2021.08.23.0000.0000", None),  // install
        ],
    ),
    // Korea
    (
        "de199059",
        &[
            ("2024.11.02.0000.0000", Some("H2024.11.02.0000.0000ad")),
            ("H2024.11.02.0000.0000b", Some("H2024.11.02.0000.0000a")),
            ("H2024.11.02.0000.0000aa", Some("H2024.11.02.0000.0000z")),
                    // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2022.04.18.0000.0001", Some("2022.04.11.0000.0001")),
            ("2021.04.29.0000.0001", Some("2021.04.20.0000.0001")),
            ("2021.04.30.0000.0000", None),  // install
            ("2021.05.22.0000.0000", Some("2021.04.30.0000.0000")),
            ("2022.04.19.0000.0000", None),  // install
            ("2015.06.08.0000.0000", None),  // install
        ],
    ),
    (
        "573d8c07",
        &[
            ("2024.10.22.0002.0000", Some("H2024.10.22.0002.0000c")),
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2016.04.05.0000.0000", None),  // install
        ],
    ),
    (
        "ce34ddbd",
        &[
            ("2024.10.22.0003.0000", Some("H2024.10.22.0003.0000e")),
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2017.09.04.0000.0000", None),  // install
        ],
    ),
    (
        "b933ed2b",
        &[
            ("2024.11.02.0000.0000", Some("H2024.11.02.0000.0000f")),
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2024.11.19.0000.0000", Some("2024.11.08.0000.0000")),
            ("2019.08.21.0000.0000", None),  // install
            ("2024.11.07.0000.0001", Some("H2024.11.02.0000.0000f")),
            ("2024.11.08.0000.0000", Some("2024.11.07.0000.0001")),
        ],
    ),
    (
        "27577888",
        &[
            ("2024.11.02.0000.0000", Some("H2024.11.02.0000.0000g")),
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2022.04.09.0000.0000", Some("2022.04.08.0003.0000")),
            ("2022.04.12.0000.0000", Some("2022.04.09.0000.0000")),
            ("2024.11.07.0000.0001", Some("H2024.11.02.0000.0000g")),
            ("2024.11.08.0000.0000", Some("2024.11.07.0000.0001")),
            ("2024.11.19.0000.0000", Some("2024.11.08.0000.0000")),
            ("2022.01.19.0000.0000", None),  // install
        ],
    ),
    // China
    (
        "c38effbc",
        &[
            ("2024.09.09.0000.0000", Some("H2024.09.09.0000.0000ad")),
            ("H2024.09.09.0000.0000b", Some("H2024.09.09.0000.0000a")),
            ("H2024.09.09.0000.0000aa", Some("H2024.09.09.0000.0000z")),
                    // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2014.03.24.0000.0000", None),
            ("2024.09.14.0000.0001", Some("H2024.09.09.0000.0000ad")),
            ("2024.09.16.0000.0000", Some("2024.09.14.0000.0001")),
        ],
    ),
    (
        "77420d17",
        &[
            ("2024.08.27.0002.0000", Some("H2024.08.27.0002.0000c")),
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2015.09.04.0000.0000", None),  // install
        ],
    ),
    (
        "ee4b5cad",
        &[
            ("2024.08.27.0003.0000", Some("H2024.08.27.0003.0000e")),
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2017.06.20.0000.0000", None),  // install
        ],
    ),
    (
        "994c6c3b",
        &[
            ("2024.09.09.0000.0000", Some("H2024.09.09.0000.0000f")),
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2019.07.31.0000.0000", None),  // install
            ("2024.09.16.0000.0000", Some("2024.09.14.0000.0001")),
            ("2024.09.17.0000.0000", Some("2024.09.16.0000.0000")),
            ("2024.10.08.0000.0001", Some("2024.09.17.0000.0000")),
            ("2024.10.09.0000.0000", Some("2024.10.08.0000.0001")),
            ("2024.09.14.0000.0001", Some("H2024.09.09.0000.0000f")),
            ("2024.10.10.0000.0000", Some("2024.10.09.0000.0000")),
        ],
    ),
    (
        "0728f998",
        &[
            ("2024.09.09.0000.0000", Some("H2024.09.09.0000.0000g")),
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2022.01.19.0000.0000", None),  // install
            ("2024.09.14.0000.0001", Some("H2024.09.09.0000.0000g")),
            ("2024.09.16.0000.0000", Some("2024.09.14.0000.0001")),
            ("2024.09.17.0000.0000", Some("2024.09.16.0000.0000")),
            ("2024.10.08.0000.0001", Some("2024.09.17.0000.0000")),
            ("2024.10.09.0000.0000", Some("2024.10.08.0000.0001")),
            ("2024.10.10.0000.0000", Some("2024.10.09.0000.0000")),
        ],
    ),
    (
        "2b5cbc63",
        &[
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2013.06.18.0000.0000", None),  // install
        ],
    ),
    (
        "5050481e",
        &[
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2024.10.09.0000.0000", None),  // install
        ],
    ),
    (
        "6cfeab11",
        &[
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2024.05.20.0000.0000", None),  // install
        ],
    ),
    (
        "702fc90e",
        &[
            // Thaliak states no prerequisite for these. Those marked install are full
            // installs, confirmed by an FHDR with no add commands and only file writes, so
            // they genuinely start a lineage; the rest are deltas whose edge was dropped and
            // whose predecessor is the version immediately before them.
            ("2024.08.19.0000.0000", None),  // install
        ],
    ),
];

/// Versions Thaliak never listed, but that an overridden edge points at.
const EXTRA_VERSIONS: ExtraTable = &[(
    "4e9a232b",
    &[
        (
            "2023.06.13.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2023.06.13.0000.0001.patch",
            89_863_002,
        ),
        (
            "2024.04.22.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2024.04.22.0000.0001.patch",
            16_909_460,
        ),
        (
            "2013.06.29.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2013.06.29.0000.0000.patch",
            1_498_700_671,
        ),
        (
            "2013.06.29.0001.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2013.06.29.0001.0000.patch",
            1_499_233_123,
        ),
        (
            "2013.06.29.0002.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2013.06.29.0002.0000.patch",
            1_498_710_404,
        ),
        (
            "2013.06.29.0003.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2013.06.29.0003.0000.patch",
            1_498_837_531,
        ),
        (
            "2013.06.29.0004.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2013.06.29.0004.0000.patch",
            1_338_394_725,
        ),
        (
            "2013.08.09.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2013.08.09.0000.0000.patch",
            1_499_488_272,
        ),
        (
            "2013.08.09.0001.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2013.08.09.0001.0000.patch",
            1_104_799_732,
        ),
        (
            "2013.11.26.0000.0002",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2013.11.26.0000.0002.patch",
            1_109_561_688,
        ),
        (
            "2013.12.05.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2013.12.05.0000.0000.patch",
            1_499_965_706,
        ),
        (
            "2013.12.05.0001.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2013.12.05.0001.0000.patch",
            536_825_180,
        ),
        (
            "2014.03.06.0000.0002",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2014.03.06.0000.0002.patch",
            682_380_696,
        ),
        (
            "2014.05.12.0000.0002",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2014.05.12.0000.0002.patch",
            1_370_456_092,
        ),
        (
            "2014.05.27.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/H2014.05.27.0000.0000h.patch",
            139_297_031,
        ),
        (
            "H2014.05.27.0000.0000a",
            "http://patch-dl.ffxiv.com/game/4e9a232b/H2014.05.27.0000.0000a.patch",
            1_499_383_758,
        ),
        (
            "H2014.05.27.0000.0000b",
            "http://patch-dl.ffxiv.com/game/4e9a232b/H2014.05.27.0000.0000b.patch",
            1_499_220_273,
        ),
        (
            "H2014.05.27.0000.0000c",
            "http://patch-dl.ffxiv.com/game/4e9a232b/H2014.05.27.0000.0000c.patch",
            1_498_992_741,
        ),
        (
            "H2014.05.27.0000.0000d",
            "http://patch-dl.ffxiv.com/game/4e9a232b/H2014.05.27.0000.0000d.patch",
            1_499_594_145,
        ),
        (
            "H2014.05.27.0000.0000e",
            "http://patch-dl.ffxiv.com/game/4e9a232b/H2014.05.27.0000.0000e.patch",
            1_499_632_220,
        ),
        (
            "H2014.05.27.0000.0000f",
            "http://patch-dl.ffxiv.com/game/4e9a232b/H2014.05.27.0000.0000f.patch",
            1_499_274_206,
        ),
        (
            "H2014.05.27.0000.0000g",
            "http://patch-dl.ffxiv.com/game/4e9a232b/H2014.05.27.0000.0000g.patch",
            1_499_155_300,
        ),
        (
            "2014.06.21.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2014.06.21.0000.0001.patch",
            14_290_027,
        ),
        (
            "2014.06.25.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2014.06.25.0000.0000.patch",
            1_499_810_701,
        ),
        (
            "2014.06.25.0001.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2014.06.25.0001.0000.patch",
            11_623_769,
        ),
        (
            "2014.10.03.0000.0002",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2014.10.03.0000.0002.patch",
            565_120_022,
        ),
        (
            "2014.10.17.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2014.10.17.0000.0000.patch",
            1_499_999_037,
        ),
        (
            "2014.10.17.0001.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2014.10.17.0001.0000.patch",
            74_222_565,
        ),
        (
            "2015.02.10.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2015.02.10.0000.0001.patch",
            1_372_945_182,
        ),
        (
            "2015.05.20.0000.0002",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2015.05.20.0000.0002.patch",
            347_123_426,
        ),
        (
            "2015.05.26.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2015.05.26.0000.0000.patch",
            1_499_889_857,
        ),
        (
            "2015.05.26.0001.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2015.05.26.0001.0000.patch",
            1_499_987_140,
        ),
        (
            "2015.05.26.0002.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2015.05.26.0002.0000.patch",
            801_468_542,
        ),
        (
            "2015.07.25.0000.0002",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2015.07.25.0000.0002.patch",
            481_251_195,
        ),
        (
            "2015.10.27.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2015.10.27.0000.0001.patch",
            1_031_856_541,
        ),
        (
            "2015.11.14.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2015.11.14.0000.0001.patch",
            35_695_740,
        ),
        (
            "2015.11.28.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2015.11.28.0000.0001.patch",
            21_522_682,
        ),
        (
            "2016.02.08.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.02.08.0000.0001.patch",
            821_798_528,
        ),
        (
            "2016.03.16.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.03.16.0000.0001.patch",
            50_172_626,
        ),
        (
            "2016.03.25.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.03.25.0000.0001.patch",
            45_277_444,
        ),
        (
            "2016.05.21.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.05.21.0000.0001.patch",
            837_717_550,
        ),
        (
            "2016.07.05.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.07.05.0000.0001.patch",
            122_145_410,
        ),
        (
            "2016.07.21.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.07.21.0000.0001.patch",
            38_913_768,
        ),
        (
            "2016.07.22.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.07.22.0000.0000.patch",
            18_532_746,
        ),
        (
            "2016.08.07.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.08.07.0000.0001.patch",
            18_941_130,
        ),
        (
            "2016.08.08.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.08.08.0000.0000.patch",
            18_528_770,
        ),
        (
            "2016.09.10.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.09.10.0000.0000.patch",
            873_512_921,
        ),
        (
            "2016.09.20.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.09.20.0000.0001.patch",
            38_665_222,
        ),
        (
            "2016.09.21.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.09.21.0000.0000.patch",
            19_497_637,
        ),
        (
            "2016.10.02.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.10.02.0000.0001.patch",
            22_848_464,
        ),
        (
            "2016.10.03.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.10.03.0000.0000.patch",
            19_018_685,
        ),
        (
            "2016.10.10.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.10.10.0000.0001.patch",
            21_075_821,
        ),
        (
            "2016.10.11.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.10.11.0000.0000.patch",
            19_027_901,
        ),
        (
            "2016.10.21.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.10.21.0000.0000.patch",
            120_948_411,
        ),
        (
            "2016.10.25.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.10.25.0000.0001.patch",
            32_473_107,
        ),
        (
            "2016.10.26.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.10.26.0000.0000.patch",
            20_926_867,
        ),
        (
            "2016.11.10.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.11.10.0000.0000.patch",
            27_141_376,
        ),
        (
            "2016.11.11.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.11.11.0000.0000.patch",
            19_181_085,
        ),
        (
            "2016.12.08.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.12.08.0000.0000.patch",
            19_037_757,
        ),
        (
            "2016.12.17.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2016.12.17.0000.0000.patch",
            750_123_737,
        ),
        (
            "2017.01.10.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.01.10.0000.0001.patch",
            37_806_236,
        ),
        (
            "2017.01.11.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.01.11.0000.0000.patch",
            24_773_971,
        ),
        (
            "2017.01.24.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.01.24.0000.0000.patch",
            24_027_763,
        ),
        (
            "2017.01.31.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.01.31.0000.0000.patch",
            20_302_910,
        ),
        (
            "2017.02.09.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.02.09.0000.0000.patch",
            80_483_283,
        ),
        (
            "2017.02.14.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.02.14.0000.0001.patch",
            77_822_748,
        ),
        (
            "2017.02.22.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.02.22.0000.0001.patch",
            39_120_267,
        ),
        (
            "2017.02.22.0001.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.02.22.0001.0000.patch",
            19_411_941,
        ),
        (
            "2017.03.03.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.03.03.0000.0000.patch",
            66_709_964,
        ),
        (
            "2017.03.04.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.03.04.0000.0000.patch",
            19_388_861,
        ),
        (
            "2017.03.12.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.03.12.0000.0001.patch",
            20_097_813,
        ),
        (
            "2017.03.13.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.03.13.0000.0000.patch",
            19_542_429,
        ),
        (
            "2017.03.17.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.03.17.0000.0000.patch",
            121_087_613,
        ),
        (
            "2017.03.22.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.03.22.0000.0001.patch",
            30_609_782,
        ),
        (
            "2017.03.23.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.03.23.0000.0000.patch",
            20_401_326,
        ),
        (
            "2017.04.13.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.04.13.0000.0001.patch",
            25_517_380,
        ),
        (
            "2017.05.26.0000.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.05.26.0000.0000.patch",
            1_499_984_269,
        ),
        (
            "2017.05.26.0001.0000",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2017.05.26.0001.0000.patch",
            675_118_392,
        ),
    ],
),
(
    "6b936f08",
    &[
        (
            "2015.03.16.0000.0000",
            "http://patch-dl.ffxiv.com/game/ex1/6b936f08/D2015.03.16.0000.0000.patch",
            1_385_754_435,
        ),
        (
            "2015.05.26.0000.0000",
            "http://patch-dl.ffxiv.com/game/ex1/6b936f08/D2015.05.26.0000.0000.patch",
            1_499_639_789,
        ),
        (
            "2015.05.26.0001.0000",
            "http://patch-dl.ffxiv.com/game/ex1/6b936f08/D2015.05.26.0001.0000.patch",
            1_020_630_197,
        ),
        (
            "2015.07.03.0000.0002",
            "http://patch-dl.ffxiv.com/game/ex1/6b936f08/D2015.07.03.0000.0002.patch",
            244_884_670,
        ),
        (
            "2015.08.20.0000.0001",
            "http://patch-dl.ffxiv.com/game/ex1/6b936f08/D2015.08.20.0000.0001.patch",
            86_971_673,
        ),
        (
            "2015.10.27.0000.0000",
            "http://patch-dl.ffxiv.com/game/ex1/6b936f08/H2015.10.27.0000.0000c.patch",
            537_937_126,
        ),
        (
            "H2015.10.27.0000.0000a",
            "http://patch-dl.ffxiv.com/game/ex1/6b936f08/H2015.10.27.0000.0000a.patch",
            1_498_642_548,
        ),
        (
            "H2015.10.27.0000.0000b",
            "http://patch-dl.ffxiv.com/game/ex1/6b936f08/H2015.10.27.0000.0000b.patch",
            1_499_638_689,
        ),
        (
            "2016.02.08.0000.0001",
            "http://patch-dl.ffxiv.com/game/ex1/6b936f08/D2016.02.08.0000.0001.patch",
            526_937_523,
        ),
        (
            "2016.05.21.0000.0001",
            "http://patch-dl.ffxiv.com/game/ex1/6b936f08/D2016.05.21.0000.0001.patch",
            565_269_990,
        ),
        (
            "2016.09.10.0000.0001",
            "http://patch-dl.ffxiv.com/game/ex1/6b936f08/D2016.09.10.0000.0001.patch",
            650_021_763,
        ),
        (
            "2016.12.17.0000.0001",
            "http://patch-dl.ffxiv.com/game/ex1/6b936f08/D2016.12.17.0000.0001.patch",
            344_071_694,
        ),
    ],
),
(
    "f29a3eb2",
    &[
        (
            "2023.12.12.0000.0000",
            "http://patch-dl.ffxiv.com/game/ex2/f29a3eb2/D2023.12.12.0000.0000.patch",
            399_216,
        ),
        (
            "2024.04.23.0000.0001",
            "http://patch-dl.ffxiv.com/game/ex2/f29a3eb2/D2024.04.23.0000.0001.patch",
            49_368_956,
        ),
        (
            "2024.05.25.0000.0000",
            "http://patch-dl.ffxiv.com/game/ex2/f29a3eb2/D2024.05.25.0000.0000.patch",
            1_499_705_173,
        ),
        (
            "2024.05.25.0001.0000",
            "http://patch-dl.ffxiv.com/game/ex2/f29a3eb2/D2024.05.25.0001.0000.patch",
            1_497_802_120,
        ),
        (
            "2024.05.25.0002.0000",
            "http://patch-dl.ffxiv.com/game/ex2/f29a3eb2/D2024.05.25.0002.0000.patch",
            1_499_982_470,
        ),
        (
            "2024.05.25.0003.0000",
            "http://patch-dl.ffxiv.com/game/ex2/f29a3eb2/D2024.05.25.0003.0000.patch",
            689_912_486,
        ),
        (
            "2024.05.30.0000.0001",
            "http://patch-dl.ffxiv.com/game/ex2/f29a3eb2/D2024.05.30.0000.0001.patch",
            14_956_202,
        ),
    ],
)];

type Overrides = HashMap<GameVersion, Option<GameVersion>>;

static PARSED_OVERRIDES: LazyLock<HashMap<&'static str, Overrides>> = LazyLock::new(|| {
    OVERRIDES
        .iter()
        .map(|(slug, entries)| {
            let entries = entries
                .iter()
                .map(|(from, to)| {
                    (
                        GameVersion::new(from).expect("override version"),
                        to.map(|to| GameVersion::new(to).expect("override version")),
                    )
                })
                .collect();
            (*slug, entries)
        })
        .collect()
});

static PARSED_EXTRA: LazyLock<HashMap<&'static str, Vec<Node>>> = LazyLock::new(|| {
    EXTRA_VERSIONS
        .iter()
        .map(|(slug, entries)| {
            let nodes = entries
                .iter()
                .map(|(version, url, size)| Node {
                    version: GameVersion::new(version).expect("extra version"),
                    is_active: false,
                    prerequisites: Vec::new(),
                    patches: vec![Patch {
                        url: (*url).to_string(),
                        size: *size,
                    }],
                })
                .collect();
            (*slug, nodes)
        })
        .collect()
});

pub fn overrides_for(slug: &str) -> Option<&'static Overrides> {
    PARSED_OVERRIDES.get(slug)
}

/// A version's place in the graph: the patch that reaches it, and the version that
/// patch is applied on top of.
#[derive(Debug, Clone)]
pub struct Step {
    pub version: GameVersion,
    pub patch: Patch,
    pub parent: Option<GameVersion>,
}

pub async fn get_versions(client: &Client, slug: &str) -> Result<Vec<Node>> {
    let mut nodes = super::query_versions(client, slug).await?;

    if let Some(extra) = PARSED_EXTRA.get(slug) {
        for node in extra {
            log::debug!("Injecting {} into patch chain", node.version);
            nodes.push(node.clone());
        }
    }

    Ok(nodes)
}

/// The ordered list of patches to apply to reach `version`.
pub async fn get_patch_chain(
    client: &Client,
    slug: &str,
    version: &GameVersion,
) -> Result<Vec<(GameVersion, Patch)>> {
    let nodes = get_versions(client, slug).await?;
    build_chain(slug, &nodes, version)
}

/// Every version the repository offers, not only those on the chain to one of them.
pub async fn get_patch_forest(client: &Client, slug: &str) -> Result<Vec<Step>> {
    let nodes = get_versions(client, slug).await?;
    build_forest(slug, &nodes)
}

type VersionIndex<'a> = HashMap<&'a GameVersion, &'a Node>;

fn index_versions<'a>(slug: &str, nodes: &'a [Node]) -> VersionIndex<'a> {
    let mut by_version = HashMap::with_capacity(nodes.len());
    for node in nodes {
        if by_version.insert(&node.version, node).is_some() {
            // Thaliak has caught up with an injected version, so EXTRA_VERSIONS has an
            // entry it no longer needs.
            log::warn!("{slug} lists {} more than once", node.version);
        }
    }
    by_version
}

fn sole_patch<'a>(slug: &str, node: &'a Node) -> Result<&'a Patch> {
    ensure!(
        node.patches.len() == 1,
        "{slug} version {} has {} patches, expected 1",
        node.version,
        node.patches.len()
    );
    Ok(&node.patches[0])
}

/// The version a patch is applied on top of, or `None` where a lineage begins.
///
/// Thaliak lists a lineage's every later version as a prerequisite of the full-install
/// patch that starts it, so those edges point forward. A prerequisite is older than what
/// it precedes, which makes the newer ones no such thing; discarding them also leaves a
/// walk whose version strictly descends, and which therefore ends.
fn predecessor<'a>(
    slug: &str,
    by_version: &VersionIndex<'a>,
    node: &Node,
) -> Result<Option<&'a Node>> {
    if let Some(replacement) = overrides_for(slug).and_then(|o| o.get(&node.version)) {
        let Some(replacement) = replacement else {
            return Ok(None);
        };
        log::debug!("Overriding {} with {replacement}", node.version);
        return by_version
            .get(replacement)
            .copied()
            .with_context(|| format!("{slug} has no version {replacement}"))
            .map(Some);
    }

    // A lineage's own start is not a version with nothing before it: Thaliak states a full
    // install's *successors* as its prerequisites, so the real root of the global game repository
    // lists sixteen of them. A genuinely empty list is Thaliak having dropped the edge, and taking
    // it at face value makes the version a root, which builds an index of that one patch and
    // nothing before it. Refused here rather than published: an eight-kilobyte CLUT where a whole
    // install takes seven megabytes reads as a working build.
    ensure!(
        !node.prerequisites.is_empty(),
        "{slug} version {} states no prerequisite at all; Thaliak has dropped the edge, so name \
         its predecessor in this module's override table",
        node.version
    );

    // Among the prerequisites, take the newest. An inactive version may still lead back
    // through inactive ones; an active one may not.
    Ok(node
        .prerequisites
        .iter()
        .filter(|prereq| **prereq < node.version)
        .map(|prereq| {
            by_version
                .get(prereq)
                .copied()
                .with_context(|| format!("{slug} has no version {prereq}"))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|prereq| !node.is_active || prereq.is_active)
        .max_by(|a, b| a.version.cmp(&b.version)))
}

fn build_chain(
    slug: &str,
    nodes: &[Node],
    version: &GameVersion,
) -> Result<Vec<(GameVersion, Patch)>> {
    let by_version = index_versions(slug, nodes);

    let mut chain: Vec<(GameVersion, Patch)> = Vec::new();
    let mut current = by_version.get(version).copied();
    while let Some(node) = current {
        chain.push((node.version.clone(), sole_patch(slug, node)?.clone()));
        current = predecessor(slug, &by_version, node)?;
    }

    chain.reverse();
    Ok(chain)
}

/// Every version in the repository, each paired with the one it follows. Ordered so
/// that a version comes after its parent, which holds because a parent is older.
pub fn build_forest(slug: &str, nodes: &[Node]) -> Result<Vec<Step>> {
    let by_version = index_versions(slug, nodes);

    let mut steps = by_version
        .values()
        .map(|node| {
            Ok(Step {
                version: node.version.clone(),
                patch: sole_patch(slug, node)?.clone(),
                parent: predecessor(slug, &by_version, node)?.map(|parent| parent.version.clone()),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    steps.sort_by(|a, b| a.version.cmp(&b.version));
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(version: &str, prerequisites: &[&str]) -> Node {
        Node {
            version: GameVersion::new(version).unwrap(),
            is_active: true,
            prerequisites: prerequisites
                .iter()
                .map(|version| GameVersion::new(version).unwrap())
                .collect(),
            patches: vec![Patch {
                url: format!("http://patches/{version}.patch"),
                size: 1,
            }],
        }
    }

    /// The lineage: a full install, one more historic patch, then two ordinary ones. The
    /// full install lists everything that follows it as a prerequisite.
    fn lineage() -> [Node; 4] {
        [
            node(
                "H2024.01.01.0000.0000a",
                &["2024.02.02.0000.0000", "2024.03.03.0000.0000"],
            ),
            node("H2024.01.01.0000.0000b", &["H2024.01.01.0000.0000a"]),
            node("2024.02.02.0000.0000", &["H2024.01.01.0000.0000b"]),
            node("2024.03.03.0000.0000", &["2024.02.02.0000.0000"]),
        ]
    }

    fn walked(nodes: &[Node], version: &str) -> Vec<String> {
        build_chain("slug", nodes, &GameVersion::new(version).unwrap())
            .unwrap()
            .into_iter()
            .map(|(version, _)| version.to_string())
            .collect()
    }

    /// Thaliak dropping an edge leaves a version with nothing before it, which taken at face value
    /// makes it a root and builds an index of that one patch. Refused rather than published.
    #[test]
    fn a_version_with_no_prerequisite_at_all_is_refused() {
        let mut nodes = lineage().to_vec();
        nodes.push(node("2024.04.04.0000.0000", &[]));
        let why = build_forest("slug", &nodes).expect_err("a dropped edge is not a lineage start");
        assert!(
            why.to_string().contains("states no prerequisite at all"),
            "{why}"
        );
        // The real lineage start is not refused: it states its successors, which the walk discards.
        assert!(build_forest("slug", &lineage()).is_ok());
    }

    /// The table is parsed lazily and panics on a malformed version, so nothing would catch a typo
    /// until a real chain walked through it. Forcing it also pins the edges Thaliak leaves out.
    #[test]
    fn the_override_table_parses_and_names_the_missing_global_edges() {
        let global = overrides_for("4e9a232b").expect("the global overrides");
        let edge = |version: &str| {
            global
                .get(&GameVersion::new(version).unwrap())
                .expect("an override")
                .as_ref()
                .map(ToString::to_string)
        };
        // Thaliak states no prerequisite for these three at all, and each is a `DIFF` patch, so
        // each has to apply on top of the one before it.
        assert_eq!(edge("2026.08.19.0000.0000").as_deref(), Some("2026.08.11.0000.0000"));
        assert_eq!(edge("2026.08.31.0000.0000").as_deref(), Some("2026.08.19.0000.0000"));
        assert_eq!(edge("2026.09.01.0000.0000").as_deref(), Some("2026.08.31.0000.0000"));
        // The one entry that deliberately ends a lineage rather than pointing further back.
        assert_eq!(edge("H2017.06.06.0000.0001a"), None);
        // Every other repository's table parses too.
        for (slug, _) in OVERRIDES {
            assert!(overrides_for(slug).is_some(), "{slug} has no parsed overrides");
        }
    }

    /// Taking a full install's forward-pointing prerequisites at face value sends a walk
    /// up the lineage and back down it, so a version in the middle would be reached by
    /// applying its own successors first.
    #[test]
    fn a_chain_stops_where_its_lineage_begins() {
        let nodes = lineage();
        assert_eq!(
            walked(&nodes, "H2024.01.01.0000.0000b"),
            ["H2024.01.01.0000.0000a", "H2024.01.01.0000.0000b"]
        );
        assert_eq!(
            walked(&nodes, "2024.03.03.0000.0000"),
            [
                "H2024.01.01.0000.0000a",
                "H2024.01.01.0000.0000b",
                "2024.02.02.0000.0000",
                "2024.03.03.0000.0000",
            ]
        );
    }

    /// The walk to a version is the walk to its parent with that version on the end, so
    /// folding the forest reaches every version having applied each patch once.
    #[test]
    fn a_forest_holds_every_version_after_the_one_it_follows() {
        let nodes = lineage();
        let forest = build_forest("slug", &nodes).unwrap();
        assert_eq!(forest.len(), nodes.len());
        assert_eq!(
            forest.iter().filter(|step| step.parent.is_none()).count(),
            1
        );

        let mut placed: Vec<GameVersion> = Vec::new();
        for step in &forest {
            if let Some(parent) = &step.parent {
                assert!(
                    placed.contains(parent),
                    "{} is placed before {parent}, which it follows",
                    step.version
                );
                assert!(
                    *parent < step.version,
                    "{parent} is not older than {}",
                    step.version
                );
            }
            let chain = walked(&nodes, &step.version.to_string());
            assert_eq!(chain.last().unwrap(), &step.version.to_string());
            placed.push(step.version.clone());
        }
    }
}
