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

trait PaginatedQueryBase: GraphQLQuery {
    type Edge: EdgeCursor;
    type Item;

    fn get_total_and_edges(data: Self::ResponseData) -> Option<(i64, Vec<Option<Self::Edge>>)>;
}

trait PaginatedQuery: PaginatedQueryBase {
    fn make_item(edge: Self::Edge) -> Option<Self::Item>;
}

trait EdgeCursor {
    fn cursor(&self) -> String;
}

trait PaginatedQueryVariables {
    fn clone_with_after(&self, after: Option<String>) -> Self;
}

macro_rules! paginated_query {
    (
        query => $ty:ty,
        item => $item:ty,
        edges => $edges:ty,
        path => ($($path:tt)+),
    ) => {
        impl EdgeCursor for $edges {
            fn cursor(&self) -> String {
                self.cursor.clone()
            }
        }

        impl PaginatedQueryVariables for <$ty as GraphQLQuery>::Variables {
            fn clone_with_after(&self, after: Option<String>) -> Self {
                let mut v = self.clone();
                v.after = after.clone();
                v
            }
        }

        impl PaginatedQueryBase for $ty {
            type Edge = $edges;
            type Item = $item;

            fn get_total_and_edges(
                data: <Self as GraphQLQuery>::ResponseData,
            ) -> Option<(i64, Vec<Option<Self::Edge>>)> {
                let items = data.$($path)+;
                Some((items.total_count, items.edges?))
            }
        }
    };
}

fn perform_paginated_query<P>(token: &str, variables: P::Variables) -> Result<Vec<P::Item>, Error>
where
    P: PaginatedQuery,
    P::Variables: PaginatedQueryVariables,
{
    let mut result = Vec::new();
    let mut after = None;
    let mut total_count;

    loop {
        let response_data = perform_query::<P>(token, variables.clone_with_after(after))?;
        let (count, edges) = P::get_total_and_edges(response_data)
            .ok_or_else(|| format_err!("error parsing paginated query response"))?;
        if edges.is_empty() {
            break;
        }
        total_count = count;
        after = edges.last().unwrap().as_ref().map(|e| e.cursor());
        result.extend(edges.into_iter().flatten().map(P::make_item).flatten());
        if result.len() >= total_count as usize {
            break;
        }
    }

    Ok(result)
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github_schema.graphql",
    query_path = "src/query/updated_issues.graphql",
    response_derives = "Clone, Debug"
)]
struct UpdatedIssues;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdatedIssue {
    pub id: String,
    pub issue_number: i64,
    pub issue_title: String,
    pub updated_at: String,
    pub issue_labels: Vec<IssueLabel>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IssueLabel {
    pub name: String,
    pub color: String,
}

paginated_query! {
    query => UpdatedIssues,
    item => UpdatedIssue,
    edges => updated_issues::UpdatedIssuesRepositoryIssuesEdges,
    path => (repository?.issues),
}

impl PaginatedQuery for UpdatedIssues {
    fn make_item(edge: Self::Edge) -> Option<Self::Item> {
        edge.node.map(|issue| UpdatedIssue {
            id: issue.id,
            issue_number: issue.number,
            issue_title: issue.title,
            updated_at: issue.updated_at,
            issue_labels: {
                issue
                    .labels
                    .and_then(|x| x.edges)
                    .into_iter()
                    .flatten()
                    .flat_map(|e| e?.node)
                    .map(|label| IssueLabel {
                        name: label.name,
                        color: label.color,
                    })
                    .collect()
            },
        })
    }
}

pub fn updated_issues(
    token: &str,
    wg_repo_owner: &str,
    wg_repo_name: &str,
    since: &str,
) -> Result<Vec<UpdatedIssue>, Error> {
    perform_paginated_query::<UpdatedIssues>(
        token,
        updated_issues::Variables {
            repo_owner: wg_repo_owner.to_string(),
            repo_name: wg_repo_name.to_string(),
            since: since.to_string(),
            after: None,
        },
    )
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github_schema.graphql",
    query_path = "src/query/issue_comments.graphql",
    response_derives = "Clone, Debug"
)]
struct IssueComments;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IssueComment {
    pub url: String,
    pub created_at: String,
    pub body_text: String,
}

paginated_query! {
    query => IssueComments,
    item => IssueComment,
    edges => issue_comments::IssueCommentsRepositoryIssueCommentsEdges,
    path => (repository?.issue?.comments),
}

impl PaginatedQuery for IssueComments {
    fn make_item(edge: Self::Edge) -> Option<Self::Item> {
        edge.node.map(|n| IssueComment {
            created_at: n.created_at,
            url: n.url,
            body_text: n.body_text,
        })
    }
}

pub fn issue_comments(
    token: &str,
    wg_repo_owner: &str,
    wg_repo_name: &str,
    number: i64,
) -> Result<Vec<IssueComment>, Error> {
    perform_paginated_query::<IssueComments>(
        token,
        issue_comments::Variables {
            repo_owner: wg_repo_owner.to_string(),
            repo_name: wg_repo_name.to_string(),
            number,
            after: None,
        },
    )
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github_schema.graphql",
    query_path = "src/query/known_labels.graphql",
    response_derives = "Clone, Debug"
)]
struct KnownLabels;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KnownLabel {
    pub id: String,
    pub name: String,
}

paginated_query! {
    query => KnownLabels,
    item => KnownLabel,
    edges => known_labels::KnownLabelsRepositoryLabelsEdges,
    path => (repository?.labels?),
}

impl PaginatedQuery for KnownLabels {
    fn make_item(edge: Self::Edge) -> Option<Self::Item> {
        edge.node.map(|n| KnownLabel {
            id: n.id,
            name: n.name,
        })
    }
}

pub fn known_labels(
    token: &str,
    repo_owner: &str,
    repo_name: &str,
) -> Result<Vec<KnownLabel>, Error> {
    perform_paginated_query::<KnownLabels>(
        token,
        known_labels::Variables {
            repo_owner: repo_owner.to_string(),
            repo_name: repo_name.to_string(),
            after: None,
        },
    )
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

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github_schema.graphql",
    query_path = "src/query/remove_labels.graphql",
    response_derives = "Debug"
)]
struct RemoveLabels;

pub fn remove_labels(token: &str, labelable: String, labels: Vec<String>) -> Result<(), Error> {
    perform_query::<RemoveLabels>(token, remove_labels::Variables { labelable, labels })?;

    Ok(())
}

const GITHUB_ENDPOINT: &'static str = "https://api.github.com/graphql";
