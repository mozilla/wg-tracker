use crate::config::Config;
use crate::query;
use crate::repo_config::RepoConfig;
use failure::{format_err, Error, ResultExt};
use std::collections::{HashSet, VecDeque};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

#[derive(Default, Deserialize, Serialize)]
pub struct State {
    tasks: VecDeque<Task>,
    handled_comments: HashSet<String>,
    last_time: String,
}

#[derive(Clone, Deserialize, Serialize)]
enum Task {
    QueryIssues(QueryIssuesTask),
    QueryIssueComments(QueryIssueCommentsTask),
    ProcessComment(ProcessCommentTask),
    FileIssue(FileIssueTask),
}

#[derive(Clone, Deserialize, Serialize)]
struct QueryIssuesTask {
    since: String,
    after: Option<String>,
    so_far: Vec<query::UpdatedIssue>,
}

#[derive(Clone, Deserialize, Serialize)]
struct QueryIssueCommentsTask {
    number: i64,
    since: String,
    after: Option<String>,
    so_far: Vec<query::IssueComment>,
}

#[derive(Clone, Deserialize, Serialize)]
struct ProcessCommentTask {
    issue_number: i64,
    issue_title: String,
    url: String,
    body_text: String,
}

#[derive(Clone, Deserialize, Serialize)]
struct FileIssueTask {
    issue_number: i64,
    issue_title: String,
    comment_url: String,
    resolutions: Vec<String>,
}

impl State {
    pub fn new(date: &str) -> State {
        State {
            tasks: VecDeque::new(),
            handled_comments: HashSet::new(),
            last_time: format!("{}T00:00:00Z", date),
        }
    }

    pub fn from_versioned_str(version: u32, json: &str) -> Result<State, Error> {
        if version != 1 {
            return Err(format_err!("unknown state file version number {}", version));
        }
        Ok(serde_json::from_str(json)
            .context(format!("could not parse state file v{}", version))?)
    }

    pub fn check_for_updates(&mut self) {
        let task = Task::QueryIssues(QueryIssuesTask {
            since: self.last_time.clone(),
            after: None,
            so_far: Vec::new(),
        });
        self.tasks.push_back(task);
    }

    pub fn save(&self, path: &Path, temp_path: &Path) -> Result<(), Error> {
        {
            let mut file =
                File::create(temp_path).context("could not create temporary state file")?;
            writeln!(file, "1").context("could not write temporary state file")?;
            serde_json::to_writer_pretty(&mut file, self)
                .context("could not write temporary state file")?;
        }
        fs::rename(temp_path, path).context("could not write state file")?;
        Ok(())
    }

    pub fn iterate(&mut self, config: &Config, repo_config: &RepoConfig) -> Result<(), Error> {
        if self.tasks.is_empty() {
            return Ok(());
        }

        match self.tasks.front().cloned().unwrap() {
            Task::QueryIssues(t) => self.do_query_issues(config, repo_config, t)?,
            Task::QueryIssueComments(t) => self.do_query_issue_comments(config, repo_config, t)?,
            Task::ProcessComment(t) => self.do_process_comment(config, repo_config, t)?,
            _ => {}
        }

        self.tasks.pop_front();
        Ok(())
    }

    fn do_query_issues(
        &mut self,
        config: &Config,
        repo_config: &RepoConfig,
        t: QueryIssuesTask,
    ) -> Result<(), Error> {
        let result = query::updated_issues(
            &config.github_key,
            &config.wg_repo_owner,
            &config.wg_repo_name,
            &t.since,
            t.after.as_ref().map(|s| &**s),
        )?;

        let since = t.since;
        let mut issues = t.so_far;
        let got_any = !result.issues.is_empty();
        issues.extend(result.issues.into_iter());
        let have_more = issues.len() < result.total_count as usize && got_any;

        if have_more {
            self.tasks.push_back(Task::QueryIssues(QueryIssuesTask {
                since,
                after: issues.last().map(|i| i.cursor.clone()),
                so_far: issues,
            }));
            return Ok(());
        }

        if let Some(issue) = issues.last() {
            self.last_time = issue.updated_at.clone();
        }

        self.tasks.extend(issues.into_iter().map(|issue| {
            Task::QueryIssueComments(QueryIssueCommentsTask {
                number: issue.issue_number,
                since: since.clone(),
                after: None,
                so_far: Vec::new(),
            })
        }));

        Ok(())
    }

    fn do_query_issue_comments(
        &mut self,
        config: &Config,
        repo_config: &RepoConfig,
        t: QueryIssueCommentsTask,
    ) -> Result<(), Error> {
        let result = query::issue_comments(
            &config.github_key,
            &config.wg_repo_owner,
            &config.wg_repo_name,
            t.number,
            t.after.as_ref().map(|s| &**s),
        )?;

        let since = t.since;
        let number = t.number;
        let title = result.issue_title;
        let mut comments = t.so_far;
        let got_any = !result.comments.is_empty();
        comments.extend(result.comments.into_iter());
        let have_more = comments.len() < result.total_count as usize && got_any;

        if have_more {
            self.tasks
                .push_back(Task::QueryIssueComments(QueryIssueCommentsTask {
                    number: t.number,
                    since: since,
                    after: comments.last().map(|i| i.cursor.clone()),
                    so_far: comments,
                }));
            return Ok(());
        }

        self.tasks.extend(
            comments
                .into_iter()
                .filter(|comment| comment.created_at >= since)
                .map(|comment| {
                    Task::ProcessComment(ProcessCommentTask {
                        issue_number: number,
                        issue_title: title.clone(),
                        url: comment.url,
                        body_text: comment.body_text,
                    })
                }),
        );

        Ok(())
    }

    fn do_process_comment(
        &mut self,
        config: &Config,
        repo_config: &RepoConfig,
        t: ProcessCommentTask,
    ) -> Result<(), Error> {
        const PREFIX: &'static str = "RESOLVED: ";

        let resolutions = t
            .body_text
            .lines()
            .filter(|line| line.starts_with(PREFIX))
            .map(|line| line[PREFIX.len()..].to_string())
            .collect::<Vec<_>>();

        if resolutions.is_empty() {
            return Ok(());
        }

        if self.handled_comments.contains(&t.url) {
            return Ok(());
        }

        self.handled_comments.insert(t.url.clone());

        self.tasks.push_back(Task::FileIssue(FileIssueTask {
            issue_number: t.issue_number,
            issue_title: t.issue_title,
            comment_url: t.url,
            resolutions,
        }));

        Ok(())
    }

    pub fn is_finished(&self) -> bool {
        self.tasks.is_empty()
    }
}
