#![warn(clippy::pedantic)]
#![allow(clippy::enum_glob_use)]

#[macro_use] extern crate boolean_enums;
extern crate capabilities;
extern crate clap;
extern crate env_logger;
extern crate failure;
extern crate glob;
extern crate libc;
#[macro_use] extern crate log;
extern crate serde;
#[macro_use] extern crate serde_derive;
extern crate toml;

use std::collections::BTreeMap;
use std::fs::*;
use std::io::prelude::*;
use std::mem::*;
use std::path::PathBuf;
use std::process::*;

use capabilities::*;
use clap::*;
use failure::{*, Error};
use glob::*;
use libc::*;
use log::Level::*;

type Result<T> = std::result::Result<T, Error>;

const CAP_DAC_OVERRIDE: u8 = 1;
const CAP_SYS_RAWIO: u8 = 17;

const CONFIG_PATH: &str = "/etc/loudspin.conf";

#[allow(non_snake_case)]
fn DEFAULT_HDPARM_PATH() -> String {
    "/sbin/hdparm".to_string()
}

#[allow(non_snake_case)]
fn DEFAULT_LEVELS() -> BTreeMap<String, u8> {
    [
        (String::from("loud"), 254),
        (String::from("quiet"), 128)
    ].iter().cloned().collect()
}

#[derive(Debug, Deserialize, Serialize)]
struct Config {
    #[serde(default = "DEFAULT_HDPARM_PATH")]
    hdparm_path: String,
    devices: Vec<String>,
    #[serde(default = "DEFAULT_LEVELS")]
    levels: BTreeMap<String, u8>,
    #[serde(skip)]
    command_arg: Option<String>,
    #[serde(skip)]
    is_showing: IsShowing,
    #[serde(skip)]
    is_listing: IsListing
}

gen_boolean_enum!(IsShowing);
gen_boolean_enum!(IsListing);

fn main() {
    if let Err(e) = the_main() {
        let mut first = true;
        for i in e.iter_chain() {
            if !first {
                eprint!(": ");
            }
            eprint!("{}", i);
            first = false;
        }
        eprintln!("");
    }
}

fn the_main() -> Result<()> {
    env_logger::init();

    let config = get_config()?;

    if log_enabled!(Debug) {
        debug!("read config:");
        for i in toml::to_string(&config)
                .context("error serializing configuration for logging")?
                .lines() {
            debug!("\t{}", i);
        }
    }

    gain_caps()?;
    debug!("set capabilities");

    if config.is_listing.into() {
        for i in config.levels {
            println!("{} = {}", i.0, i.1);
        }
        return Ok(())
    }

    for g in &config.devices {
        debug!("processing glob \"{}\"", g);
        let files = glob_with(&g, MatchOptions {
            require_literal_separator: true,
            require_literal_leading_dot: true,
            ..MatchOptions::default()
        }).context("error listing device files")?;

        for i in files {
            let dev_filename = match i {
                Ok(x) => {
                    debug!("found device file at {}", x.to_string_lossy());
                    x
                },
                Err(e) => {
                    eprintln!("failed to list file: {}", e);
                    continue;
                }
            };

            process_devfile(&config, &dev_filename)?;
        }
    }

    Ok(())
}

fn get_config() -> Result<Config> {
    let matches = get_matches();

    let mut config = read_config_file()?;
    config.command_arg = matches.value_of("loudness").map(String::from);
    config.is_showing = matches.subcommand_matches("show").is_some().into();
    config.is_listing = matches.subcommand_matches("list").is_some().into();

    if (!config.is_listing).into() && config.command_arg.is_none() {
        // show is the default if no arguments are passed
        config.is_showing = true.into();
    }

    // loud and quiet are always present, but can be overridden
    config.levels.entry(String::from("loud")).or_insert(254);
    config.levels.entry(String::from("quiet")).or_insert(128);

    validate_config(&config)?;

    Ok(config)
}

fn get_matches() -> Box<ArgMatches<'static>> {
    let matches = App::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .about(crate_description!())
        .subcommand(SubCommand::with_name("show")
            .about("Shows the current state")
        ).subcommand(SubCommand::with_name("list")
            .about("Lists all configured loudness levels.  Default is \
                loud = \"254\" and quiet = \"128\"")
        ).arg(Arg::with_name("loudness")
            .value_name("LOUDNESS_LEVEL")
        ).get_matches();
    Box::new(matches)
}

fn read_config_file() -> Result<Config> {
    let mut config_file = File::open(CONFIG_PATH)
        .context("error opening the configuration file")?;
    let mut config_str = String::new();
    config_file.read_to_string(&mut config_str)
        .context("error reading from the configuration file")?;
    drop(config_file);

    let config: Config = toml::from_str(&config_str)
        .context("error parsing the configuration")?;

    Ok(config)
}

fn validate_config(config: &Config) -> Result<()> {
    for i in &config.levels {
        let level = *i.1;
        if level < 128 || level > 254 {
            bail!(
                "invalid AAM level in {}: {} = {}",
                CONFIG_PATH,
                i.0,
                level
            );
        }
    }

    Ok(())
}

fn gain_caps() -> Result<()> {
    let mut caps = Capabilities::new()
        .context("error initializing capabilities")?;

    let capset = [
        Capability::CAP_DAC_OVERRIDE,
        Capability::CAP_SYS_RAWIO
    ];
    if !caps.update(&capset, Flag::Effective, true)
            || !caps.update(&capset, Flag::Inheritable, true)
            || !caps.update(&capset, Flag::Permitted, true) {
        bail!("");
    }
    caps.apply().context("error setting capabilities")?;

    set_ambient_cap(CAP_DAC_OVERRIDE)
        .context("error setting ambient capabilities")?;
    set_ambient_cap(CAP_SYS_RAWIO)
        .context("error setting ambient capabilities")?;

    Ok(())
}

fn set_ambient_cap(cap: u8) -> Result<()> { unsafe {
    #[allow(clippy::cast_sign_loss)]
    let ret = prctl(
        PR_CAP_AMBIENT,
        PR_CAP_AMBIENT_RAISE as c_ulong,
        c_ulong::from(cap),
        0,
        0
    );
    if ret == -1 {
        bail!("unable to set ambient capabilities: {}",
            std::io::Error::last_os_error());
    }
    Ok(())
}}

fn process_devfile(config: &Config, dev_filename: &PathBuf) -> Result<()> {
    let mut cmd = Command::new(&config.hdparm_path);
        cmd.arg("-M");
        if config.is_showing.into() {
            cmd.arg(&dev_filename);
        } else {
            let hdparm_arg = format!("{}", translate_arg(&config)?);
            cmd.arg(hdparm_arg).arg(&dev_filename.as_os_str());
        };
        cmd.spawn().context("error calling hdparm")?
            .wait().context("error waiting for hdparm to complete")?;
    debug!("executed hdparm for {}",
        dev_filename.to_string_lossy());

    Ok(())
}

fn translate_arg(config: &Config) -> Result<u8> {
    let command_arg = config.command_arg.as_ref().unwrap();

    Ok(match config.levels.get(command_arg) {
        Some(level) => *level,
        None => {
            match command_arg.as_str() {
                "loud" => 254,
                "quiet" => 128,
                _ => bail!("no such loudness level: {}", command_arg)
            }
        }
    })
}
