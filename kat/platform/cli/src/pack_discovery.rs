use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

pub(crate) struct PackDiscoveryPaths {
    pub(crate) skill_pack_search_directory: PathBuf,
    pub(crate) data_home_pack_search_directory: PathBuf,
    pub(crate) additional_pack_directories: Vec<PathBuf>,
}

pub(crate) struct DiscoveredPacks {
    packs: BTreeMap<String, DiscoveredPack>,
}

impl DiscoveredPacks {
    pub(crate) fn iter(&self) -> impl Iterator<Item = &DiscoveredPack> {
        self.packs.values()
    }

    // 精确目标选择由后续 PACK inspection/run/test 切片接入。
    #[allow(dead_code)]
    pub(crate) fn get(&self, name: &str) -> Option<&DiscoveredPack> {
        self.packs.get(name)
    }
}

pub(crate) struct DiscoveredPack {
    name: String,
    title: String,
    description: String,
    owner: String,
    directory: PathBuf,
}

impl DiscoveredPack {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    pub(crate) fn owner(&self) -> &str {
        &self.owner
    }

    // canonical directory 只交给后续私有 Runtime request，不进入公开列表。
    #[allow(dead_code)]
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    name: String,
    title: String,
    description: String,
    owner: String,
}

pub(crate) fn discover(paths: PackDiscoveryPaths) -> Result<DiscoveredPacks, PackDiscoveryError> {
    let mut state = DiscoveryState::default();
    discover_search_directory(&paths.skill_pack_search_directory, &mut state)?;
    discover_search_directory(&paths.data_home_pack_search_directory, &mut state)?;
    for directory in paths.additional_pack_directories {
        state.discover_candidate(&directory)?;
    }

    Ok(DiscoveredPacks { packs: state.packs })
}

#[derive(Default)]
struct DiscoveryState {
    directories: BTreeSet<PathBuf>,
    packs: BTreeMap<String, DiscoveredPack>,
}

impl DiscoveryState {
    fn discover_candidate(&mut self, directory: &Path) -> Result<(), PackDiscoveryError> {
        let canonical_directory = dunce::canonicalize(directory).map_err(|source| {
            PackDiscoveryError::CanonicalizePackDirectory {
                path: directory.to_path_buf(),
                source,
            }
        })?;
        let metadata = fs::metadata(&canonical_directory).map_err(|source| {
            PackDiscoveryError::InspectPackDirectory {
                path: canonical_directory.clone(),
                source,
            }
        })?;
        if !metadata.is_dir() {
            return Err(PackDiscoveryError::PackPathIsNotDirectory {
                path: canonical_directory,
            });
        }
        if !self.directories.insert(canonical_directory.clone()) {
            return Ok(());
        }

        let manifest_path = canonical_directory.join("pack.toml");
        let contents = fs::read_to_string(&manifest_path).map_err(|source| {
            PackDiscoveryError::ReadManifest {
                path: manifest_path.clone(),
                source,
            }
        })?;
        let manifest: Manifest =
            toml::from_str(&contents).map_err(|source| PackDiscoveryError::ParseManifest {
                path: manifest_path.clone(),
                source,
            })?;
        validate_pack_name(&manifest.name, &manifest_path)?;
        let title =
            normalized_display_field("title", manifest.title, &manifest.name, &manifest_path)?;
        let description = normalized_display_field(
            "description",
            manifest.description,
            &manifest.name,
            &manifest_path,
        )?;
        let owner =
            normalized_display_field("owner", manifest.owner, &manifest.name, &manifest_path)?;
        let pack = DiscoveredPack {
            name: manifest.name.clone(),
            title,
            description,
            owner,
            directory: canonical_directory.clone(),
        };

        if let Some(previous) = self.packs.get(&manifest.name) {
            return Err(PackDiscoveryError::DuplicatePackName {
                name: manifest.name,
                first_directory: previous.directory.clone(),
                second_directory: canonical_directory,
            });
        }
        self.packs.insert(manifest.name, pack);
        Ok(())
    }
}

fn discover_search_directory(
    search_directory: &Path,
    state: &mut DiscoveryState,
) -> Result<(), PackDiscoveryError> {
    let entries = match fs::read_dir(search_directory) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(PackDiscoveryError::ReadSearchDirectory {
                path: search_directory.to_path_buf(),
                source,
            });
        }
    };
    let mut entries = entries.collect::<Result<Vec<_>, _>>().map_err(|source| {
        PackDiscoveryError::EnumerateSearchDirectory {
            path: search_directory.to_path_buf(),
            source,
        }
    })?;
    entries.sort_by_key(fs::DirEntry::path);

    for entry in entries {
        let entry_path = entry.path();
        let file_type =
            entry
                .file_type()
                .map_err(|source| PackDiscoveryError::InspectSearchEntry {
                    path: entry_path.clone(),
                    source,
                })?;
        if !file_type.is_dir() {
            continue;
        }
        match fs::symlink_metadata(entry_path.join("pack.toml")) {
            Ok(_) => state.discover_candidate(&entry_path)?,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(PackDiscoveryError::InspectManifestPath {
                    path: entry_path.join("pack.toml"),
                    source,
                });
            }
        }
    }
    Ok(())
}

fn validate_pack_name(name: &str, manifest_path: &Path) -> Result<(), PackDiscoveryError> {
    let valid = !name.is_empty()
        && name.split('-').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        });
    if !valid {
        return Err(PackDiscoveryError::InvalidPackName {
            path: manifest_path.to_path_buf(),
            name: name.to_owned(),
            reason: PackNameInvalidity::NotLowercaseAsciiKebabCase,
        });
    }
    if is_windows_device_name(name) {
        return Err(PackDiscoveryError::InvalidPackName {
            path: manifest_path.to_path_buf(),
            name: name.to_owned(),
            reason: PackNameInvalidity::WindowsDeviceName,
        });
    }
    Ok(())
}

fn is_windows_device_name(name: &str) -> bool {
    matches!(name, "con" | "prn" | "aux" | "nul")
        || (name.len() == 4
            && (name.starts_with("com") || name.starts_with("lpt"))
            && matches!(name.as_bytes()[3], b'1'..=b'9'))
}

fn normalized_display_field(
    field: &'static str,
    value: String,
    pack_name: &str,
    manifest_path: &Path,
) -> Result<String, PackDiscoveryError> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(PackDiscoveryError::EmptyManifestField {
            path: manifest_path.to_path_buf(),
            name: pack_name.to_owned(),
            field,
        });
    }
    Ok(normalized.to_owned())
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PackDiscoveryError {
    #[error("failed to read default PACK search directory {path}")]
    ReadSearchDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed while enumerating default PACK search directory {path}")]
    EnumerateSearchDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to inspect default PACK search entry {path}")]
    InspectSearchEntry {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to inspect PACK manifest path {path}")]
    InspectManifestPath {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to resolve PACK directory {path}")]
    CanonicalizePackDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to inspect PACK directory {path}")]
    InspectPackDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("PACK path is not a directory: {path}")]
    PackPathIsNotDirectory { path: PathBuf },
    #[error("failed to read PACK manifest {path}")]
    ReadManifest {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse PACK manifest {path}")]
    ParseManifest {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("invalid PACK name {name:?} in {path}: {reason}")]
    InvalidPackName {
        path: PathBuf,
        name: String,
        reason: PackNameInvalidity,
    },
    #[error("PACK manifest field {field:?} must not be empty for {name:?} in {path}")]
    EmptyManifestField {
        path: PathBuf,
        name: String,
        field: &'static str,
    },
    #[error("duplicate PACK name {name:?} in {first_directory} and {second_directory}")]
    DuplicatePackName {
        name: String,
        first_directory: PathBuf,
        second_directory: PathBuf,
    },
}

#[derive(Debug)]
pub(crate) enum PackNameInvalidity {
    NotLowercaseAsciiKebabCase,
    WindowsDeviceName,
}

impl std::fmt::Display for PackNameInvalidity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotLowercaseAsciiKebabCase => {
                formatter.write_str("expected lowercase ASCII kebab-case")
            }
            Self::WindowsDeviceName => formatter.write_str("reserved Windows device name"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error, fs, path::Path};

    use super::*;

    fn write_pack(directory: &std::path::Path, manifest: &str) {
        fs::create_dir_all(directory).expect("create PACK directory");
        fs::write(directory.join("pack.toml"), manifest).expect("write PACK manifest");
    }

    fn valid_manifest(name: &str) -> String {
        format!(
            "name = {name:?}\ntitle = \"Title\"\ndescription = \"Description\"\nowner = \"Owner\"\n"
        )
    }

    fn discovery_paths(root: &Path) -> PackDiscoveryPaths {
        PackDiscoveryPaths {
            skill_pack_search_directory: root.join("skill-packs"),
            data_home_pack_search_directory: root.join("data-packs"),
            additional_pack_directories: Vec::new(),
        }
    }

    fn expect_discovery_error(
        result: Result<DiscoveredPacks, PackDiscoveryError>,
    ) -> PackDiscoveryError {
        match result {
            Ok(_) => panic!("expected PACK discovery to fail"),
            Err(error) => error,
        }
    }

    #[test]
    fn missing_default_search_directories_produce_an_empty_result() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let discovered = discover(PackDiscoveryPaths {
            skill_pack_search_directory: temporary.path().join("skill-packs"),
            data_home_pack_search_directory: temporary.path().join("data-packs"),
            additional_pack_directories: Vec::new(),
        })
        .expect("missing default PACK directories are empty search locations");

        assert_eq!(discovered.iter().count(), 0);
    }

    #[test]
    fn discovers_default_and_exact_additional_candidates_by_manifest_name() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let skill_packs = temporary.path().join("skill-packs");
        let data_packs = temporary.path().join("data-packs");
        let additional = temporary.path().join("checkout-with-an-unrelated-name");
        write_pack(
            &skill_packs.join("z-directory"),
            r#"
name = "bravo"
title = "  Bravo title  "
description = "\nBravo description\t"
owner = " Bravo Team "
"#,
        );
        write_pack(
            &data_packs.join("a-directory"),
            r#"
name = "alpha"
title = "Alpha title"
description = "Alpha description"
owner = "Alpha Team"
"#,
        );
        write_pack(
            &additional,
            r#"
name = "kat-charlie"
title = "Charlie title"
description = "Charlie description"
owner = "Charlie Team"
"#,
        );

        let discovered = discover(PackDiscoveryPaths {
            skill_pack_search_directory: skill_packs,
            data_home_pack_search_directory: data_packs,
            additional_pack_directories: vec![additional.clone()],
        })
        .expect("discover valid PACKs");

        let names = discovered
            .iter()
            .map(DiscoveredPack::name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["alpha", "bravo", "kat-charlie"]);
        let bravo = discovered.get("bravo").expect("find PACK by exact name");
        assert_eq!(bravo.title(), "Bravo title");
        assert_eq!(bravo.description(), "Bravo description");
        assert_eq!(bravo.owner(), "Bravo Team");
        let charlie = discovered.get("kat-charlie").expect("find kat- PACK");
        assert_eq!(
            charlie.directory(),
            dunce::canonicalize(additional).unwrap()
        );
        assert!(discovered.get("Kat-Charlie").is_none());
    }

    #[test]
    fn repeated_canonical_directory_is_idempotent() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let candidate = temporary.path().join("skill-packs").join("candidate");
        write_pack(&candidate, &valid_manifest("same-pack"));
        let alias = candidate.join("..").join("candidate");

        let discovered = discover(PackDiscoveryPaths {
            skill_pack_search_directory: temporary.path().join("skill-packs"),
            data_home_pack_search_directory: temporary.path().join("data-packs"),
            additional_pack_directories: vec![candidate.clone(), alias, candidate],
        })
        .expect("the same canonical directory is one candidate");

        assert_eq!(
            discovered
                .iter()
                .map(DiscoveredPack::name)
                .collect::<Vec<_>>(),
            ["same-pack"]
        );
    }

    #[test]
    fn different_directories_with_the_same_name_fail_the_whole_discovery() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let first = temporary.path().join("skill-packs").join("first");
        let second = temporary.path().join("data-packs").join("second");
        write_pack(&first, &valid_manifest("duplicate"));
        write_pack(&second, &valid_manifest("duplicate"));

        let error = expect_discovery_error(discover(discovery_paths(temporary.path())));

        match error {
            PackDiscoveryError::DuplicatePackName {
                name,
                first_directory,
                second_directory,
            } => {
                assert_eq!(name, "duplicate");
                assert_eq!(first_directory, dunce::canonicalize(first).unwrap());
                assert_eq!(second_directory, dunce::canonicalize(second).unwrap());
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn manifest_has_exactly_four_root_string_fields() {
        let invalid_manifests = [
            "name = \"alpha\"\ntitle = \"Title\"\ndescription = \"Description\"\nowner = \"Owner\"\nversion = \"1\"\n",
            "[pack]\nname = \"alpha\"\ntitle = \"Title\"\ndescription = \"Description\"\nowner = \"Owner\"\n",
            "name = \"alpha\"\ntitle = \"Title\"\ndescription = \"Description\"\n",
            "name = \"alpha\"\ntitle = 1\ndescription = \"Description\"\nowner = \"Owner\"\n",
        ];

        for (index, manifest) in invalid_manifests.into_iter().enumerate() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let candidate = temporary.path().join(format!("candidate-{index}"));
            write_pack(&candidate, manifest);
            let error = expect_discovery_error(discover(PackDiscoveryPaths {
                skill_pack_search_directory: temporary.path().join("missing-skill"),
                data_home_pack_search_directory: temporary.path().join("missing-data"),
                additional_pack_directories: vec![candidate],
            }));

            assert!(matches!(error, PackDiscoveryError::ParseManifest { .. }));
            assert!(error.source().is_some(), "parse error keeps its source");
        }
    }

    #[test]
    fn display_fields_must_be_non_empty_after_unicode_trim() {
        for (field, original, toml_value) in [
            ("title", "title = \"Title\"", "\" \\u2003 \""),
            ("description", "description = \"Description\"", "\"\\n\\t\""),
            ("owner", "owner = \"Owner\"", "\"  \""),
        ] {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let candidate = temporary.path().join(field);
            let manifest =
                valid_manifest("alpha").replace(original, &format!("{field} = {toml_value}"));
            write_pack(&candidate, &manifest);

            let error = expect_discovery_error(discover(PackDiscoveryPaths {
                skill_pack_search_directory: temporary.path().join("missing-skill"),
                data_home_pack_search_directory: temporary.path().join("missing-data"),
                additional_pack_directories: vec![candidate],
            }));

            assert!(matches!(
                error,
                PackDiscoveryError::EmptyManifestField {
                    field: actual,
                    name,
                    ..
                } if actual == field && name == "alpha"
            ));
        }
    }

    #[test]
    fn pack_name_is_portable_lowercase_ascii_kebab_case() {
        for invalid_name in [
            "",
            "Alpha",
            "alpha_beta",
            "alpha--beta",
            "-alpha",
            "alpha-",
            "café",
            "con",
            "com1",
            "lpt9",
        ] {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let candidate = temporary.path().join("candidate");
            write_pack(&candidate, &valid_manifest(invalid_name));

            let error = expect_discovery_error(discover(PackDiscoveryPaths {
                skill_pack_search_directory: temporary.path().join("missing-skill"),
                data_home_pack_search_directory: temporary.path().join("missing-data"),
                additional_pack_directories: vec![candidate],
            }));

            assert!(matches!(
                error,
                PackDiscoveryError::InvalidPackName { name, .. } if name == invalid_name
            ));
        }
    }

    #[test]
    fn default_candidates_are_validated_in_stable_path_order() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let search = temporary.path().join("skill-packs");
        let first = search.join("a-broken");
        let later = search.join("z-broken");
        write_pack(&later, "not valid TOML = [");
        write_pack(&first, "also not valid TOML = [");

        let error = expect_discovery_error(discover(PackDiscoveryPaths {
            skill_pack_search_directory: search,
            data_home_pack_search_directory: temporary.path().join("data-packs"),
            additional_pack_directories: Vec::new(),
        }));

        assert!(matches!(
            error,
            PackDiscoveryError::ParseManifest { path, .. }
                if path == dunce::canonicalize(first).unwrap().join("pack.toml")
        ));
    }

    #[test]
    fn search_sources_fail_fast_in_declared_order() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let skill_candidate = temporary.path().join("skill-packs").join("broken");
        let data_candidate = temporary.path().join("data-packs").join("broken");
        let additional_candidate = temporary.path().join("additional");
        write_pack(&skill_candidate, "broken = [");
        write_pack(&data_candidate, "broken = [");
        write_pack(&additional_candidate, "broken = [");

        let error = expect_discovery_error(discover(PackDiscoveryPaths {
            skill_pack_search_directory: temporary.path().join("skill-packs"),
            data_home_pack_search_directory: temporary.path().join("data-packs"),
            additional_pack_directories: vec![additional_candidate],
        }));

        assert!(matches!(
            error,
            PackDiscoveryError::ParseManifest { path, .. }
                if path == dunce::canonicalize(skill_candidate).unwrap().join("pack.toml")
        ));
    }

    #[test]
    fn explicit_pack_directory_is_not_a_search_root() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let explicit = temporary.path().join("explicit");
        write_pack(&explicit.join("nested"), &valid_manifest("nested"));

        let error = expect_discovery_error(discover(PackDiscoveryPaths {
            skill_pack_search_directory: temporary.path().join("missing-skill"),
            data_home_pack_search_directory: temporary.path().join("missing-data"),
            additional_pack_directories: vec![explicit.clone()],
        }));

        assert!(matches!(
            error,
            PackDiscoveryError::ReadManifest { path, source }
                if path == dunce::canonicalize(explicit).unwrap().join("pack.toml")
                    && source.kind() == std::io::ErrorKind::NotFound
        ));
    }

    #[test]
    fn default_search_ignores_non_candidates_without_recursing() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let search = temporary.path().join("skill-packs");
        fs::create_dir_all(search.join("ordinary").join("nested")).unwrap();
        fs::write(search.join("ordinary-file"), "ignored").unwrap();
        write_pack(
            &search.join("ordinary").join("nested"),
            &valid_manifest("nested"),
        );

        let discovered = discover(PackDiscoveryPaths {
            skill_pack_search_directory: search,
            data_home_pack_search_directory: temporary.path().join("missing-data"),
            additional_pack_directories: Vec::new(),
        })
        .expect("non-candidates are ignored");

        assert_eq!(discovered.iter().count(), 0);
    }
}
