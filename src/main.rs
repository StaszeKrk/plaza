// Many model fields/types are consumed by the TUI plan; allow during build-up.
#![allow(dead_code)]

mod action;
mod app;
mod event;
mod model;
mod search;
mod sources;

use crate::search::aggregator::{merge, relevance_sort};
use crate::sources::installed::InstalledIndex;
use tokio::process::Command;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--search") {
        let term = args.get(pos + 1).cloned().unwrap_or_default();
        if term.is_empty() {
            eprintln!("usage: plaza --search <term>");
            std::process::exit(2);
        }
        return run_search_cli(&term).await;
    }
    println!("plaza: TUI not yet implemented (use --search <term>)");
    Ok(())
}

async fn installed_index() -> InstalledIndex {
    match Command::new("pacman").arg("-Q").output().await {
        Ok(out) => InstalledIndex::from_query_output(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => InstalledIndex::default(),
    }
}

async fn run_search_cli(term: &str) -> anyhow::Result<()> {
    let sources = sources::detect_sources();
    if sources.is_empty() {
        eprintln!("plaza: no supported package sources detected (need pacman and/or yay)");
        std::process::exit(1);
    }

    // Query all sources concurrently.
    let mut handles = Vec::new();
    for src in sources {
        let term = term.to_string();
        handles.push(tokio::spawn(async move {
            (src.display_name().to_string(), src.search(&term).await)
        }));
    }

    let mut all_hits = Vec::new();
    for h in handles {
        let (name, result) = h.await?;
        match result {
            Ok(hits) => all_hits.extend(hits),
            Err(e) => eprintln!("plaza: source '{name}' failed: {e}"),
        }
    }

    let idx = installed_index().await;
    let mut rows = merge(all_hits, &idx);
    relevance_sort(term, &mut rows);

    for row in &rows {
        let badges: Vec<&str> = row.providers.iter().map(|p| p.source_id.badge()).collect();
        let installed = if row.any_installed() { " [installed]" } else { "" };
        println!("{:<35} [{}]{}", row.name, badges.join(","), installed);
    }
    println!("\n{} packages", rows.len());
    Ok(())
}
