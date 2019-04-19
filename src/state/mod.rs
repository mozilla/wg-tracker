mod current;

use failure::{format_err, Error, ResultExt};
use std::fs::File;
use std::io::Read;
use std::ops::{Deref, DerefMut};
use std::path::Path;

pub use current::State;

#[derive(Default)]
pub struct VersionedState(State);

impl Deref for VersionedState {
    type Target = State;

    fn deref(&self) -> &State {
        &self.0
    }
}

impl DerefMut for VersionedState {
    fn deref_mut(&mut self) -> &mut State {
        &mut self.0
    }
}

impl VersionedState {
    pub fn new(date: &str) -> VersionedState {
        // FIXME Use a better type for date or assert the value is valid.
        VersionedState(State::new(date))
    }

    pub fn from_path(path: &Path) -> Result<VersionedState, Error> {
        let mut contents = String::new();
        File::open(path)
            .context("could not open state file")?
            .read_to_string(&mut contents)
            .context("could not read state file")?;

        match contents.find('\n') {
            Some(i) => {
                let version = contents[0..i]
                    .parse::<u32>()
                    .context("could not parse version number in state file")?;
                let json = &contents[i + 1..];
                State::from_versioned_str(version, json).map(VersionedState)
            }
            None => Err(format_err!("could not find version number in state file")),
        }
    }
}
