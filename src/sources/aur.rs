use crate::model::{PackageHit, SourceId, SourceMeta};
use serde::Deserialize;

#[derive(Deserialize)]
struct RpcResponse {
    results: Vec<RpcResult>,
}

#[derive(Deserialize)]
struct RpcResult {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Version")]
    version: String,
    #[serde(rename = "Description")]
    description: Option<String>,
    #[serde(rename = "NumVotes")]
    num_votes: Option<u32>,
    #[serde(rename = "Maintainer")]
    maintainer: Option<String>,
    #[serde(rename = "OutOfDate")]
    out_of_date: Option<i64>,
}

/// Parse an AUR RPC v5 `search` response body into hits.
pub fn parse_rpc_response(body: &str) -> anyhow::Result<Vec<PackageHit>> {
    let resp: RpcResponse = serde_json::from_str(body)?;
    Ok(resp
        .results
        .into_iter()
        .map(|r| PackageHit {
            name: r.name,
            version: r.version,
            source_id: SourceId::Aur,
            description: r.description.unwrap_or_default(),
            meta: SourceMeta {
                votes: r.num_votes,
                maintained: r.maintainer.is_some(),
                out_of_date: r.out_of_date.is_some(),
                repo: None,
            },
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
      "resultcount": 2,
      "results": [
        {"Name":"firefox-bin","Version":"141.0-1","Description":"Standalone web browser - binary","NumVotes":120,"Maintainer":"alice","OutOfDate":null},
        {"Name":"firefox-git","Version":"142.0.r1-1","Description":"Standalone web browser - git","NumVotes":5,"Maintainer":null,"OutOfDate":1700000000}
      ],
      "type":"search",
      "version":5
    }"#;

    #[test]
    fn parses_results_with_meta() {
        let hits = parse_rpc_response(FIXTURE).unwrap();
        assert_eq!(hits.len(), 2);

        assert_eq!(hits[0].name, "firefox-bin");
        assert_eq!(hits[0].version, "141.0-1");
        assert_eq!(hits[0].source_id, SourceId::Aur);
        assert_eq!(hits[0].meta.votes, Some(120));
        assert!(hits[0].meta.maintained);
        assert!(!hits[0].meta.out_of_date);

        assert_eq!(hits[1].name, "firefox-git");
        assert!(!hits[1].meta.maintained); // Maintainer null
        assert!(hits[1].meta.out_of_date); // OutOfDate set
    }

    #[test]
    fn empty_results() {
        let body = r#"{"resultcount":0,"results":[],"type":"search","version":5}"#;
        assert!(parse_rpc_response(body).unwrap().is_empty());
    }
}
