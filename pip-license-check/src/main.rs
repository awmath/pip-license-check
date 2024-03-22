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

type SharedResult = Arc<Mutex<CheckResults>>;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = CliArgs::parse();

    let settings = LicenseSettings::from_file(&cli.settings);
    let allowed_regex = parse_regex_strings(settings.allowed).expect("Error while parsing allowed regex expression. Please check settings file");
    let disallowed_regex = parse_regex_strings(settings.disallowed).expect("Error while parsing disallowed regex expression. Please check settings file");
    let packages = read_packages_from_requirements(&cli.requirements);

    let results = Arc::new(Mutex::new(CheckResults::new()));


}

/// the info portion of the PyPI API response
#[derive(Deserialize)]
struct PyPiInfo {
    /// The name of the package
    name: String,
    /// The classifiers of the package
    classifiers: Option<Vec<String>>,
    /// The license of the package
    license: Option<String>,
}

/// The JSON info returned by the PyPI API
#[derive(Deserialize)]
struct PyPiJson {
    /// The info portion of the PyPI API response
    info: PyPiInfo,
}

impl PyPiInfo {
    fn get_license_from_info(&self) -> Option<String> {
        if let Some(license) = self.license.clone() {
            if !license.trim().is_empty() {
                return Some(license);
            }
        }
        return None;
    }

    fn get_licenses_from_classifiers(&self) -> Vec<String> {
        let regex =
            Regex::new(r"License :: OSI Approved :: (.*)").expect("Could not compile regex");

        let mut licenses = Vec::new();

        if let Some(classifiers) = &self.classifiers {
            for classifier in classifiers {
                // Use the regex to extract the license from the classifier
                let Some(captured) = regex.captures(classifier.as_str()) else {
                    continue;
                };

                let license_str = match captured.get(1) {
                    Some(license_match) => license_match.as_str(),
                    None => continue,
                };
                if !license_str.trim().is_empty() {
                    licenses.push(license_str.trim().to_string());
                }
            }
        }
        licenses
    }
}

impl PyPiJson {
    fn licenses(&self) -> Vec<String> {
        let mut from_classifiers = self.info.get_licenses_from_classifiers();
        if let Some(from_info) = self.info.get_license_from_info() {
            from_classifiers.push(from_info);
        }
        return from_classifiers;
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

    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,
}

/// The settings for the license check
#[derive(Deserialize)]
struct LicenseSettings {
    /// The list of licenses as regex that are considered valid.
    allowed: Vec<String>,
    /// The list of licenses as regex that are considered invalid.
    disallowed: Vec<String>,
    /// The list of packages that should be ignored
    ignored: Vec<String>,
    /// A map of package names to licenses which should be used as fallback
    missing: HashMap<String, String>
}

impl LicenseSettings {
    fn from_file(file_name: &str) -> Self {
        let settings_file =
            std::fs::read_to_string(file_name).expect("Could not read settings file");
        match toml::from_str(&settings_file) {
            Ok(settings) => settings,
            Err(e) => {
                panic!("Could not read settings file. {}.", e)
            }
        }
    }
}

/// The results of the license check
struct CheckResults {
    /// The list of packages that are valid
    valid: Vec<String>,
    /// The list of packages that are invalid
    invalid: Vec<String>,
}

impl CheckResults {
    fn new() -> Self {
        Self {
            valid: Vec::new(),
            invalid: Vec::new(),
        }
    }
}
/// Extract the package name from a line in the requirements file.
/// Returns None if the line is not a valid package name.
/// Removes comments and version tags.
fn extract_package_name_from_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let regex = Regex::new(r"^([a-zA-Z0-9\-_\,]+).*$").expect("Could not compile regex");

    Some(regex.captures(line.trim())?.get(1)?.as_str().to_string())
}

/// Read the requirements file and return the list of packages.
/// Ignores comments and empty lines.
fn read_packages_from_requirements(file_name: &str) -> Vec<String> {
    let mut packages = Vec::new();
    let file = std::fs::read_to_string(file_name).expect("Could not read requirements file");
    for line in file.lines() {
        if let Some(package_name) = extract_package_name_from_line(line) {
            packages.push(package_name);
        }
    }

    packages.sort();
    packages
}

/// Fetch the JSON info from PyPI for a package
async fn fetch_json(package_name: &str) -> Result<PyPiJson, reqwest::Error> {
    let url = format!("https://pypi.org/pypi/{}/json", package_name);
    let resp = reqwest::get(url).await?.json::<PyPiJson>().await?;
    Ok(resp)
}

async fn get_licenses_for_package(package_name: &str) -> Option<Vec<String>> {
    match fetch_json(package_name).await {
        Ok(json) => {
            let mut licenses = json.licenses();
            if licenses.is_empty() {
                return None;
            } else {
                licenses.sort();
                licenses.dedup();
                return Some(licenses);
            }
        }
        Err(e) => {
            warn!(
                "Could not get licenses for package {}. {}.",
                package_name, e
            );
            return None;
        }
    };
}

fn check_license(license: &str, settings: &LicenseSettings) -> Option<bool> {
    if settings.allowed.iter().any(|r| regex::Regex::new(r).unwrap().is_match(license)) {
        return Some(true);
    }
    else if settings.disallowed.iter().any(|r| regex::Regex::new(r).unwrap().is_match(license)) {
        return Some(false);
    }

    None
}

async fn check_licenses(licenses: &Vec<String>, settings: &LicenseSettings) -> Result<(), String> {
    let mut valid = Vec::new();
    let mut invalid = Vec::new();

    for license in licenses {
        if check_license(license, settings) {
            valid.push(license.to_string());
        } else {
            invalid.push(license.to_string());
        }
    }

    if !invalid.is_empty() {
        return Err(format!("Invalid licenses: {:?}", invalid));
    }

    Ok(())
}

fn parse_regex_strings(strings: Vec<String>) -> Result<Vec<Regex>, regex::Error> {
    let mut results = Vec::new();
    for re_string in strings {
        let re = Regex::new(&re_string)?;
        results.push(re);
    }
    Ok(results)
}



#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_fetch_json() {
        let package_name = "requests";
        let resp = fetch_json(package_name).await;
        assert!(resp.is_ok());
        assert_eq!(resp.unwrap().info.name, "requests");
    }

    #[test]
    fn test_extract_package_name() {
        let line = "requests";
        let package_name = extract_package_name_from_line(line).unwrap();
        assert_eq!(package_name, "requests");
    }

    #[test]
    fn test_extract_package_name_with_version() {
        assert_eq!(
            extract_package_name_from_line("requests==2.23.0").unwrap(),
            "requests"
        );
        assert_eq!(
            extract_package_name_from_line("requests<2.23.0").unwrap(),
            "requests"
        );
        assert_eq!(
            extract_package_name_from_line("requests>2.23.0").unwrap(),
            "requests"
        );
    }

    #[test]
    fn test_extract_package_name_ignore_comments() {
        assert_eq!(
            extract_package_name_from_line("requests #this is a comment").unwrap(),
            "requests"
        );
        assert_eq!(
            extract_package_name_from_line("requests# this is a comment").unwrap(),
            "requests"
        );
    }

    #[test]
    fn test_extract_package_name_ignore_empty_lines() {
        assert_eq!(extract_package_name_from_line(""), None);
        assert_eq!(extract_package_name_from_line("#this is a comment"), None);
    }

    #[test]
    #[should_panic(expected = "Could not read requirements file")]
    fn test_requirements_file_not_found() {
        read_packages_from_requirements("not_found.txt");
    }

    #[test]
    fn test_read_packages_from_requirements() {
        let packages = read_packages_from_requirements("tests/requirements.txt");
        assert_eq!(packages, vec!["django", "flask", "rasterio"]);
    }

    #[test]
    fn test_get_licenses_from_classifiers() {
        let classifiers = [
            "License :: OSI Approved :: MIT License",
            "License :: OSI Approved :: BSD License",
            "License :: OSI Approved",
            "Something :: Else",
            "License :: OSI Approved :: Apache Software License",
        ]
        .iter()
        .map(|x| x.to_string())
        .collect();

        let info = PyPiInfo {
            classifiers: Some(classifiers),
            name: "stub".to_string(),
            license: None,
        };

        let licenses = info.get_licenses_from_classifiers();
        assert_eq!(
            licenses,
            vec!["MIT License", "BSD License", "Apache Software License"]
        );
    }

    #[test]
    fn test_empty_classifiers() {
        let classifiers = Vec::new();

        let info = PyPiInfo {
            classifiers: Some(classifiers),
            name: "stub".to_string(),
            license: None,
        };

        let licenses = info.get_licenses_from_classifiers();
        assert!(licenses.is_empty());
    }

    #[test]
    fn test_get_license_from_info() {
        let mit_string = "MIT License".to_string();
        let info = PyPiInfo {
            classifiers: None,
            name: "stub".to_string(),
            license: Some(mit_string.clone()),
        };

        assert_eq!(info.get_license_from_info(), Some(mit_string));
    }

    #[test]
    fn test_get_license_from_info_empty() {
        let info = PyPiInfo {
            classifiers: None,
            name: "stub".to_string(),
            license: None,
        };

        assert_eq!(info.get_license_from_info(), None);
    }

    #[test]
    fn test_get_license_from_info_empty_string() {
        let info = PyPiInfo {
            classifiers: None,
            name: "stub".to_string(),
            license: Some("".to_string()),
        };

        assert_eq!(info.get_license_from_info(), None);
    }

    #[tokio::test]
    async fn test_get_licenses_for_package() {
        assert_eq!(
            get_licenses_for_package("django").await.unwrap(),
            vec!["BSD License", "BSD-3-Clause"]
        )
    }

    #[test]
    fn test_load_settings() {
        let settings = LicenseSettings::from_file("tests/licenses.toml");
        assert_eq!(settings.allowed, vec!["(The )?MIT( License)?"]);
        assert_eq!(settings.disallowed, vec![".*[^L]GPL.*"]);
        assert_eq!(settings.ignored, vec!["mapdata"]);
        assert_eq!(
            settings.missing.get("nothing").unwrap(),
            &"MIT License".to_string()
        );
    }
}
