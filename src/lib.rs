use log::warn;
use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::result::Result;
use std::str;
use std::io::Error as IoError;

/// the info portion of the ``PyPI`` API response
#[derive(Deserialize)]
struct PyPiInfo {
    /// The classifiers of the package
    classifiers: Option<Vec<String>>,
    /// The license of the package
    license: Option<String>,
}

/// The JSON info returned by the ``PyPI`` API
#[derive(Deserialize)]
struct PyPiJson {
    /// The info portion of the PyPI API response
    info: PyPiInfo,
}

impl PyPiInfo {
    fn get_license_from_info(&self) -> Option<String> {
        if let Some(license) = self.license.clone() {
            if let Some(first_line) = license.lines().next() {
                if !first_line.trim().is_empty() {
                    return Some(first_line.trim().to_string());
                }
            }
        }
        None
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
        from_classifiers
    }
}

/// The settings for the license check
#[derive(Deserialize)]
pub struct LicenseSettings {
    /// The list of licenses as regex that are considered valid.
    #[serde(with = "serde_regex")]
    allowed: Vec<Regex>,
    /// The list of licenses as regex that are considered invalid.
    #[serde(with = "serde_regex")]
    disallowed: Vec<Regex>,
    /// The list of packages that should be ignored
    ignored: HashSet<String>,
    /// A map of package names to licenses which should be used as fallback
    missing: HashMap<String, String>,
}

impl LicenseSettings {
    pub fn from_file(file_name: &str) -> Result<Self, Box<dyn Error>> {
        let settings_file =
            std::fs::read_to_string(file_name)?;
        let settings = toml::from_str(&settings_file)?;
        Ok(settings)
    }
}

#[derive(Debug)]
pub struct PackageResult {
    pub name: String,
    pub licenses: Vec<String>,
    pub allowed: Vec<String>,
    pub disallowed: Vec<String>,
    pub ignored: bool,
}

impl PackageResult {
    pub async fn new(name: String, settings: &LicenseSettings) -> Self {
        let mut results = Self {
            licenses: get_licenses_for_package(&name).await,
            name: name.clone(),
            allowed: Vec::new(),
            disallowed: Vec::new(),
            ignored: false,
        };

        if let Some(overridden) = settings.missing.get(&name) {
            results.allowed.push(overridden.clone());
            return results;
        }
        if settings.ignored.contains(&name) {
            results.ignored = true;
            return results;
        }

        for license in results.licenses.iter() {
            if let Some(found) = get_first_match(&settings.disallowed, license.as_str()) {
                results.disallowed.push(found.to_string());
            } else if let Some(found) = get_first_match(&settings.allowed, license.as_str()) {
                results.allowed.push(found.to_string());
            }
        }
        results
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
/// 
/// # Errors
/// 
/// ``std::io::Error`` on errors reading the requirements file
pub fn read_packages_from_requirements(file_name: &str) -> Result<Vec<String>, IoError> {
    let mut packages = Vec::new();
    let file = std::fs::read_to_string(file_name)?;
    for line in file.lines() {
        if let Some(package_name) = extract_package_name_from_line(line) {
            packages.push(package_name);
        }
    }

    packages.sort();
    Ok(packages)
}

/// Fetch the JSON info from PyPI for a package
async fn fetch_json(package_name: &str) -> Result<PyPiJson, reqwest::Error> {
    let url = format!("https://pypi.org/pypi/{}/json", package_name);
    let resp = reqwest::get(url).await?.json::<PyPiJson>().await?;
    Ok(resp)
}

async fn get_licenses_for_package(package_name: &str) -> Vec<String> {
    match fetch_json(package_name).await {
        Ok(json) => {
            let mut licenses = json.licenses();

            licenses.sort();
            licenses.dedup();
            licenses
        }
        Err(e) => {
            warn!(
                "Could not get licenses for package {}. {}.",
                package_name, e
            );
            Vec::new()
        }
    }
}

fn get_first_match<'a>(regexes: &Vec<Regex>, haystack: &'a str) -> Option<&'a str> {
    for regex in regexes {
        if let Some(found) = regex.find(haystack) {
            return Some(found.as_str());
        }
    }
    None
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_fetch_json() {
        let package_name = "requests";
        let resp = fetch_json(package_name).await;
        assert!(resp.is_ok());
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
    fn test_requirements_file_not_found() {
        assert!(read_packages_from_requirements("not_found.txt").is_err());
    }

    #[test]
    fn test_read_packages_from_requirements() {
        let packages = read_packages_from_requirements("tests/requirements.txt");
        assert_eq!(packages.unwrap(), vec!["django", "flask", "rasterio"]);
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
            license: Some(mit_string.clone()),
        };

        assert_eq!(info.get_license_from_info(), Some(mit_string));
    }

    #[test]
    fn test_get_license_from_info_empty() {
        let info = PyPiInfo {
            classifiers: None,
            license: None,
        };

        assert_eq!(info.get_license_from_info(), None);
    }

    #[test]
    fn test_get_license_from_info_empty_string() {
        let info = PyPiInfo {
            classifiers: None,
            license: Some("".to_string()),
        };

        assert_eq!(info.get_license_from_info(), None);
    }

    #[tokio::test]
    async fn test_get_licenses_for_package() {
        assert_eq!(
            get_licenses_for_package("django").await,
            vec!["BSD License", "BSD-3-Clause"]
        )
    }

    #[test]
    fn test_load_settings() {
        let settings = LicenseSettings::from_file("tests/licenses.toml").unwrap();
        let mut packages = settings
            .allowed
            .iter()
            .map(|r| r.as_str())
            .collect::<Vec<&str>>();
        packages.dedup();
        assert_eq!(packages, vec!["(The )?MIT( License)?", "BSD"]);
        assert_eq!(
            settings
                .disallowed
                .iter()
                .map(|r| r.as_str())
                .collect::<Vec<&str>>(),
            vec![".*[^L]GPL.*", "BSD$"]
        );
        assert_eq!(settings.ignored, HashSet::from(["mapdata".to_string()]));
        assert_eq!(
            settings.missing.get("nothing").unwrap(),
            &"MIT License".to_string()
        );
    }

    #[test]
    fn test_get_first_match() {
        assert_eq!(
            get_first_match(
                &vec![
                    Regex::new(r"([a-z]*)[0-9]*([a-z])").unwrap(),
                    Regex::new(r"[0-9]+([a-z])").unwrap()
                ],
                "abc123tyh"
            )
            .unwrap(),
            "abc123t"
        );
        assert_eq!(
            get_first_match(
                &vec![
                    Regex::new(r"[0-9]+([a-z])").unwrap(),
                    Regex::new(r"([a-z]*)[0-9]*([a-z])").unwrap()
                ],
                "abc123tyh"
            )
            .unwrap(),
            "123t"
        );
    }
}
