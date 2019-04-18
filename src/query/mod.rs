use failure::{format_err, Error, ResultExt};
use graphql_client::*;
use lazy_static::lazy_static;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;
use std::fmt;

type DateTime = String;
type URI = String;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github_schema.graphql",
    query_path = "src/query/updated_issues.graphql",
    response_derives = "Debug"
)]
struct UpdatedIssues;

#[derive(Debug, Default)]
pub struct UpdatedIssuesResult {
    pub total_count: i64,
    pub issues: Vec<UpdatedIssue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdatedIssue {
    pub cursor: String,
    pub issue_number: i64,
    pub updated_at: String,
}

pub fn updated_issues(
    token: &str,
    wg_repo_owner: &str,
    wg_repo_name: &str,
    since: &str,
    after: Option<&str>,
) -> Result<UpdatedIssuesResult, Error> {
    let q = UpdatedIssues::build_query(updated_issues::Variables {
        repo_owner: wg_repo_owner.to_string(),
        repo_name: wg_repo_name.to_string(),
        since: since.to_string(),
        after: after.map(|s| s.to_string()),
    });

    let response = CLIENT
        .post(GITHUB_ENDPOINT)
        .bearer_auth(token)
        .json(&q)
        .send()
        .context("could not perform network request")?
        .json::<Response<updated_issues::ResponseData>>()
        .context("could not parse response")?;

    if let Some(errors) = response.errors {
        return Err(format_err!("errors in response: {:?}", errors));
    }

    let data = response
        .data
        .ok_or_else(|| format_err!("no data in response"))?;

    let mut result: UpdatedIssuesResult = Default::default();
    if let Some(issues) = data.repository.map(|r| r.issues) {
        result.total_count = issues.total_count;
        if let Some(edges) = issues.edges {
            for edge in edges.into_iter().flatten() {
                let cursor = edge.cursor;
                if let Some(issue) = edge.node {
                    result.issues.push(UpdatedIssue {
                        cursor,
                        issue_number: issue.number,
                        updated_at: issue.updated_at,
                    });
                }
            }
        }
    }

    Ok(result)
}

lazy_static! {
    static ref CLIENT: reqwest::Client = reqwest::Client::new();
}

const GITHUB_ENDPOINT: &'static str = "https://api.github.com/graphql";
