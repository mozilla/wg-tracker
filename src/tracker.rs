use crate::config::Config;
use crate::repo_config::RepoConfig;
use crate::state::VersionedState;
use crate::util::CLIENT;
use failure::{Error, ResultExt};
use fs2::FileExt;
use std::fs::File;
use std::path::{Path, PathBuf};

pub struct Tracker {
    config: Config,
    repo_config: RepoConfig,
    lockfile: Option<File>,
    state: VersionedState,

    statefile_path: PathBuf,
    statefile_temp_path: PathBuf,
}

impl Tracker {
    pub fn new(config: Config) -> Tracker {
        let state_directory_path = Path::new(&config.state_directory);

        let mut statefile_path = state_directory_path.to_path_buf();
        statefile_path.push("state");

        let mut statefile_temp_path = state_directory_path.to_path_buf();
        statefile_temp_path.push("state.temp");

        Tracker {
            config,
            repo_config: Default::default(),
            lockfile: None,
            state: Default::default(),
            statefile_path,
            statefile_temp_path,
        }
    }

    pub fn run(&mut self) -> Result<(), Error> {
        if !self.try_lock()? {
            // Silently exit if there is another wg-tracker instance running.
            return Ok(());
        }

        let repo_config_url = format!(
            "https://raw.githubusercontent.com/{}/{}/master/config.toml",
            self.config.decisions_repo_owner, self.config.decisions_repo_name
        );
        let repo_config_toml = CLIENT
            .get(&repo_config_url)
            .send()
            .context("could not perform network request")?
            .text()
            .context("could not read request body")?;

        self.repo_config = RepoConfig::from_str(&repo_config_toml)?;

        self.state = if self.statefile_path.exists() {
            VersionedState::from_path(&self.statefile_path)?
        } else {
            VersionedState::new(&self.config.start_date)
        };

        self.state.check_for_updates();

        loop {
            let result = self.state.iterate(&self.config, &self.repo_config);
            self.state
                .save(&self.statefile_path, &self.statefile_temp_path)?;
            result?;
            if self.state.is_finished() {
                return Ok(());
            }
        }
    }

    /// Attempts to lock the lockfile, to prevent simultanteous wg-tracker
    /// instances from running.
    fn try_lock(&mut self) -> Result<bool, Error> {
        let mut lockfile_path = PathBuf::from(&self.config.state_directory);
        lockfile_path.push("lock");

        let lockfile = File::create(lockfile_path).context("Could not open lockfile")?;

        let locked = lockfile.try_lock_exclusive().is_ok();

        self.lockfile = Some(lockfile);

        Ok(locked)
    }
}
