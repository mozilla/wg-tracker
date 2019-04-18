use crate::config::Config;
use crate::state::VersionedState;
use failure::{Error, ResultExt};
use fs2::FileExt;
use std::fs::File;
use std::path::{Path, PathBuf};

pub struct Tracker {
    config: Config,
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
            lockfile: None,
            state: Default::default(),
            statefile_path,
            statefile_temp_path,
        }
    }

    pub fn run(&mut self) -> Result<(), Error> {
        self.lock()?;

        self.state = if self.statefile_path.exists() {
            VersionedState::from_path(&self.statefile_path)?
        } else {
            VersionedState::new(&self.config.start_date)
        };

        self.state.check_for_updates();

        loop {
            let result = self.state.iterate(&self.config);
            self.state
                .save(&self.statefile_path, &self.statefile_temp_path)?;
            result?;
            if self.state.is_finished() {
                return Ok(());
            }
        }
    }

    fn lock(&mut self) -> Result<(), Error> {
        let mut lockfile_path = PathBuf::from(&self.config.state_directory);
        lockfile_path.push("lock");

        let lockfile = File::create(lockfile_path).context("Could not open lockfile")?;
        lockfile
            .lock_exclusive()
            .context("Could not lock lockfile")?;
        self.lockfile = Some(lockfile);

        Ok(())
    }
}
