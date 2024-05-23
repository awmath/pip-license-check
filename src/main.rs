use std::sync::Arc;

use clap::Parser;
use futures::future::join_all;
use log::warn;
type PackageResults = Vec<PackageResult>;

use pip_license_check::{read_packages_from_requirements, LicenseSettings, PackageResult};
use tokio::sync::Mutex;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = CliArgs::parse();

    let settings = Arc::new(
        LicenseSettings::from_file(&cli.settings).unwrap_or_else(|err| {
            println!("Could not read license settings {err}.");
            std::process::exit(1);
        }),
    );

    let packages = read_packages_from_requirements(&cli.requirements).unwrap_or_else(|err| {
        println!("Could not read requirements: {err}");
        std::process::exit(1);
    });

    let results = Arc::new(Mutex::new(PackageResults::new()));

    let tasks = packages.into_iter().map(|package_name| {
        let arc_settings = Arc::clone(&settings);
        let results = Arc::clone(&results);
        tokio::spawn(async move {
            let result = PackageResult::new(package_name, arc_settings).await;
            results.lock().await.push(result);
        })
    });

    join_all(tasks).await;

    let mut fail = false;
    for result in results.lock().await.iter() {
        if result.ignored {
            warn!("Package {} has been ignored as per settings.", result.name)
        } else if !result.disallowed.is_empty() {
            println!(
                "Found disallowed licenses for {}: {:?}",
                result.name, result.disallowed
            );
            fail = true
        } else if cli.verbose && !result.allowed.is_empty() {
            println!(
                "Found allowed licenses for {}: {:?}",
                result.name, result.allowed
            )
        } else if result.licenses.is_empty() {
            println!("No licenses found for {}", result.name);
            fail = true
        } else if result.allowed.is_empty() && result.disallowed.is_empty() {
            println!(
                "No match in allowed or disallowed licenses for license(s) {:?} of {}",
                result.licenses, result.name
            );
            fail = true
        }
    }
    if fail {
        std::process::exit(1)
    }
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

    #[arg(short, long)]
    verbose: bool,
}
