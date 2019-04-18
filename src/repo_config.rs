use failure::{format_err, Error, ResultExt};
use std::collections::HashMap;

#[derive(Debug, Default, Deserialize)]
pub struct RepoConfig {
    pub labels: Option<RepoConfigLabels>,
    pub components: Option<HashMap<String, String>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RepoConfigLabels {
    pub color: Option<String>,
    pub prefixes: Option<Vec<String>>,
}

impl RepoConfig {
    pub fn from_str(toml: &str) -> Result<RepoConfig, Error> {
        let repo_config: RepoConfig =
            toml::from_str(toml).context("could not parse repo config file")?;
        Ok(repo_config)
    }
}
