use crate::config::Config;
use crate::query;
use crate::repo_config::RepoConfig;
use crate::util::{escape_markdown, extract_urls};
use failure::{format_err, Error, ResultExt};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::mem;
use std::path::Path;

#[derive(Default, Deserialize, Serialize)]
pub struct State {
    tasks: VecDeque<Box<dyn Task>>,
    posted_tasks: Vec<Box<dyn Task>>,
    handled_wg_comments: HashSet<String>,
    handled_decisions_issues: HashSet<i64>,
    #[serde(skip)]
    known_labels: Option<HashMap<String, String>>,
    #[serde(skip)]
    decisions_repo_id: Option<String>,
    last_time_wg: String,
    last_time_decisions: String,
}

impl State {
    pub fn new(date: &str) -> State {
        State {
            tasks: VecDeque::new(),
            posted_tasks: Vec::new(),
            handled_wg_comments: HashSet::new(),
            handled_decisions_issues: HashSet::new(),
            known_labels: None,
            decisions_repo_id: None,
            last_time_wg: format!("{}T00:00:00Z", date),
            last_time_decisions: String::from("2019-01-01T00:00:00Z"),
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
        self.tasks.push_back(Box::new(QueryWGIssuesTask {
            since: self.last_time_wg.clone(),
        }));
        self.tasks.push_back(Box::new(QueryDecisionsIssuesTask {
            since: self.last_time_decisions.clone(),
        }));
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

        let task = self.tasks.pop_front().unwrap();
        let result = task.run(self, config, repo_config);

        if result.is_err() {
            self.tasks.push_front(task);
        }

        result
    }

    pub fn is_finished(&self) -> bool {
        self.tasks.is_empty() && self.posted_tasks.is_empty()
    }

    fn post_task<T: Task + 'static>(&mut self, task: T) {
        self.posted_tasks.push(Box::new(task));
    }
}

#[typetag::serde(tag = "type")]
trait Task: fmt::Debug {
    fn run(
        &self,
        state: &mut State,
        config: &Config,
        repo_config: &RepoConfig,
    ) -> Result<(), Error>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct QueryWGIssuesTask {
    since: String,
}

#[typetag::serde]
impl Task for QueryWGIssuesTask {
    fn run(
        &self,
        state: &mut State,
        config: &Config,
        _repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        let issues = query::updated_issues(
            &config.github_key,
            &config.wg_repo_owner,
            &config.wg_repo_name,
            &self.since,
        )?;

        if let Some(issue) = issues.last() {
            state.last_time_wg = issue.updated_at.clone();
        }

        for issue in issues {
            state.post_task(QueryWGIssueCommentsTask {
                number: issue.issue_number,
                issue_title: issue.issue_title.clone(),
                issue_labels: issue.issue_labels,
                since: self.since.clone(),
            });
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct QueryDecisionsIssuesTask {
    since: String,
}

fn parse_component(s: &str) -> Result<(&str, &str), Error> {
    let bits = s.split(" :: ").collect::<Vec<_>>();
    if bits.len() == 2 {
        Ok((bits[0], bits[1]))
    } else {
        Err(format_err!("could not parse component '{}'", s))
    }
}

#[typetag::serde]
impl Task for QueryDecisionsIssuesTask {
    fn run(
        &self,
        state: &mut State,
        config: &Config,
        repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        let issues = query::updated_issues(
            &config.github_key,
            &config.decisions_repo_owner,
            &config.decisions_repo_name,
            &self.since,
        )?;

        if let Some(issue) = issues.last() {
            state.last_time_decisions = issue.updated_at.clone();
        }

        for issue in issues {
            if state.handled_decisions_issues.contains(&issue.issue_number) {
                continue;
            }

            if !issue.issue_labels.iter().any(|label| label.name == "bug") {
                continue;
            }

            state.handled_decisions_issues.insert(issue.issue_number);

            let mut components = Vec::new();
            for label in issue.issue_labels {
                if !label.name.starts_with("[spec] ") {
                    continue;
                }

                let mut spec = String::from(&label.name["[spec] ".len()..]);
                while let Some(c) = spec.pop() {
                    if !(c >= '0' && c <= '9' || c == '-') {
                        spec.push(c);
                        break;
                    }
                }

                if let Some(cs) = &repo_config.components {
                    if let Some(c) = cs.get(&spec) {
                        components.push(c);
                    }
                }
            }

            let product_component = if components.len() == 1 {
                Some(parse_component(&components[0]))
            } else if let Some(cs) = &repo_config.components {
                cs.get("default").map(|c| parse_component(&*c))
            } else {
                None
            };
            let product_component = product_component.unwrap_or(Ok(("Invalid Bugs", "General")))?;

            state.post_task(FileBugForDecisionsIssueTask {
                product: product_component.0.to_string(),
                component: product_component.1.to_string(),
                issue_number: issue.issue_number,
                issue_id: issue.id.clone(),
            });

            state.post_task(RemoveDecisionsIssueBugLabelTask {
                issue_id: issue.id.clone(),
            });
            state.post_task(CloseIssueTask { issue_id: issue.id });
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct QueryWGIssueCommentsTask {
    number: i64,
    issue_title: String,
    issue_labels: Vec<query::IssueLabel>,
    since: String,
}

#[typetag::serde]
impl Task for QueryWGIssueCommentsTask {
    fn run(
        &self,
        state: &mut State,
        config: &Config,
        _repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        let comments = query::issue_comments(
            &config.github_key,
            &config.wg_repo_owner,
            &config.wg_repo_name,
            self.number,
        )?;

        for comment in comments {
            if comment.created_at >= self.since {
                state.post_task(ProcessWGCommentTask {
                    issue_number: self.number,
                    issue_title: self.issue_title.clone(),
                    issue_labels: self.issue_labels.clone(),
                    url: comment.url,
                    body_text: comment.body_text,
                });
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ProcessWGCommentTask {
    issue_number: i64,
    issue_title: String,
    issue_labels: Vec<query::IssueLabel>,
    url: String,
    body_text: String,
}

#[typetag::serde]
impl Task for ProcessWGCommentTask {
    fn run(
        &self,
        state: &mut State,
        _config: &Config,
        repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        const PREFIX: &'static str = "RESOLVED: ";

        let resolutions = self
            .body_text
            .lines()
            .filter(|line| line.starts_with(PREFIX))
            .map(|line| line[PREFIX.len()..].to_string())
            .collect::<Vec<_>>();

        if resolutions.is_empty() {
            return Ok(());
        }

        if state.handled_wg_comments.contains(&self.url) {
            return Ok(());
        }

        state.handled_wg_comments.insert(self.url.clone());

        let mut desired_labels = Vec::new();
        if let Some(labels_config) = &repo_config.labels {
            for label in &self.issue_labels {
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
            state.post_task(EnsureLabelTask {
                name: format!("[spec] {}", label.name),
                color: label.color.clone(),
            });
        }

        state.post_task(FileIssueTask {
            issue_number: self.issue_number,
            issue_title: self.issue_title.clone(),
            issue_labels: desired_labels
                .into_iter()
                .map(|l| format!("[spec] {}", l.name))
                .collect(),
            comment_url: self.url.clone(),
            resolutions,
        });

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct QueryDecisionsKnownLabelsTask;

#[typetag::serde]
impl Task for QueryDecisionsKnownLabelsTask {
    fn run(
        &self,
        state: &mut State,
        config: &Config,
        _repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        let result = query::known_labels(
            &config.github_key,
            &config.decisions_repo_owner,
            &config.decisions_repo_name,
        )?;

        let known_labels = state.known_labels.get_or_insert_with(|| HashMap::new());

        for label in result {
            known_labels.insert(label.name, label.id);
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EnsureLabelTask {
    name: String,
    color: String,
}

#[typetag::serde]
impl Task for EnsureLabelTask {
    fn run(
        &self,
        state: &mut State,
        config: &Config,
        _repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        if state.known_labels.is_none() {
            state.post_task(QueryDecisionsKnownLabelsTask);
            state.post_task(self.clone());
            return Ok(());
        }

        if state.decisions_repo_id.is_none() {
            state.post_task(QueryDecisionsRepoID);
            state.post_task(self.clone());
            return Ok(());
        }

        if state
            .known_labels
            .as_ref()
            .unwrap()
            .contains_key(&self.name)
        {
            return Ok(());
        }

        let label_id = query::create_label(
            &config.github_key,
            state.decisions_repo_id.as_ref().unwrap(),
            &self.name,
            &self.color,
        )?;

        state
            .known_labels
            .as_mut()
            .unwrap()
            .insert(self.name.clone(), label_id);

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct QueryDecisionsRepoID;

#[typetag::serde]
impl Task for QueryDecisionsRepoID {
    fn run(
        &self,
        state: &mut State,
        config: &Config,
        _repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        let result = query::repo_id(
            &config.github_key,
            &config.decisions_repo_owner,
            &config.decisions_repo_name,
        )?;

        if result.is_none() {
            return Err(format_err!("repository not found"));
        }

        state.decisions_repo_id = result;

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FileIssueTask {
    issue_number: i64,
    issue_title: String,
    issue_labels: Vec<String>,
    comment_url: String,
    resolutions: Vec<String>,
}

#[typetag::serde]
impl Task for FileIssueTask {
    fn run(
        &self,
        state: &mut State,
        config: &Config,
        _repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        if state.known_labels.is_none() {
            state.post_task(QueryDecisionsKnownLabelsTask);
            state.post_task(self.clone());
            return Ok(());
        }

        if state.decisions_repo_id.is_none() {
            state.post_task(QueryDecisionsRepoID);
            state.post_task(self.clone());
            return Ok(());
        }

        let plural = if self.resolutions.len() == 1 {
            "A resolution was"
        } else {
            "Resolutions were"
        };
        let issue_url = format!("{}/issues/{}", config.wg_repo_url(), self.issue_number);
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
            self.issue_number,
            issue_url,
            escape_markdown(&self.issue_title),
            self.resolutions
                .iter()
                .map(|s| format!("* RESOLVED: {}\n", escape_markdown(&s)))
                .collect::<String>(),
            self.comment_url,
        );

        let label_ids = self
            .issue_labels
            .iter()
            .flat_map(|s| state.known_labels.as_ref().unwrap().get(s))
            .map(|s| s.to_string())
            .collect::<Vec<_>>();

        query::create_issue(
            &config.github_key,
            state.decisions_repo_id.as_ref().unwrap(),
            self.issue_title.clone(),
            Some(body),
            Some(label_ids),
        )?;

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RemoveDecisionsIssueBugLabelTask {
    issue_id: String,
}

#[typetag::serde]
impl Task for RemoveDecisionsIssueBugLabelTask {
    fn run(
        &self,
        state: &mut State,
        config: &Config,
        _repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        if state.known_labels.is_none() {
            state.post_task(QueryDecisionsKnownLabelsTask);
            state.post_task(self.clone());
            return Ok(());
        }

        let label_id = state
            .known_labels
            .as_ref()
            .unwrap()
            .get("bug")
            .ok_or_else(|| format_err!("decisions repo missing 'bug' label"))?
            .clone();

        query::remove_labels(&config.github_key, self.issue_id.clone(), vec![label_id])?;

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CloseIssueTask {
    issue_id: String,
}

#[typetag::serde]
impl Task for CloseIssueTask {
    fn run(
        &self,
        _state: &mut State,
        config: &Config,
        _repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        query::close_issue(&config.github_key, self.issue_id.clone())?;

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FileBugForDecisionsIssueTask {
    product: String,
    component: String,
    issue_number: i64,
    issue_id: String,
}

#[typetag::serde]
impl Task for FileBugForDecisionsIssueTask {
    fn run(
        &self,
        state: &mut State,
        config: &Config,
        _repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        let title_and_body = query::issue_title_and_body(
            &config.github_key,
            &config.decisions_repo_owner,
            &config.decisions_repo_name,
            self.issue_number,
        )?;

        let title = title_and_body.0;
        let body = title_and_body.1;

        let body = body.split("----").next().unwrap_or_default();
        let mut urls = extract_urls(&body).into_iter().collect::<Vec<_>>();

        urls.push(format!(
            "{}/issues/{}",
            config.decisions_repo_url(),
            self.issue_number
        ));

        state.post_task(FileBugForDecisionsIssueWithDetailsTask {
            product: self.product.clone(),
            component: self.component.clone(),
            summary: title,
            description: body.to_string(),
            urls,
            issue_number: self.issue_number,
            issue_id: self.issue_id.clone(),
        });

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FileBugForDecisionsIssueWithDetailsTask {
    product: String,
    component: String,
    summary: String,
    description: String,
    urls: Vec<String>,
    issue_number: i64,
    issue_id: String,
}

#[typetag::serde]
impl Task for FileBugForDecisionsIssueWithDetailsTask {
    fn run(
        &self,
        state: &mut State,
        config: &Config,
        _repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        let url = query::file_bug(
            &config.bugzilla_key,
            self.product.clone(),
            self.component.clone(),
            self.summary.clone(),
            self.description.clone(),
            self.urls.clone(),
        )?;

        state.post_task(AddIssueCommentTask {
            issue_id: self.issue_id.clone(),
            body: url,
        });

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AddIssueCommentTask {
    issue_id: String,
    body: String,
}

#[typetag::serde]
impl Task for AddIssueCommentTask {
    fn run(
        &self,
        _state: &mut State,
        config: &Config,
        _repo_config: &RepoConfig,
    ) -> Result<(), Error> {
        query::add_issue_comment(&config.github_key, self.issue_id.clone(), self.body.clone())?;

        Ok(())
    }
}
