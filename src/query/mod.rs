use crate::util::CLIENT;
use failure::{format_err, Error, ResultExt};
use graphql_client::*;

type DateTime = String;
type URI = String;

fn do_perform_query<Q>(
    token: &str,
    mime_type: Option<&str>,
    variables: Q::Variables,
) -> Result<Q::ResponseData, Error>
where
    Q: GraphQLQuery,
{
    let mut request = CLIENT.post(GITHUB_ENDPOINT).bearer_auth(token);

    if let Some(mime_type) = mime_type {
        request = request.header("Accept", mime_type);
    }

    let response = request
        .json(&Q::build_query(variables))
        .send()
        .context("could not perform network request")?
        .json::<Response<Q::ResponseData>>()
        .context("could not parse response")?;

    if let Some(errors) = response.errors {
        return Err(format_err!("errors in response: {:?}", errors));
    }

    response
        .data
        .ok_or_else(|| format_err!("no data in response"))
}

fn perform_query<Q>(token: &str, variables: Q::Variables) -> Result<Q::ResponseData, Error>
where
    Q: GraphQLQuery,
{
    do_perform_query::<Q>(token, None, variables)
}

fn perform_query_with_preview<Q>(
    token: &str,
    mime_type: &str,
    variables: Q::Variables,
) -> Result<Q::ResponseData, Error>
where
    Q: GraphQLQuery,
{
    do_perform_query::<Q>(token, Some(mime_type), variables)
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github_schema.graphql",
    query_path = "src/query/updated_issues.graphql",
    response_derives = "Debug"
)]
struct UpdatedIssues;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdatedIssue {
    pub issue_number: i64,
    pub updated_at: String,
    pub issue_labels: Vec<IssueLabel>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IssueLabel {
    pub name: String,
    pub color: String,
}

pub fn updated_issues(
    token: &str,
    wg_repo_owner: &str,
    wg_repo_name: &str,
    since: &str,
) -> Result<Vec<UpdatedIssue>, Error> {
    let mut result = Vec::new();
    let mut after = None;
    let mut total_count;

    loop {
        let data = perform_query::<UpdatedIssues>(
            token,
            updated_issues::Variables {
                repo_owner: wg_repo_owner.to_string(),
                repo_name: wg_repo_name.to_string(),
                since: since.to_string(),
                after: after.clone(),
            },
        )?;

        let issues = data
            .repository
            .ok_or_else(|| format_err!("repository not found"))?
            .issues;

        total_count = issues.total_count;

        let edges = issues
            .edges
            .ok_or_else(|| format_err!("issue edges not found"))?;

        if edges.is_empty() {
            break;
        }

        after = edges.last().unwrap().as_ref().map(|e| e.cursor.clone());
        result.extend(
            edges
                .into_iter()
                .flatten()
                .flat_map(|e| e.node)
                .map(|n| {
                    let mut issue_labels = Vec::new();
                    if let Some(labels) = n.labels {
                        if let Some(edges) = labels.edges {
                            for edge in edges.into_iter().flatten() {
                                if let Some(label) = edge.node {
                                    issue_labels.push(IssueLabel {
                                        name: label.name,
                                        color: label.color,
                                    });
                                }
                            }
                        }
                    }
                    UpdatedIssue {
                        issue_number: n.number,
                        updated_at: n.updated_at,
                        issue_labels,
                    }
                })
        );

        if result.len() >= total_count as usize {
            break;
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
    pub comments: Vec<IssueComment>,
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
    after: Option<String>,
) -> Result<IssueCommentsResult, Error> {
    let data = perform_query::<IssueComments>(
        token,
        issue_comments::Variables {
            repo_owner: wg_repo_owner.to_string(),
            repo_name: wg_repo_name.to_string(),
            number,
            after,
        },
    )?;

    let mut result: IssueCommentsResult = Default::default();
    if let Some(issue) = data.repository.and_then(|r| r.issue) {
        result.issue_title = issue.title;
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KnownLabel {
    pub id: String,
    pub name: String,
}

pub fn known_labels(
    token: &str,
    repo_owner: &str,
    repo_name: &str,
) -> Result<Vec<KnownLabel>, Error> {
    let mut result = Vec::new();
    let mut after = None;
    let mut total_count;

    loop {
        let data = perform_query::<KnownLabels>(
            token,
            known_labels::Variables {
                repo_owner: repo_owner.to_string(),
                repo_name: repo_name.to_string(),
                after: after.clone(),
            },
        )?;

        let labels = data
            .repository
            .ok_or_else(|| format_err!("repository not found"))?
            .labels
            .ok_or_else(|| format_err!("labels not found"))?;

        total_count = labels.total_count;

        let edges = labels
            .edges
            .ok_or_else(|| format_err!("label edges not found"))?;

        if edges.is_empty() {
            break;
        }

        after = edges.last().unwrap().as_ref().map(|e| e.cursor.clone());
        result.extend(
            edges
                .into_iter()
                .flatten()
                .flat_map(|e| e.node)
                .map(|n| KnownLabel { id: n.id, name: n.name }),
        );

        if result.len() >= total_count as usize {
            break;
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
    let data = perform_query::<RepoID>(
        token,
        repo_id::Variables {
            repo_owner: repo_owner.to_string(),
            repo_name: repo_name.to_string(),
        },
    )?;

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
    let data = perform_query_with_preview::<CreateLabel>(
        token,
        "application/vnd.github.bane-preview+json",
        create_label::Variables {
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            color: color.to_string(),
        },
    )?;

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
    let data = perform_query::<CreateIssue>(
        token,
        create_issue::Variables {
            repo_id: repo_id.to_string(),
            title,
            body,
            labels,
        },
    )?;

    data.create_issue
        .and_then(|m| m.issue)
        .map(|i| i.id)
        .ok_or_else(|| format_err!("issue creation failed"))
}

const GITHUB_ENDPOINT: &'static str = "https://api.github.com/graphql";
