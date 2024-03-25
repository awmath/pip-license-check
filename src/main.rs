use clap::Parser;
use log::warn;









type PackageResults = Vec<PackageResult>;

use pip_license_check::{read_packages_from_requirements, LicenseSettings, PackageResult};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = CliArgs::parse();

    let settings = LicenseSettings::from_file(&cli.settings);
    let packages = read_packages_from_requirements(&cli.requirements);

    // let results = Arc::new(Mutex::new(PackageResults::new()));
    let mut results = PackageResults::new();

    let tasks = packages
        .into_iter()
        .map(|package_name| PackageResult::new(package_name, &settings));

    for task in tasks {
        results.push(task.await);
    }

    let mut fail = false;
    for result in results {
        if result.ignored {
            warn!("Package {} has been ignored as per settings.", result.name)
        }
        else if !result.disallowed.is_empty() {
            println!(
                "Found disallowed licenses for {}: {:?}",
                result.name, result.disallowed
            );
            fail = true
        }
        else if cli.verbose && !result.allowed.is_empty() {
            println!("Found allowed licenses for {}: {:?}", result.name, result.allowed )
        }
        else if result.licenses.is_empty() {
            println!("No licenses found for {}", result.name);
            fail = true
        }
        else if result.allowed.is_empty() && result.disallowed.is_empty() {
            println!("No match in allowed or disallowed licenses for license(s) {:?} of {}", result.licenses, result.name);
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
