mod pack_discovery;
mod response;

use std::{fs, io, path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};
use miette::Diagnostic;
use pack_discovery::{DiscoveredPack, PackDiscoveryPaths};
use serde::Serialize;
use thiserror::Error;

#[derive(Parser)]
#[command(name = "kat", disable_version_flag = true)]
struct Cli {
    #[command(subcommand)]
    operation: Operation,
}

#[derive(Subcommand)]
enum Operation {
    /// Inspect the PACKs available to this KAT Skill.
    Inspect {
        /// Inspect one KAT Dataset directory.
        #[arg(long, value_name = "DIRECTORY", conflicts_with = "pack_directories")]
        dataset: Option<PathBuf>,
        #[arg(
            long = "pack-dir",
            value_name = "DIRECTORY",
            help = "Add an exact PACK candidate directory containing pack.toml. Repetition preserves validation order; results remain sorted by PACK name."
        )]
        pack_directories: Vec<PathBuf>,
    },
}

#[derive(Serialize)]
struct InspectPacksResult {
    packs: Vec<PackResult>,
}

#[derive(Serialize)]
struct PackResult {
    name: String,
    title: String,
    description: String,
    owner: String,
}

pub fn run() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let exit_code = error.exit_code();
            let _ = error.print();
            return ExitCode::from(exit_code as u8);
        }
    };

    match cli.operation {
        Operation::Inspect {
            dataset: Some(dataset),
            ..
        } => {
            let prepared = match inspect_dataset(dataset) {
                Ok(result) => response::prepare_success(result),
                Err(error) => response::prepare_cli_failure(miette::Report::new(error)),
            };
            response::publish(prepared)
        }
        Operation::Inspect {
            dataset: None,
            pack_directories,
        } => {
            let prepared = match inspect_packs(pack_directories) {
                Ok(result) => response::prepare_success(result),
                Err(error) => response::prepare_cli_failure(miette::Report::new(error)),
            };
            response::publish(prepared)
        }
    }
}

fn inspect_packs(pack_directories: Vec<PathBuf>) -> Result<InspectPacksResult, InspectPacksError> {
    let skill_root = locate_skill_root()?;
    let data_home = locate_data_home().ok_or(InspectPacksError::DataHomeUnavailable)?;
    let discovered = pack_discovery::discover(PackDiscoveryPaths {
        skill_pack_search_directory: skill_root.join("assets").join("packs"),
        data_home_pack_search_directory: data_home.join("packs"),
        additional_pack_directories: pack_directories,
    })
    .map_err(InspectPacksError::from)?;

    Ok(InspectPacksResult {
        packs: discovered.iter().map(project_pack).collect(),
    })
}

fn locate_data_home() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "KAT")
        .map(|project_dirs| project_dirs.data_dir().to_path_buf())
}

fn project_pack(pack: &DiscoveredPack) -> PackResult {
    PackResult {
        name: pack.name().to_owned(),
        title: pack.title().to_owned(),
        description: pack.description().to_owned(),
        owner: pack.owner().to_owned(),
    }
}

#[derive(Serialize)]
struct InspectDatasetResult {
    path: String,
    tables: Vec<DatasetTableResult>,
}

#[derive(Serialize)]
struct DatasetTableResult {
    name: String,
    columns: Vec<DatasetColumnResult>,
}

#[derive(Serialize)]
struct DatasetColumnResult {
    name: String,
    #[serde(rename = "type")]
    data_type: String,
    nullable: bool,
}

fn inspect_dataset(path: PathBuf) -> Result<InspectDatasetResult, InspectDatasetError> {
    locate_skill_root().map_err(InspectDatasetError::SkillRoot)?;
    let inspection = kat_datasource::inspect_dataset(&path)
        .map_err(|source| InspectDatasetError::Inspection { source })?;
    let canonical_path = inspection
        .path()
        .to_str()
        .ok_or_else(|| InspectDatasetError::NonUnicodePath {
            path: inspection.path().to_path_buf(),
        })?
        .to_owned();
    Ok(InspectDatasetResult {
        path: canonical_path,
        tables: inspection
            .tables()
            .iter()
            .map(|table| DatasetTableResult {
                name: table.name().to_owned(),
                columns: table
                    .columns()
                    .iter()
                    .map(|column| DatasetColumnResult {
                        name: column.name().to_owned(),
                        data_type: column.data_type().to_owned(),
                        nullable: column.nullable(),
                    })
                    .collect(),
            })
            .collect(),
    })
}

fn locate_skill_root() -> Result<PathBuf, SkillRootError> {
    let executable = std::env::current_exe().map_err(SkillRootError::CurrentExecutable)?;
    let payload = executable
        .parent()
        .ok_or_else(|| SkillRootError::InvalidLayout {
            executable: executable.clone(),
        })?;
    let targets = payload
        .parent()
        .ok_or_else(|| SkillRootError::InvalidLayout {
            executable: executable.clone(),
        })?;
    let scripts = targets
        .parent()
        .ok_or_else(|| SkillRootError::InvalidLayout {
            executable: executable.clone(),
        })?;
    let skill = scripts
        .parent()
        .ok_or_else(|| SkillRootError::InvalidLayout {
            executable: executable.clone(),
        })?;
    let expected_binary = if cfg!(windows) { "kat.exe" } else { "kat" };
    if executable.file_name().and_then(|name| name.to_str()) != Some(expected_binary)
        || targets.file_name().and_then(|name| name.to_str()) != Some("targets")
        || scripts.file_name().and_then(|name| name.to_str()) != Some("scripts")
    {
        return Err(SkillRootError::InvalidLayout { executable });
    }

    let marker = skill.join("SKILL.md");
    let metadata = fs::symlink_metadata(&marker).map_err(|source| SkillRootError::SkillMarker {
        path: marker.clone(),
        source,
    })?;
    if !metadata.file_type().is_file() {
        return Err(SkillRootError::SkillMarkerIsNotFile { path: marker });
    }
    dunce::canonicalize(skill).map_err(|source| SkillRootError::CanonicalSkillRoot {
        path: skill.to_path_buf(),
        source,
    })
}

#[derive(Debug, Error)]
enum SkillRootError {
    #[error("failed to locate the current executable")]
    CurrentExecutable(#[source] io::Error),
    #[error("KAT executable is not in <skill>/scripts/targets/<target>: {executable}")]
    InvalidLayout { executable: PathBuf },
    #[error("failed to inspect KAT Skill marker {path}")]
    SkillMarker {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("KAT Skill marker is not a regular file: {path}")]
    SkillMarkerIsNotFile { path: PathBuf },
    #[error("failed to resolve KAT Skill root {path}")]
    CanonicalSkillRoot {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Error, Diagnostic)]
enum InspectPacksError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help(
        "Run kat from <skill>/scripts/targets/<target> with a regular <skill>/SKILL.md marker"
    ))]
    SkillRoot(
        #[from]
        #[source]
        SkillRootError,
    ),
    #[error("KAT Data Home is unavailable on this platform")]
    #[diagnostic(help("Run KAT on Linux or Windows with a platform standard user data directory"))]
    DataHomeUnavailable,
    #[error("PACK discovery failed")]
    #[diagnostic(help("Correct the first invalid PACK candidate and retry"))]
    Discovery {
        #[source]
        source: pack_discovery::PackDiscoveryError,
    },
    #[error("PACK discovery failed")]
    #[diagnostic(help(
        "Make the default PACK search path a readable directory or remove it, then retry"
    ))]
    DefaultPackSearchPath {
        #[source]
        source: pack_discovery::PackDiscoveryError,
    },
    #[error("PACK discovery failed")]
    #[diagnostic(help("Remove one conflicting PACK or give the PACKs distinct names, then retry"))]
    DuplicatePackName {
        #[source]
        source: pack_discovery::PackDiscoveryError,
    },
}

impl From<pack_discovery::PackDiscoveryError> for InspectPacksError {
    fn from(source: pack_discovery::PackDiscoveryError) -> Self {
        match source {
            source @ pack_discovery::PackDiscoveryError::DuplicatePackName { .. } => {
                Self::DuplicatePackName { source }
            }
            source @ pack_discovery::PackDiscoveryError::ReadSearchDirectory { .. }
            | source @ pack_discovery::PackDiscoveryError::EnumerateSearchDirectory { .. }
            | source @ pack_discovery::PackDiscoveryError::InspectSearchEntry { .. } => {
                Self::DefaultPackSearchPath { source }
            }
            source => Self::Discovery { source },
        }
    }
}

#[derive(Debug, Error, Diagnostic)]
enum InspectDatasetError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
    #[error("Dataset inspection failed")]
    #[diagnostic(help("Provide a complete KAT Dataset directory and retry"))]
    Inspection {
        #[source]
        source: kat_datasource::DatasetInspectionError,
    },
    #[error("Dataset path cannot be represented as native Unicode: {path:?}")]
    NonUnicodePath { path: PathBuf },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_ordered_repeated_pack_directories() {
        let cli = Cli::try_parse_from([
            "kat",
            "inspect",
            "--pack-dir",
            "first",
            "--pack-dir",
            "second",
        ])
        .expect("parse inspect");

        let Operation::Inspect {
            dataset,
            pack_directories,
        } = cli.operation;
        assert!(dataset.is_none());
        assert_eq!(
            pack_directories,
            [PathBuf::from("first"), PathBuf::from("second")]
        );
    }

    #[test]
    fn parser_rejects_bare_and_unknown_operations() {
        assert!(Cli::try_parse_from(["kat"]).is_err());
        assert!(Cli::try_parse_from(["kat", "list"]).is_err());
        assert!(Cli::try_parse_from(["kat", "inspect", "--version"]).is_err());
    }
}
