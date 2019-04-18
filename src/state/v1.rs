use crate::config::Config;
use crate::query;
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
}

#[derive(Clone, Deserialize, Serialize)]
struct ProcessCommentTask {
    url: String,
    body: String,
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

    pub fn iterate(&mut self, config: &Config) -> Result<(), Error> {
        if self.tasks.is_empty() {
            return Ok(());
        }

        match self.tasks.front().cloned().unwrap() {
            Task::QueryIssues(t) => self.do_query_issues(config, t)?,
            _ => {}
        }

        self.tasks.pop_front();
        Ok(())
    }

    fn do_query_issues(&mut self, config: &Config, t: QueryIssuesTask) -> Result<(), Error> {
        let result = query::updated_issues(
            &config.github_key,
            &config.wg_repo_owner,
            &config.wg_repo_name,
            &t.since,
            t.after.as_ref().map(|s| &**s),
        )?;

        let since = t.since;
        let mut issues = t.so_far;
        let have_more = issues.len() < result.total_count as usize && !result.issues.is_empty();
        issues.extend(result.issues.into_iter());

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
            })
        }));

        Ok(())
    }

    pub fn is_finished(&self) -> bool {
        self.tasks.is_empty()
    }
}
