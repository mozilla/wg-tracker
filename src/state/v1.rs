use crate::config::Config;
use crate::query;
use crate::repo_config::RepoConfig;
use crate::util::escape_markdown;
use failure::{format_err, Error, ResultExt};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::Write;
use std::mem;
use std::path::Path;

#[derive(Default, Deserialize, Serialize)]
pub struct State {
    tasks: VecDeque<Task>,
    posted_tasks: Vec<Task>,
    handled_comments: HashSet<String>,
    #[serde(skip)]
    known_labels: Option<HashMap<String, String>>,
    #[serde(skip)]
    decisions_repo_id: Option<String>,
    last_time: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
enum Task {
    QueryIssues(QueryIssuesTask),
    QueryIssueComments(QueryIssueCommentsTask),
    ProcessComment(ProcessCommentTask),
    QueryKnownLabels(QueryKnownLabelsTask),
    QueryDecisionsRepoID,
    EnsureLabel(EnsureLabelTask),
    FileIssue(FileIssueTask),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct QueryIssuesTask {
    since: String,
    after: Option<String>,
    so_far: Vec<query::UpdatedIssue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct QueryIssueCommentsTask {
    number: i64,
    since: String,
    after: Option<String>,
    so_far: Vec<query::IssueComment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ProcessCommentTask {
    issue_number: i64,
    issue_title: String,
    issue_labels: Vec<query::IssueLabel>,
    url: String,
    body_text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct IssueLabel {
    name: String,
    color: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct QueryKnownLabelsTask {
    so_far: Vec<query::KnownLabel>,
    after: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EnsureLabelTask {
    name: String,
    color: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FileIssueTask {
    issue_number: i64,
    issue_title: String,
    issue_labels: Vec<String>,
    comment_url: String,
    resolutions: Vec<String>,
}

impl State {
    pub fn new(date: &str) -> State {
        State {
            tasks: VecDeque::new(),
            posted_tasks: Vec::new(),
            handled_comments: HashSet::new(),
            known_labels: None,
            decisions_repo_id: None,
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
        if !self.posted_tasks.is_empty() {
            let mut new_tasks = Vec::new();
            mem::swap(&mut new_tasks, &mut self.posted_tasks);
            new_tasks.extend(self.tasks.drain(..));
            self.tasks.extend(new_tasks.drain(..));
        }

        if self.tasks.is_empty() {
            return Ok(());
        }

        match dbg!(self.tasks.front().cloned().unwrap()) {
            Task::QueryIssues(t) => self.do_query_issues(config, repo_config, t)?,
            Task::QueryIssueComments(t) => self.do_query_issue_comments(config, repo_config, t)?,
            Task::ProcessComment(t) => self.do_process_comment(config, repo_config, t)?,
            Task::QueryKnownLabels(t) => self.do_query_known_labels(config, repo_config, t)?,
            Task::QueryDecisionsRepoID => self.do_query_decisions_repo_id(config, repo_config)?,
            Task::EnsureLabel(t) => self.do_ensure_label(config, repo_config, t)?,
            Task::FileIssue(t) => self.do_file_issue(config, repo_config, t)?,
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
            self.post_task(Task::QueryIssues(QueryIssuesTask {
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
        let labels = result.issue_labels;
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
                        issue_labels: labels.clone(),
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

        let mut desired_labels = Vec::new();
        if let Some(labels_config) = &repo_config.labels {
            for label in t.issue_labels {
                if let Some(color) = &labels_config.color {
                    if label.color == *color {
                        desired_labels.push(label);
                        continue;
                    }
                }
                if let Some(prefixes) = &labels_config.prefixes {
                    for prefix in prefixes {
                        if label.name.starts_with(prefix) {
                            desired_labels.push(label);
                            break;
                        }
                    }
                }
            }
        }

        for label in &desired_labels {
            self.post_task(Task::EnsureLabel(EnsureLabelTask {
                name: format!("[spec] {}", label.name),
                color: label.color.clone(),
            }));
        }

        self.post_task(Task::FileIssue(FileIssueTask {
            issue_number: t.issue_number,
            issue_title: t.issue_title,
            issue_labels: desired_labels
                .into_iter()
                .map(|l| format!("[spec] {}", l.name))
                .collect(),
            comment_url: t.url,
            resolutions,
        }));

        Ok(())
    }

    fn do_query_known_labels(
        &mut self,
        config: &Config,
        repo_config: &RepoConfig,
        t: QueryKnownLabelsTask,
    ) -> Result<(), Error> {
        let result = query::known_labels(
            &config.github_key,
            &config.decisions_repo_owner,
            &config.decisions_repo_name,
            t.after.as_ref().map(|s| &**s),
        )?;

        let mut known_labels = t.so_far;
        let got_any = !result.known_labels.is_empty();
        known_labels.extend(result.known_labels.into_iter());
        let have_more = known_labels.len() < result.total_count as usize && got_any;

        if have_more {
            self.post_task(Task::QueryKnownLabels(QueryKnownLabelsTask {
                after: known_labels.last().map(|l| l.cursor.clone()),
                so_far: known_labels,
            }));
            return Ok(());
        }

        if self.known_labels.is_none() {
            self.known_labels = Some(HashMap::new());
        }

        let map = self.known_labels.as_mut().unwrap();

        for label in known_labels {
            map.insert(label.name, label.id);
        }

        Ok(())
    }

    fn do_ensure_label(
        &mut self,
        config: &Config,
        repo_config: &RepoConfig,
        t: EnsureLabelTask,
    ) -> Result<(), Error> {
        if self.known_labels.is_none() {
            self.post_task(Task::QueryKnownLabels(QueryKnownLabelsTask {
                so_far: Vec::new(),
                after: None,
            }));
            self.post_task(Task::EnsureLabel(t));
            return Ok(());
        }

        if self.decisions_repo_id.is_none() {
            self.post_task(Task::QueryDecisionsRepoID);
            self.post_task(Task::EnsureLabel(t));
            return Ok(());
        }

        if self.known_labels.as_ref().unwrap().contains_key(&t.name) {
            return Ok(());
        }

        let result = query::create_label(
            &config.github_key,
            self.decisions_repo_id.as_ref().unwrap(),
            &t.name,
            &t.color,
        )?;

        Ok(())
    }

    fn do_query_decisions_repo_id(
        &mut self,
        config: &Config,
        repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        let result = query::repo_id(
            &config.github_key,
            &config.decisions_repo_owner,
            &config.decisions_repo_name,
        )?;

        if result.is_none() {
            return Err(format_err!("repository not found"));
        }

        self.decisions_repo_id = result;

        Ok(())
    }

    fn do_file_issue(
        &mut self,
        config: &Config,
        repo_config: &RepoConfig,
        t: FileIssueTask,
    ) -> Result<(), Error> {
        if self.known_labels.is_none() {
            self.post_task(Task::QueryKnownLabels(QueryKnownLabelsTask {
                so_far: Vec::new(),
                after: None,
            }));
            self.post_task(Task::FileIssue(t));
            return Ok(());
        }

        if self.decisions_repo_id.is_none() {
            self.post_task(Task::QueryDecisionsRepoID);
            self.post_task(Task::FileIssue(t));
            return Ok(());
        }

        let plural = if t.resolutions.len() == 1 {
            "A resolution was"
        } else {
            "Resolutions were"
        };
        let issue_url = format!(
            "https://github.com./{}/{}/issues/{}",
            config.wg_repo_owner, config.wg_repo_name, t.issue_number,
        );
        let body = format!(
            "{} made for [{}/#{}]({}).\n\
             \n\
             **{}**\n\
             \n\
             {}\n\
             \n\
             [Discussion.]({})\n\
             \n\
             ----\n\
             \n\
             To file a bug automatically for these resolutions, add the **bug** \
             label to the issue.\n\
             \n\
             If no bug is needed, the issue can be closed.",
            plural,
            config.wg_repo_name,
            t.issue_number,
            issue_url,
            escape_markdown(&t.issue_title),
            t.resolutions
                .into_iter()
                .map(|s| format!("* RESOLVED: {}\n", escape_markdown(&s)))
                .collect::<String>(),
            t.comment_url,
        );

        let label_ids = t
            .issue_labels
            .into_iter()
            .flat_map(|s| self.known_labels.as_ref().unwrap().get(&s))
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let result = query::create_issue(
            &config.github_key,
            self.decisions_repo_id.as_ref().unwrap(),
            t.issue_title,
            Some(body),
            Some(label_ids),
        )?;

        Ok(())
    }

    pub fn is_finished(&self) -> bool {
        self.tasks.is_empty()
    }

    fn post_task(&mut self, task: Task) {
        self.posted_tasks.push(task);
    }
}
