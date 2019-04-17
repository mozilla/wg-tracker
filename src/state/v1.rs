use failure::{format_err, Error, ResultExt};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

#[derive(Default, Deserialize, Serialize)]
pub struct State {
    tasks: Vec<Task>,
    handled_comments: HashSet<String>,
    last_time: String,
}

#[derive(Deserialize, Serialize)]
pub enum Task {
    QueryIssues {
        since: String,
        after: Option<String>,
        issues_so_far: Vec<u32>,
    },
    QueryIssueComments {
        number: u32,
        since: String,
        after: Option<String>,
    },
    ProcessComment {
        url: String,
        body: String,
    },
}

impl State {
    pub fn new(date: &str) -> State {
        State {
            tasks: Vec::new(),
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
        let task = Task::QueryIssues {
            since: self.last_time.clone(),
            after: None,
            issues_so_far: Vec::new(),
        };
        self.tasks.push(task);
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

    pub fn iterate(&mut self) -> Result<bool, Error> {
        Ok(false)
    }
}
