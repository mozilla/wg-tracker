use failure::{format_err, Error, ResultExt};
use lazy_static::lazy_static;
use regex::Regex;
use std::fs::File;
use std::io::Read;

#[derive(Deserialize)]
pub struct Config {
    pub github_key: String,
    pub wg_repo_owner: String,
    pub wg_repo_name: String,
    pub decisions_repo_owner: String,
    pub decisions_repo_name: String,
    pub state_directory: String,
    pub start_date: String,
}

impl Config {
    pub fn from_file(file: &str) -> Result<Config, Error> {
        let mut toml = String::new();
        File::open(file)
            .context("could not open config file")?
            .read_to_string(&mut toml)
            .context("could not read config file")?;

        let config: Config = toml::from_str(&toml).context("could not parse config file")?;

        validate_syntax("wg_repo_owner", &config.wg_repo_owner, &REPO_ID_RE)?;
        validate_syntax("wg_repo_name", &config.wg_repo_name, &REPO_ID_RE)?;
        validate_syntax(
            "decisions_repo_owner",
            &config.decisions_repo_owner,
            &REPO_ID_RE,
        )?;
        validate_syntax(
            "decisions_repo_name",
            &config.decisions_repo_name,
            &REPO_ID_RE,
        )?;
        validate_syntax("start_date", &config.start_date, &DATE_RE)?;

        Ok(config)
    }

    pub fn wg_repo_url(&self) -> String {
        format!(
            "https://github.com/{}/{}",
            self.wg_repo_owner, self.wg_repo_name
        )
    }

    pub fn decisions_repo_url(&self) -> String {
        format!(
            "https://github.com/{}/{}",
            self.decisions_repo_owner, self.decisions_repo_name
        )
    }
}

fn validate_syntax(key: &str, value: &str, regex: &Regex) -> Result<(), Error> {
    if !regex.is_match(value) {
        return Err(format_err!(
            "config file {} value has the wrong syntax",
            key,
        ));
    }
    Ok(())
}

lazy_static! {
    static ref DATE_RE: Regex = Regex::new(r"^(\d\d\d\d)-(\d\d)-(\d\d)$").unwrap();
    static ref REPO_ID_RE: Regex = Regex::new(r"^[0-9A-Za-z_-]+$").unwrap();
}
