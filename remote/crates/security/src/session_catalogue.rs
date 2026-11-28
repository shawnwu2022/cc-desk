//! Desktop-side read model for session lists. No wire decoder or action authority.
use std::collections::{BTreeMap, BTreeSet};

pub type OpaqueKey = [u8; 32];
pub const MAX_CATALOGUE_CANDIDATES: usize = 8192;
const MAX_TITLE_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ListMode {
    Sessions,
    Archived,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveEvidence {
    NotArchived,
    Archived,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DisplayLifecycle {
    Open,
    Closed,
    History,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRow {
    /// Backend-only source approved for this desktop and paired device.
    pub source: OpaqueKey,
    /// Backend-only stable physical identity, for alias conflict detection.
    pub physical: OpaqueKey,
    pub archive: ArchiveEvidence,
    pub lifecycle: DisplayLifecycle,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayRow {
    pub lifecycle: DisplayLifecycle,
    pub title: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogueError {
    ReadBudget,
}

/// A bounded snapshot built only from sources already retained by the desktop.
/// Keep physical identity until all admitted batches have been considered.
#[derive(Default)]
pub struct OrdinaryPortfolio {
    admitted: BTreeSet<OpaqueKey>,
    rows: Vec<SourceRow>,
    received: usize,
    overflowed: bool,
}

impl OrdinaryPortfolio {
    pub fn push(&mut self, source: OpaqueKey, rows: Vec<SourceRow>) -> Result<(), CatalogueError> {
        self.received = self.received.saturating_add(rows.len());
        if self.received > MAX_CATALOGUE_CANDIDATES {
            self.overflowed = true;
            return Err(CatalogueError::ReadBudget);
        }
        let previous_len = self.rows.len();
        self.rows
            .extend(rows.into_iter().filter(|row| row.source == source));
        if self.rows.len() != previous_len {
            self.admitted.insert(source);
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Vec<DisplayRow>, CatalogueError> {
        if self.overflowed {
            return Err(CatalogueError::ReadBudget);
        }
        list(ListMode::Sessions, &self.admitted, &self.rows)
    }
}

/// A caller supplies fresh rows and source keys already admitted for the
/// exact paired device. This function cannot establish pairing or sign data.
/// Its output is a display projection, not a reusable control target.
pub fn list(
    mode: ListMode,
    admitted_sources: &BTreeSet<OpaqueKey>,
    rows: &[SourceRow],
) -> Result<Vec<DisplayRow>, CatalogueError> {
    if rows.len() > MAX_CATALOGUE_CANDIDATES {
        return Err(CatalogueError::ReadBudget);
    }
    let mut seen = BTreeMap::<OpaqueKey, Option<&SourceRow>>::new();
    for row in rows
        .iter()
        .filter(|row| admitted_sources.contains(&row.source))
    {
        // Detect collisions before filtering archive state. A profile alias
        // must not mask a contradictory or indeterminate provider result.
        seen.entry(row.physical)
            .and_modify(|slot| *slot = None)
            .or_insert(Some(row));
    }
    let wanted = match mode {
        ListMode::Sessions => ArchiveEvidence::NotArchived,
        ListMode::Archived => ArchiveEvidence::Archived,
    };
    Ok(seen
        .into_values()
        .flatten()
        .filter(|row| row.archive == wanted)
        // An apparently live archived row is contradictory.
        .filter(|row| mode != ListMode::Archived || row.lifecycle != DisplayLifecycle::Open)
        .filter(|row| title_safe(&row.title))
        .map(|row| DisplayRow {
            lifecycle: row.lifecycle,
            title: row.title.clone(),
        })
        .collect())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn valid_display_row(mode: ListMode, row: &DisplayRow) -> bool {
    title_safe(&row.title)
        && (mode != ListMode::Archived || row.lifecycle != DisplayLifecycle::Open)
}

fn title_safe(title: &str) -> bool {
    title.len() <= MAX_TITLE_BYTES
        && !title.trim().is_empty()
        && !title.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        source: u8,
        physical: u8,
        archive: ArchiveEvidence,
        lifecycle: DisplayLifecycle,
        title: &str,
    ) -> SourceRow {
        SourceRow {
            source: [source; 32],
            physical: [physical; 32],
            archive,
            lifecycle,
            title: title.into(),
        }
    }

    #[test]
    fn ordinary_listing_contains_open_and_closed_but_never_archived() {
        use ArchiveEvidence::*;
        use DisplayLifecycle::*;
        let rows = [
            item(1, 1, NotArchived, Open, "open"),
            item(1, 2, NotArchived, Closed, "closed"),
            item(1, 3, Archived, Closed, "archived"),
            item(1, 4, Unknown, History, "unknown"),
            item(2, 5, NotArchived, Open, "other device"),
        ];
        let allowed = BTreeSet::from([[1; 32]]);
        for (mode, expected) in [
            (ListMode::Sessions, vec!["open", "closed"]),
            (ListMode::Archived, vec!["archived"]),
        ] {
            let result = list(mode, &allowed, &rows).unwrap();
            assert_eq!(
                result
                    .iter()
                    .map(|row| row.title.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn duplicate_identity_and_contradictory_live_archive_are_hidden() {
        use ArchiveEvidence::*;
        use DisplayLifecycle::*;
        let rows = [
            item(1, 1, NotArchived, Closed, "seems visible"),
            item(2, 1, Unknown, History, "ambiguous alias"),
            item(1, 2, Archived, Open, "contradiction"),
            item(1, 3, Archived, Closed, "visible archive"),
        ];
        let allowed = BTreeSet::from([[1; 32], [2; 32]]);
        assert!(
            list(ListMode::Sessions, &allowed, &rows)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            list(ListMode::Archived, &allowed, &rows).unwrap()[0].title,
            "visible archive"
        );
    }

    #[test]
    fn ordinary_portfolio_applies_archive_evidence_after_combining_scopes() {
        use ArchiveEvidence::*;
        use DisplayLifecycle::*;
        let mut portfolio = OrdinaryPortfolio::default();
        portfolio
            .push(
                [1; 32],
                vec![
                    item(1, 1, NotArchived, Open, "open"),
                    item(1, 3, NotArchived, Closed, "unresolved alias"),
                ],
            )
            .unwrap();
        portfolio
            .push(
                [2; 32],
                vec![
                    item(2, 2, NotArchived, Closed, "closed"),
                    item(2, 3, Unknown, History, "ambiguous"),
                    item(2, 4, Archived, Closed, "archived"),
                    item(9, 5, NotArchived, Open, "outside scope"),
                ],
            )
            .unwrap();
        assert_eq!(
            portfolio.finish().unwrap(),
            [
                DisplayRow {
                    lifecycle: Open,
                    title: "open".into()
                },
                DisplayRow {
                    lifecycle: Closed,
                    title: "closed".into()
                },
            ]
        );
    }

    #[test]
    fn ordinary_portfolio_bounds_total_candidates_across_scopes() {
        let mut portfolio = OrdinaryPortfolio::default();
        portfolio
            .push(
                [1; 32],
                vec![
                    item(
                        1,
                        1,
                        ArchiveEvidence::NotArchived,
                        DisplayLifecycle::Closed,
                        "row"
                    );
                    MAX_CATALOGUE_CANDIDATES
                ],
            )
            .unwrap();
        assert_eq!(
            portfolio.push(
                [2; 32],
                vec![item(
                    2,
                    2,
                    ArchiveEvidence::NotArchived,
                    DisplayLifecycle::Open,
                    "last"
                )]
            ),
            Err(CatalogueError::ReadBudget)
        );
        assert_eq!(portfolio.finish(), Err(CatalogueError::ReadBudget));
    }

    #[test]
    fn bounds_and_untrusted_display_data_do_not_fall_back_to_other_data() {
        let scope = BTreeSet::from([[1; 32]]);
        let a = item(
            1,
            1,
            ArchiveEvidence::NotArchived,
            DisplayLifecycle::History,
            "\u{1b}[2J",
        );
        assert!(
            list(ListMode::Sessions, &scope, std::slice::from_ref(&a))
                .unwrap()
                .is_empty()
        );
        let too_many = vec![a; MAX_CATALOGUE_CANDIDATES + 1];
        assert_eq!(
            list(ListMode::Sessions, &scope, &too_many),
            Err(CatalogueError::ReadBudget)
        );
    }
}
