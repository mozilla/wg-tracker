#[macro_use]
extern crate serde_derive;

mod config;
mod state;
mod tracker;

use clap::{App, Arg};
use config::Config;
use failure::Error;
use tracker::Tracker;

fn run() -> Result<(), Error> {
    let matches = App::new("wg-tracker")
        .arg(Arg::with_name("CONFIG").help("Config file").required(true))
        .get_matches();
    let config = Config::from_file(matches.value_of("CONFIG").unwrap())?;
    Tracker::new(config).run()?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        println!(
            "error: {}{}",
            e,
            e.as_fail()
                .cause()
                .map_or(String::new(), |f| format!(": {}", f))
        );
    }
}
