use crate::model::{Action, CommandLine, PackageDetail, PackageHit, SourceId, SourceMeta};
use crate::sources::Source;
use async_trait::async_trait;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde::Deserialize;

pub struct AurSource {
    client: reqwest::Client,
}

impl AurSource {
    pub fn new() -> Self {
        AurSource {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Source for AurSource {
    fn id(&self) -> SourceId {
        SourceId::Aur
    }

    fn display_name(&self) -> &'static str {
        "aur"
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<PackageHit>> {
        let encoded = utf8_percent_encode(query, NON_ALPHANUMERIC).to_string();
        let url = format!("https://aur.archlinux.org/rpc/v5/search/{encoded}?by=name-desc");
        let body = self.client.get(&url).send().await?.text().await?;
        parse_rpc_response(&body)
    }

    fn action_command(&self, _action: Action, pkg: &str) -> CommandLine {
        CommandLine {
            program: "yay".into(),
            args: vec!["-S".into(), pkg.into()],
        }
    }
}

#[derive(Deserialize)]
struct RpcResponse<T> {
    results: Vec<T>,
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
    #[serde(rename = "LastModified")]
    last_modified: Option<i64>,
}

/// Parse an AUR RPC v5 `search` response body into hits.
pub fn parse_rpc_response(body: &str) -> anyhow::Result<Vec<PackageHit>> {
    let resp: RpcResponse<RpcResult> = serde_json::from_str(body)?;
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
                last_modified: r.last_modified,
                canonical_id: None,
            },
        })
        .collect())
}

/// The AUR `info` RPC URL for a single package (full metadata: url, deps,
/// maintainer, popularity, license).
pub fn info_url(name: &str) -> String {
    let encoded = utf8_percent_encode(name, NON_ALPHANUMERIC).to_string();
    format!("https://aur.archlinux.org/rpc/v5/info?arg[]={encoded}")
}

#[derive(Deserialize)]
struct RpcInfoResult {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "Maintainer")]
    maintainer: Option<String>,
    #[serde(rename = "Popularity")]
    popularity: Option<f64>,
    #[serde(rename = "Depends")]
    depends: Option<Vec<String>>,
    #[serde(rename = "OptDepends")]
    opt_depends: Option<Vec<String>>,
    #[serde(rename = "License")]
    license: Option<Vec<String>>,
}

/// Parse an AUR RPC v5 `info` response body into a `PackageDetail` (first
/// result). `None` when the package is unknown (empty results) or the body does
/// not parse.
pub fn parse_info_response(body: &str) -> Option<PackageDetail> {
    let resp: RpcResponse<RpcInfoResult> = serde_json::from_str(body).ok()?;
    let r = resp.results.into_iter().next()?;
    Some(PackageDetail {
        url: r.url.filter(|s| !s.is_empty()),
        repo_url: Some(format!("https://aur.archlinux.org/packages/{}", r.name)),
        licenses: r.license.filter(|l| !l.is_empty()).map(|l| l.join(", ")),
        install_size: None,
        build_date: None,
        depends: r.depends.unwrap_or_default(),
        optional_depends: r.opt_depends.unwrap_or_default(),
        maintainer: r.maintainer,
        popularity: r.popularity,
    })
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

    #[test]
    fn parses_info_detail() {
        let body = r#"{"resultcount":1,"results":[
          {"Name":"firefox-git","URL":"https://www.mozilla.org","Maintainer":"alice",
           "Popularity":0.41,"Depends":["gtk3","nss"],"OptDepends":["ffmpeg: video"],
           "License":["MPL-2.0","GPL"]}
        ],"type":"multiinfo","version":5}"#;
        let d = parse_info_response(body).unwrap();
        assert_eq!(d.url.as_deref(), Some("https://www.mozilla.org"));
        assert_eq!(d.maintainer.as_deref(), Some("alice"));
        assert_eq!(d.popularity, Some(0.41));
        assert_eq!(d.depends, vec!["gtk3", "nss"]);
        assert_eq!(d.optional_depends, vec!["ffmpeg: video"]);
        assert_eq!(d.licenses.as_deref(), Some("MPL-2.0, GPL"));
        assert_eq!(
            d.repo_url.as_deref(),
            Some("https://aur.archlinux.org/packages/firefox-git")
        );
    }

    #[test]
    fn info_empty_results_is_none() {
        let body = r#"{"resultcount":0,"results":[],"type":"multiinfo","version":5}"#;
        assert!(parse_info_response(body).is_none());
    }

    #[test]
    fn info_url_encodes_name() {
        assert_eq!(
            info_url("c++"),
            "https://aur.archlinux.org/rpc/v5/info?arg[]=c%2B%2B"
        );
    }
}
