use crate::util::CLIENT;
use failure::{format_err, Error, ResultExt};
use graphql_client::*;
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

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github_schema.graphql",
    query_path = "src/query/issue_comments.graphql",
    response_derives = "Debug"
)]
struct IssueComments;

#[derive(Debug, Default)]
pub struct IssueCommentsResult {
    pub total_count: i64,
    pub issue_title: String,
    pub issue_labels: Vec<IssueLabel>,
    pub comments: Vec<IssueComment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IssueLabel {
    pub name: String,
    pub color: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IssueComment {
    pub cursor: String,
    pub url: String,
    pub created_at: String,
    pub body_text: String,
}

pub fn issue_comments(
    token: &str,
    wg_repo_owner: &str,
    wg_repo_name: &str,
    number: i64,
    after: Option<&str>,
) -> Result<IssueCommentsResult, Error> {
    let q = IssueComments::build_query(issue_comments::Variables {
        repo_owner: wg_repo_owner.to_string(),
        repo_name: wg_repo_name.to_string(),
        number,
        after: after.map(|s| s.to_string()),
    });

    let response = CLIENT
        .post(GITHUB_ENDPOINT)
        .bearer_auth(token)
        .json(&q)
        .send()
        .context("could not perform network request")?
        .json::<Response<issue_comments::ResponseData>>()
        .context("could not parse response")?;

    if let Some(errors) = response.errors {
        return Err(format_err!("errors in response: {:?}", errors));
    }

    let data = response
        .data
        .ok_or_else(|| format_err!("no data in response"))?;

    let mut result: IssueCommentsResult = Default::default();
    if let Some(issue) = data.repository.and_then(|r| r.issue) {
        result.issue_title = issue.title;
        if let Some(labels) = issue.labels {
            if let Some(edges) = labels.edges {
                for edge in edges.into_iter().flatten() {
                    if let Some(label) = edge.node {
                        result.issue_labels.push(IssueLabel {
                            name: label.name,
                            color: label.color,
                        });
                    }
                }
            }
        }
        result.total_count = issue.comments.total_count;
        if let Some(edges) = issue.comments.edges {
            for edge in edges.into_iter().flatten() {
                let cursor = edge.cursor;
                if let Some(comment) = edge.node {
                    result.comments.push(IssueComment {
                        cursor,
                        created_at: comment.created_at,
                        url: comment.url,
                        body_text: comment.body_text,
                    });
                }
            }
        }
    }

    Ok(result)
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github_schema.graphql",
    query_path = "src/query/known_labels.graphql",
    response_derives = "Debug"
)]
struct KnownLabels;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct KnownLabelsResult {
    pub total_count: i64,
    pub known_labels: Vec<KnownLabel>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KnownLabel {
    pub cursor: String,
    pub id: String,
    pub name: String,
}

pub fn known_labels(
    token: &str,
    repo_owner: &str,
    repo_name: &str,
    after: Option<&str>,
) -> Result<KnownLabelsResult, Error> {
    let q = KnownLabels::build_query(known_labels::Variables {
        repo_owner: repo_owner.to_string(),
        repo_name: repo_name.to_string(),
        after: after.map(|s| s.to_string()),
    });

    let response = CLIENT
        .post(GITHUB_ENDPOINT)
        .bearer_auth(token)
        .json(&q)
        .send()
        .context("could not perform network request")?
        .json::<Response<known_labels::ResponseData>>()
        .context("could not parse response")?;

    if let Some(errors) = response.errors {
        return Err(format_err!("errors in response: {:?}", errors));
    }

    let data = response
        .data
        .ok_or_else(|| format_err!("no data in response"))?;

    let mut result: KnownLabelsResult = Default::default();
    if let Some(labels) = data.repository.and_then(|r| r.labels) {
        result.total_count = labels.total_count;
        if let Some(edges) = labels.edges {
            for edge in edges.into_iter().flatten() {
                let cursor = edge.cursor;
                if let Some(label) = edge.node {
                    result.known_labels.push(KnownLabel {
                        cursor,
                        id: label.id,
                        name: label.name,
                    });
                }
            }
        }
    }

    Ok(result)
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github_schema.graphql",
    query_path = "src/query/repo_id.graphql",
    response_derives = "Debug"
)]
struct RepoID;

pub fn repo_id(token: &str, repo_owner: &str, repo_name: &str) -> Result<Option<String>, Error> {
    let q = RepoID::build_query(repo_id::Variables {
        repo_owner: repo_owner.to_string(),
        repo_name: repo_name.to_string(),
    });

    let response = CLIENT
        .post(GITHUB_ENDPOINT)
        .bearer_auth(token)
        .json(&q)
        .send()
        .context("could not perform network request")?
        .json::<Response<repo_id::ResponseData>>()
        .context("could not parse response")?;

    if let Some(errors) = response.errors {
        return Err(format_err!("errors in response: {:?}", errors));
    }

    let data = response
        .data
        .ok_or_else(|| format_err!("no data in response"))?;

    Ok(data.repository.map(|r| r.id))
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github_schema.graphql",
    query_path = "src/query/create_label.graphql",
    response_derives = "Debug"
)]
struct CreateLabel;

pub fn create_label(token: &str, repo_id: &str, name: &str, color: &str) -> Result<String, Error> {
    let q = CreateLabel::build_query(create_label::Variables {
        repo_id: repo_id.to_string(),
        name: name.to_string(),
        color: color.to_string(),
    });

    let response = CLIENT
        .post(GITHUB_ENDPOINT)
        .bearer_auth(token)
        .header("Accept", "application/vnd.github.bane-preview+json")
        .json(&q)
        .send()
        .context("could not perform network request")?
        .json::<Response<create_label::ResponseData>>()
        .context("could not parse response")?;

    if let Some(errors) = response.errors {
        return Err(format_err!("errors in response: {:?}", errors));
    }

    let data = response
        .data
        .ok_or_else(|| format_err!("no data in response"))?;

    data.create_label
        .and_then(|m| m.label)
        .map(|l| l.id)
        .ok_or_else(|| format_err!("label creation failed"))
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github_schema.graphql",
    query_path = "src/query/create_issue.graphql",
    response_derives = "Debug"
)]
struct CreateIssue;

pub fn create_issue(
    token: &str,
    repo_id: &str,
    title: String,
    body: Option<String>,
    labels: Option<Vec<String>>,
) -> Result<String, Error> {
    let q = CreateIssue::build_query(create_issue::Variables {
        repo_id: repo_id.to_string(),
        title,
        body,
        labels,
    });

    let response = CLIENT
        .post(GITHUB_ENDPOINT)
        .bearer_auth(token)
        .json(&q)
        .send()
        .context("could not perform network request")?
        .json::<Response<create_issue::ResponseData>>()
        .context("could not parse response")?;

    if let Some(errors) = response.errors {
        return Err(format_err!("errors in response: {:?}", errors));
    }

    let data = response
        .data
        .ok_or_else(|| format_err!("no data in response"))?;

    data.create_issue
        .and_then(|m| m.issue)
        .map(|i| i.id)
        .ok_or_else(|| format_err!("issue creation failed"))
}

const GITHUB_ENDPOINT: &'static str = "https://api.github.com/graphql";
