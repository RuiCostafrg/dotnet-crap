//! The versioned `dotnet-crap.json` project configuration and its precedence
//! against explicit command-line values.
//!
//! See `docs/CONFIG.md` for the file format.
use crate::error::{Error, ErrorKind, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// File name looked up in the scan root when no `--config` is given.
pub const FILE_NAME: &str = "dotnet-crap.json";

/// The only config schema version this build reads.
const SUPPORTED_VERSION: i64 = 1;

/// Parsed and validated configuration. Every field except `version` is optional.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Schema version; must be 1.
    pub version: i64,
    /// Scores strictly greater than this fail; finite and nonnegative.
    pub threshold: Option<f64>,
    /// Include globs, as `ScanOptions::include`.
    pub include: Option<Vec<String>>,
    /// Exclude globs, as `ScanOptions::exclude`.
    pub exclude: Option<Vec<String>>,
    /// As `ScanOptions::respect_ignore_files`.
    pub respect_ignore_files: Option<bool>,
    /// As `ScanOptions::require_coverage`.
    pub require_coverage: Option<bool>,
    /// As `ScanOptions::fail_on_empty`.
    pub fail_on_empty: Option<bool>,
    /// As `ScanOptions::skip_unparsed`.
    pub skip_unparsed: Option<bool>,
    /// As `ScanOptions::defines`.
    pub defines: Option<Vec<String>>,
    /// Baseline file, as `ScanOptions::baseline`. Written relative to the
    /// config file's directory; [`load`] resolves it against that directory.
    pub baseline: Option<PathBuf>,
}

/// Build the error for a config file that cannot be used.
fn invalid(path: &Path, reason: impl std::fmt::Display) -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        format!("invalid config {}: {reason}", path.display()),
    )
}

/// Read and validate a config file. Every failure is an `InvalidInput` error
/// whose message names `path`.
pub fn load(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path).map_err(|e| invalid(path, e))?;
    let mut config: Config = serde_json::from_str(&text).map_err(|e| invalid(path, e))?;
    if config.version != SUPPORTED_VERSION {
        return Err(invalid(
            path,
            format!(
                "unsupported config version {}; this dotnet-crap reads version {SUPPORTED_VERSION}",
                config.version
            ),
        ));
    }
    if config.threshold.is_some_and(|t| !t.is_finite() || t < 0.0) {
        return Err(invalid(path, "threshold must be finite and nonnegative"));
    }
    if let Some(baseline) = &mut config.baseline {
        // Relative to the config file, whatever the working directory is.
        *baseline = path.parent().unwrap_or(Path::new("")).join(&*baseline);
    }
    Ok(config)
}

/// Find the config for a scan: `explicit` must exist; otherwise
/// `<root>/dotnet-crap.json` is used when present; `disabled` skips both.
pub fn discover(root: &Path, explicit: Option<&Path>, disabled: bool) -> Result<Option<Config>> {
    if disabled {
        return Ok(None);
    }
    if let Some(path) = explicit {
        return load(path).map(Some);
    }
    let path: PathBuf = root.join(FILE_NAME);
    if path.exists() {
        return load(&path).map(Some);
    }
    Ok(None)
}

/// Values given explicitly on the command line; unset means "not given".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overrides {
    /// `--threshold`.
    pub threshold: Option<f64>,
    /// `--require-coverage` / `--no-require-coverage`.
    pub require_coverage: Option<bool>,
    /// `--fail-on-empty` / `--no-fail-on-empty`.
    pub fail_on_empty: Option<bool>,
    /// `--skip-unparsed` / `--no-skip-unparsed`.
    pub skip_unparsed: Option<bool>,
    /// `--no-ignore` gives `Some(false)`.
    pub respect_ignore_files: Option<bool>,
    /// `--include` values; a non-empty list replaces the config list.
    pub include: Vec<String>,
    /// `--exclude` values; a non-empty list replaces the config list.
    pub exclude: Vec<String>,
    /// `--define` values; a non-empty list replaces the config list.
    pub defines: Vec<String>,
    /// `--baseline`.
    pub baseline: Option<PathBuf>,
    /// `--no-baseline`: ignore a configured baseline.
    pub no_baseline: bool,
}

/// Effective settings after applying precedence.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// Effective threshold.
    pub threshold: f64,
    /// Effective `require_coverage`.
    pub require_coverage: bool,
    /// Effective `fail_on_empty`.
    pub fail_on_empty: bool,
    /// Effective `skip_unparsed`.
    pub skip_unparsed: bool,
    /// Effective `respect_ignore_files`.
    pub respect_ignore_files: bool,
    /// Effective include globs.
    pub include: Vec<String>,
    /// Effective exclude globs.
    pub exclude: Vec<String>,
    /// Effective defines.
    pub defines: Vec<String>,
    /// Effective baseline file, if any.
    pub baseline: Option<PathBuf>,
}

/// Apply precedence: explicit CLI value, then config value, then built-in default.
pub fn resolve(config: Option<&Config>, cli: &Overrides) -> Resolved {
    let from_config = Config::default();
    let config = config.unwrap_or(&from_config);
    let list = |cli: &[String], config: &Option<Vec<String>>| {
        if cli.is_empty() {
            config.clone().unwrap_or_default()
        } else {
            cli.to_vec()
        }
    };
    Resolved {
        threshold: cli.threshold.or(config.threshold).unwrap_or(10.0),
        require_coverage: cli
            .require_coverage
            .or(config.require_coverage)
            .unwrap_or(false),
        fail_on_empty: cli.fail_on_empty.or(config.fail_on_empty).unwrap_or(false),
        skip_unparsed: cli.skip_unparsed.or(config.skip_unparsed).unwrap_or(false),
        respect_ignore_files: cli
            .respect_ignore_files
            .or(config.respect_ignore_files)
            .unwrap_or(true),
        include: list(&cli.include, &config.include),
        exclude: list(&cli.exclude, &config.exclude),
        defines: list(&cli.defines, &config.defines),
        baseline: if cli.no_baseline {
            None
        } else {
            cli.baseline.clone().or_else(|| config.baseline.clone())
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(text: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dotnet-crap.json");
        fs::write(&path, text).unwrap();
        (dir, path)
    }

    fn rejected(text: &str) -> String {
        let (_dir, path) = write(text);
        let error = load(&path).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        let message = error.to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
        message
    }

    #[test]
    fn loads_a_full_file() {
        let (_dir, path) = write(
            r#"{ "version": 1, "threshold": 7.5, "include": ["src/**"],
                "exclude": ["**/Migrations/**"], "respect_ignore_files": false,
                "require_coverage": true, "fail_on_empty": true,
                "skip_unparsed": true, "defines": ["NET10_0"],
                "baseline": "ci/baseline.json" }"#,
        );
        let config = load(&path).unwrap();
        assert_eq!(
            config,
            Config {
                version: 1,
                threshold: Some(7.5),
                include: Some(vec!["src/**".into()]),
                exclude: Some(vec!["**/Migrations/**".into()]),
                respect_ignore_files: Some(false),
                require_coverage: Some(true),
                fail_on_empty: Some(true),
                skip_unparsed: Some(true),
                defines: Some(vec!["NET10_0".into()]),
                baseline: Some(path.parent().unwrap().join("ci/baseline.json")),
            }
        );
    }

    #[test]
    fn loads_a_minimal_file() {
        let (_dir, path) = write(r#"{"version":1}"#);
        assert_eq!(
            load(&path).unwrap(),
            Config {
                version: 1,
                ..Config::default()
            }
        );
    }

    #[test]
    fn accepts_a_zero_threshold() {
        let (_dir, path) = write(r#"{"version":1,"threshold":0}"#);
        assert_eq!(load(&path).unwrap().threshold, Some(0.0));
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(rejected(r#"{"version":1,"treshold":3}"#).contains("unknown field"));
    }

    #[test]
    fn rejects_other_versions() {
        assert!(rejected(r#"{"version":2}"#)
            .contains("unsupported config version 2; this dotnet-crap reads version 1"));
        assert!(rejected(r#"{"version":0}"#).contains("unsupported config version 0"));
    }

    #[test]
    fn rejects_a_missing_version() {
        assert!(rejected(r#"{"threshold":3}"#).contains("missing field `version`"));
    }

    #[test]
    fn rejects_a_negative_threshold() {
        assert!(rejected(r#"{"version":1,"threshold":-1}"#)
            .contains("threshold must be finite and nonnegative"));
    }

    #[test]
    fn rejects_invalid_json_naming_the_file() {
        rejected("{ not json");
    }

    #[test]
    fn rejects_a_missing_file_naming_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.json");
        let error = load(&path).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert!(error.to_string().contains("absent.json"));
    }

    #[test]
    fn discovery_prefers_explicit_then_root_file_and_honors_disable() {
        let (dir, root_file) = write(r#"{"version":1,"threshold":1}"#);
        let other = dir.path().join("other.json");
        fs::write(&other, r#"{"version":1,"threshold":2}"#).unwrap();
        let threshold = |explicit: Option<&Path>, disabled| {
            discover(dir.path(), explicit, disabled)
                .unwrap()
                .map(|c| c.threshold)
        };
        assert_eq!(threshold(None, false), Some(Some(1.0)));
        assert_eq!(threshold(Some(&other), false), Some(Some(2.0)));
        assert_eq!(threshold(None, true), None);
        assert_eq!(threshold(Some(&other), true), None);
        fs::remove_file(root_file).unwrap();
        assert_eq!(threshold(None, false), None);
        assert!(discover(dir.path(), Some(&dir.path().join("nope.json")), false).is_err());
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn defaults_apply_without_config_or_cli() {
        let resolved = resolve(None, &Overrides::default());
        assert_eq!(
            resolved,
            Resolved {
                threshold: 10.0,
                require_coverage: false,
                fail_on_empty: false,
                skip_unparsed: false,
                respect_ignore_files: true,
                include: vec![],
                exclude: vec![],
                defines: vec![],
                baseline: None,
            }
        );
    }

    #[test]
    fn config_overrides_defaults() {
        let config = Config {
            version: 1,
            threshold: Some(4.0),
            include: Some(strings(&["a"])),
            exclude: Some(strings(&["b"])),
            respect_ignore_files: Some(false),
            require_coverage: Some(true),
            fail_on_empty: Some(true),
            skip_unparsed: Some(true),
            defines: Some(strings(&["X"])),
            baseline: Some(PathBuf::from("cfg/base.json")),
        };
        let resolved = resolve(Some(&config), &Overrides::default());
        assert_eq!(
            resolved,
            Resolved {
                threshold: 4.0,
                require_coverage: true,
                fail_on_empty: true,
                skip_unparsed: true,
                respect_ignore_files: false,
                include: strings(&["a"]),
                exclude: strings(&["b"]),
                defines: strings(&["X"]),
                baseline: Some(PathBuf::from("cfg/base.json")),
            }
        );
    }

    #[test]
    fn cli_overrides_config() {
        let config = Config {
            version: 1,
            threshold: Some(4.0),
            include: Some(strings(&["a"])),
            exclude: Some(strings(&["b"])),
            respect_ignore_files: Some(true),
            require_coverage: Some(true),
            fail_on_empty: Some(true),
            skip_unparsed: Some(true),
            defines: Some(strings(&["X"])),
            baseline: Some(PathBuf::from("cfg/base.json")),
        };
        let cli = Overrides {
            threshold: Some(0.0),
            require_coverage: Some(false),
            fail_on_empty: Some(false),
            skip_unparsed: Some(false),
            respect_ignore_files: Some(false),
            include: strings(&["c", "d"]),
            exclude: strings(&["e"]),
            defines: strings(&["Y"]),
            baseline: Some(PathBuf::from("cli.json")),
            no_baseline: false,
        };
        let resolved = resolve(Some(&config), &cli);
        assert_eq!(
            resolved,
            Resolved {
                threshold: 0.0,
                require_coverage: false,
                fail_on_empty: false,
                skip_unparsed: false,
                respect_ignore_files: false,
                include: strings(&["c", "d"]),
                exclude: strings(&["e"]),
                defines: strings(&["Y"]),
                baseline: Some(PathBuf::from("cli.json")),
            }
        );
        // `--no-baseline` beats a config baseline, and the command line beats
        // the config when it names one.
        let off = Overrides {
            no_baseline: true,
            ..Overrides::default()
        };
        assert_eq!(resolve(Some(&config), &off).baseline, None);
        assert_eq!(
            resolve(Some(&config), &Overrides::default()).baseline,
            Some(PathBuf::from("cfg/base.json"))
        );
    }

    #[test]
    fn a_relative_baseline_resolves_against_the_config_directory() {
        let (dir, path) = write(r#"{"version":1,"baseline":"b.json"}"#);
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        let nested = sub.join("c.json");
        fs::write(&nested, r#"{"version":1,"baseline":"../b.json"}"#).unwrap();
        assert_eq!(
            load(&path).unwrap().baseline,
            Some(dir.path().join("b.json"))
        );
        assert_eq!(load(&nested).unwrap().baseline, Some(sub.join("../b.json")));
        // An absolute path stays as written.
        let absolute = dir.path().join("abs.json");
        fs::write(
            &absolute,
            format!(
                r#"{{"version":1,"baseline":{:?}}}"#,
                nested.to_str().unwrap()
            ),
        )
        .unwrap();
        assert_eq!(load(&absolute).unwrap().baseline, Some(nested));
    }
}
