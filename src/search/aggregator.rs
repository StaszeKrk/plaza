use crate::model::{normalize_key, PackageHit, PackageRow, Provider, SourceId};
use crate::sources::installed::InstalledIndex;
use std::collections::HashMap;

/// Merge hits from all sources into rows, keyed by a layered match key: a hit's
/// canonical ID when present, else its normalized name (lowercased, trimmed,
/// with a trailing `-bin`/`-git` stripped when `group_variants` is on).
///
/// Processed in three rounds so the key is order-independent: base names first,
/// then stripped variants (which join an existing base bucket, else stand
/// alone), then Flatpak hits (which bridge onto an existing repo/AUR bucket by
/// their normalized human name when grouping is on, else stand alone in their
/// own app-ID bucket). The row label is the shortest raw name in the bucket.
pub fn merge(
    hits: Vec<PackageHit>,
    installed: &InstalledIndex,
    group_variants: bool,
) -> Vec<PackageRow> {
    let mut map: HashMap<String, PackageRow> = HashMap::new();

    // Round 0: base hits. Round 1: stripped variants. Round 2: Flatpak.
    let mut rounds: Vec<Vec<&PackageHit>> = vec![Vec::new(), Vec::new(), Vec::new()];
    for h in &hits {
        rounds[round_of(h, group_variants)].push(h);
    }

    for (round, group) in rounds.into_iter().enumerate() {
        for hit in group {
            let key = bucket_key(hit, group_variants, round == 1, &map);
            let target = hit
                .meta
                .canonical_id
                .clone()
                .unwrap_or_else(|| hit.name.clone());
            let provider = Provider {
                source_id: hit.source_id,
                version: hit.version.clone(),
                installed: installed.is_installed(&target),
                installed_version: installed.version(&target).map(str::to_string),
                target,
                meta: hit.meta.clone(),
            };
            let row = map.entry(key).or_insert_with(|| PackageRow {
                name: hit.name.clone(),
                providers: Vec::new(),
                best_description: String::new(),
            });
            // Display label: the shortest raw name across the bucket.
            if hit.name.len() < row.name.len() {
                row.name = hit.name.clone();
            }
            if row.best_description.is_empty() && !hit.description.is_empty() {
                row.best_description = hit.description.clone();
            }
            // Dedup per (source, repo) so a package in several pacman repos keeps a
            // provider for each (world, extra-x86-64-v3, extra, …), in priority order.
            let dup = row
                .providers
                .iter()
                .any(|p| p.source_id == provider.source_id && p.meta.repo == provider.meta.repo);
            if !dup {
                row.providers.push(provider);
            }
        }
    }

    let mut rows: Vec<PackageRow> = map.into_values().collect();
    for row in &mut rows {
        row.providers.sort_by_key(|p| source_order(p.source_id));
    }
    rows
}

/// Which merge round a hit belongs to (0 base, 1 stripped variant, 2 Flatpak).
fn round_of(h: &PackageHit, group_variants: bool) -> usize {
    if h.source_id == SourceId::Flatpak {
        return 2;
    }
    let raw = h.name.trim().to_lowercase();
    if group_variants && h.meta.canonical_id.is_none() && normalize_key(&h.name, true) != raw {
        return 1;
    }
    0
}

/// The bucket key for a hit, given whether it is a stripped variant and the
/// buckets filled so far.
fn bucket_key(
    hit: &PackageHit,
    group_variants: bool,
    is_stripped: bool,
    map: &HashMap<String, PackageRow>,
) -> String {
    // Flatpak: bridge onto an existing repo/AUR bucket by normalized human name
    // (grouping on), else stand alone in its own app-ID bucket.
    if hit.source_id == SourceId::Flatpak {
        if group_variants {
            let nk = format!("name:{}", normalize_key(&hit.name, true));
            if map.contains_key(&nk) {
                return nk;
            }
        }
        if let Some(id) = &hit.meta.canonical_id {
            return format!("id:{id}");
        }
    }
    if let Some(id) = &hit.meta.canonical_id {
        return format!("id:{id}");
    }
    if is_stripped {
        // Existence predicate: only fold into a base bucket that actually exists,
        // otherwise keep the variant as its own row under its raw name.
        let base = format!("name:{}", normalize_key(&hit.name, true));
        if map.contains_key(&base) {
            return base;
        }
        return format!("name:{}", hit.name.trim().to_lowercase());
    }
    format!("name:{}", normalize_key(&hit.name, group_variants))
}

fn source_order(id: SourceId) -> u8 {
    match id {
        SourceId::Pacman => 0,
        SourceId::Aur => 1,
        SourceId::Flatpak => 2,
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

/// Byte range of the first case-insensitive (ASCII) occurrence of `query` in
/// `name`, for underlining the matched part in the results list. ASCII case
/// folding preserves byte offsets, so the range slices `name` directly.
pub fn match_range(name: &str, query: &str) -> Option<(usize, usize)> {
    if query.is_empty() {
        return None;
    }
    let start = name.to_ascii_lowercase().find(&query.to_ascii_lowercase())?;
    Some((start, start + query.len()))
}

pub fn rank(q: &str, name: &str) -> u8 {
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

    fn fhit(name: &str, source: SourceId, canonical: Option<&str>) -> PackageHit {
        PackageHit {
            name: name.into(),
            version: "1".into(),
            source_id: source,
            description: String::new(),
            meta: SourceMeta {
                canonical_id: canonical.map(str::to_string),
                ..Default::default()
            },
        }
    }

    #[test]
    fn groups_variants_and_flatpak_bridge_when_on() {
        let inst = InstalledIndex::default();
        let hits = vec![
            fhit("gimp", SourceId::Pacman, None),
            fhit("gimp-git", SourceId::Aur, None),
            fhit("GIMP", SourceId::Flatpak, Some("org.gimp.GIMP")),
        ];
        let rows = merge(hits, &inst, true);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "gimp"); // shortest base label
        assert_eq!(rows[0].providers.len(), 3);
        let fp = rows[0]
            .providers
            .iter()
            .find(|p| p.source_id == SourceId::Flatpak)
            .unwrap();
        assert_eq!(fp.target, "org.gimp.GIMP"); // installs the app id, not "gimp"
    }

    #[test]
    fn multiword_flatpak_name_stays_standalone() {
        let inst = InstalledIndex::default();
        let hits = vec![
            fhit("code", SourceId::Pacman, None),
            fhit("Visual Studio Code", SourceId::Flatpak, Some("com.visualstudio.code")),
        ];
        let rows = merge(hits, &inst, true);
        assert_eq!(rows.len(), 2); // no false merge onto "code"
    }

    #[test]
    fn orphan_variant_stays_standalone_without_base() {
        let inst = InstalledIndex::default();
        let hits = vec![fhit("python-git", SourceId::Aur, None)];
        let rows = merge(hits, &inst, true);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "python-git"); // did not invent a "python" row
    }

    #[test]
    fn grouping_off_keeps_separate_rows_and_standalone_flatpak() {
        let inst = InstalledIndex::default();
        let hits = vec![
            fhit("gimp", SourceId::Pacman, None),
            fhit("gimp-bin", SourceId::Aur, None),
            fhit("GIMP", SourceId::Flatpak, Some("org.gimp.GIMP")),
        ];
        let rows = merge(hits, &inst, false);
        assert_eq!(rows.len(), 3); // nothing merges with grouping off
    }

    #[test]
    fn merges_same_name_across_sources_into_one_row() {
        let hits = vec![
            hit("firefox", SourceId::Aur, "141.0-1", ""),
            hit("firefox", SourceId::Pacman, "141.0", "A web browser"),
            hit("firefox-bin", SourceId::Aur, "141.0-1", "binary build"),
        ];
        let idx = InstalledIndex::from_query_output("firefox 141.0\n");
        let mut rows = merge(hits, &idx, false);
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

    fn hit_repo(name: &str, ver: &str, repo: &str) -> PackageHit {
        PackageHit {
            name: name.into(),
            version: ver.into(),
            source_id: SourceId::Pacman,
            description: String::new(),
            meta: SourceMeta {
                repo: Some(repo.into()),
                maintained: true,
                ..Default::default()
            },
        }
    }

    #[test]
    fn keeps_one_provider_per_pacman_repo_in_priority_order() {
        let hits = vec![
            hit_repo("neovim", "0.12.3-1", "world"),
            hit_repo("neovim", "0.12.3-1.1", "extra-x86-64-v3"),
            hit_repo("neovim", "0.12.3-1", "extra"),
        ];
        let idx = InstalledIndex::default();
        let rows = merge(hits, &idx, false);
        assert_eq!(rows.len(), 1);
        let badges: Vec<&str> = rows[0].providers.iter().map(|p| p.badge()).collect();
        assert_eq!(badges, vec!["world", "extra-x86-64-v3", "extra"]);
        // First provider is the highest-priority repo (what pacman installs).
        assert_eq!(rows[0].providers[0].version, "0.12.3-1");
    }

    #[test]
    fn match_range_finds_case_insensitive_substring() {
        assert_eq!(match_range("Thunar", "na"), Some((3, 5)));
        assert_eq!(match_range("firefox", "FIRE"), Some((0, 4)));
        assert_eq!(match_range("snappy", "na"), Some((1, 3)));
        assert_eq!(match_range("firefox", "zzz"), None);
        assert_eq!(match_range("firefox", ""), None);
    }

    #[test]
    fn relevance_orders_exact_prefix_substring() {
        let idx = InstalledIndex::default();
        let hits = vec![
            hit("xfirefox", SourceId::Aur, "1", ""),
            hit("firefox-bin", SourceId::Aur, "1", ""),
            hit("firefox", SourceId::Pacman, "1", ""),
        ];
        let mut rows = merge(hits, &idx, false);
        relevance_sort("firefox", &mut rows);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["firefox", "firefox-bin", "xfirefox"]);
    }
}
