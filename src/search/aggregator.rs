use crate::model::{PackageHit, PackageRow, Provider, SourceId};
use crate::sources::installed::InstalledIndex;
use std::collections::HashMap;

/// Merge hits from all sources into one row per distinct package name.
pub fn merge(hits: Vec<PackageHit>, installed: &InstalledIndex) -> Vec<PackageRow> {
    let mut map: HashMap<String, PackageRow> = HashMap::new();

    for hit in hits {
        let provider = Provider {
            source_id: hit.source_id,
            version: hit.version.clone(),
            installed: installed.is_installed(&hit.name),
            installed_version: installed.version(&hit.name).map(str::to_string),
            meta: hit.meta.clone(),
        };

        let row = map.entry(hit.name.clone()).or_insert_with(|| PackageRow {
            name: hit.name.clone(),
            providers: Vec::new(),
            best_description: String::new(),
        });

        if row.best_description.is_empty() && !hit.description.is_empty() {
            row.best_description = hit.description.clone();
        }
        if !row.providers.iter().any(|p| p.source_id == provider.source_id) {
            row.providers.push(provider);
        }
    }

    let mut rows: Vec<PackageRow> = map.into_values().collect();
    for row in &mut rows {
        row.providers.sort_by_key(|p| source_order(p.source_id));
    }
    rows
}

fn source_order(id: SourceId) -> u8 {
    match id {
        SourceId::Pacman => 0,
        SourceId::Aur => 1,
    }
}

/// Sort rows by relevance to `query`: exact > prefix > substring, then shorter
/// name, then alphabetical. Total + deterministic.
pub fn relevance_sort(query: &str, rows: &mut [PackageRow]) {
    let q = query.to_lowercase();
    rows.sort_by(|a, b| {
        rank(&q, &a.name)
            .cmp(&rank(&q, &b.name))
            .then_with(|| a.name.len().cmp(&b.name.len()))
            .then_with(|| a.name.cmp(&b.name))
    });
}

fn rank(q: &str, name: &str) -> u8 {
    let n = name.to_lowercase();
    if n == q {
        0
    } else if n.starts_with(q) {
        1
    } else if n.contains(q) {
        2
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SourceMeta;

    fn hit(name: &str, source: SourceId, ver: &str, desc: &str) -> PackageHit {
        PackageHit {
            name: name.into(),
            version: ver.into(),
            source_id: source,
            description: desc.into(),
            meta: SourceMeta::default(),
        }
    }

    #[test]
    fn merges_same_name_across_sources_into_one_row() {
        let hits = vec![
            hit("firefox", SourceId::Aur, "141.0-1", ""),
            hit("firefox", SourceId::Pacman, "141.0", "A web browser"),
            hit("firefox-bin", SourceId::Aur, "141.0-1", "binary build"),
        ];
        let idx = InstalledIndex::from_query_output("firefox 141.0\n");
        let mut rows = merge(hits, &idx);
        relevance_sort("firefox", &mut rows);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "firefox");
        assert_eq!(rows[0].providers.len(), 2);
        assert_eq!(rows[0].providers[0].source_id, SourceId::Pacman);
        assert_eq!(rows[0].providers[1].source_id, SourceId::Aur);
        assert!(rows[0].providers[0].installed);
        assert_eq!(rows[0].providers[0].installed_version.as_deref(), Some("141.0"));
        assert_eq!(rows[0].best_description, "A web browser");

        assert_eq!(rows[1].name, "firefox-bin");
        assert!(!rows[1].any_installed());
    }

    #[test]
    fn relevance_orders_exact_prefix_substring() {
        let idx = InstalledIndex::default();
        let hits = vec![
            hit("xfirefox", SourceId::Aur, "1", ""),
            hit("firefox-bin", SourceId::Aur, "1", ""),
            hit("firefox", SourceId::Pacman, "1", ""),
        ];
        let mut rows = merge(hits, &idx);
        relevance_sort("firefox", &mut rows);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["firefox", "firefox-bin", "xfirefox"]);
    }
}
