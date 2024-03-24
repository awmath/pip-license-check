use clap::Parser;
use log::warn;
use regex::Regex;
use reqwest::{self, Error};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::result::Result;
use std::str;
use std::sync::{Arc, Mutex};
use tokio;

type PackageResults = Vec<PackageResult>;

type SharedResult = Arc<Mutex<PackageResults>>;

use pip_license_check::{read_packages_from_requirements, LicenseSettings, PackageResult};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = CliArgs::parse();

    let settings = LicenseSettings::from_file(&cli.settings);
    let packages = read_packages_from_requirements(&cli.requirements);

    // let results = Arc::new(Mutex::new(PackageResults::new()));
    let mut results = PackageResults::new();

    let tasks = packages.into_iter().map(
        |package_name| PackageResult::new(package_name, &settings)
    );

    for task in tasks {
        results.push(task.await);
    }

    for result in results {
        if !result.disallowed.is_empty() {
            println!("Found disallowed licenses for {}: {:?}", result.name, result.disallowed);
            std::process::exit(1)
        }
    }
}

async fn add_results(package_name: &str, settings: &LicenseSettings, results: & mut SharedResult) {
    results.lock().unwrap().push(PackageResult::new(package_name.to_string(), &settings).await);
}
/// The arguments for the CLI
#[derive(Parser)]
struct CliArgs {
    /// The path to the requirements file
    #[arg(short, long, default_value = "requirements.txt")]
    requirements: String,
    /// The path to the settings file
    #[arg(short, long, default_value = "licenses.toml")]
    settings: String,

    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,
}
