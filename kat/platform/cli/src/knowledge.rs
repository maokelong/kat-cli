use std::{
    fs, io,
    path::{Path, PathBuf},
};

use miette::{IntoDiagnostic, Result, WrapErr, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    pack_discovery::{DiscoveredPack, PackDiscoveryPaths, validate_pack_name},
    response::{self, PreparedResponse},
};

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    #[serde(deserialize_with = "text_field")]
    pub(super) name: String,
    #[serde(deserialize_with = "text_field")]
    pub(super) description: String,
    #[serde(
        default,
        rename = "user-invocable",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) user_invocable: Option<bool>,
    #[serde(
        default,
        rename = "disable-model-invocation",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) disable_model_invocation: Option<bool>,
}

fn text_field<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    match serde_yaml_ng::Value::deserialize(deserializer)? {
        serde_yaml_ng::Value::String(value) => Ok(value),
        _ => Err(serde::de::Error::custom(
            "knowledge declaration field must be a string",
        )),
    }
}

pub(super) struct Document {
    pub(super) path: String,
    pub(super) entry: Option<Entry>,
    pub(super) content: String,
}

pub(super) enum Selection<'a> {
    Entry { pack: &'a str, name: &'a str },
    Reference { pack: &'a str, path: &'a str },
}

pub(super) struct Adoption<'a> {
    pub(super) selection: Selection<'a>,
    pub(super) update_revision: Option<u64>,
}

impl Selection<'_> {
    pub(super) fn pack(&self) -> &str {
        match self {
            Self::Entry { pack, .. } | Self::Reference { pack, .. } => pack,
        }
    }

    pub(super) fn validate(&self) -> Result<()> {
        validate_pack_name(self.pack(), Path::new("pack.toml")).into_diagnostic()?;
        match self {
            Self::Entry { name, .. } => validate_name(name),
            Self::Reference { path, .. } => validate_reference_path(path),
        }
    }

    pub(super) fn load(&self, directories: Vec<PathBuf>) -> Result<Document> {
        let pack = select_pack(self.pack(), directories)?;
        match self {
            Self::Entry { name, .. } => read_entry(pack.directory(), name),
            Self::Reference { path, .. } => read_reference(pack.directory(), path),
        }
    }
}

pub(super) fn inspect(
    pack: &str,
    entry: Option<&str>,
    path: Option<&str>,
    directories: Vec<PathBuf>,
) -> PreparedResponse<Value> {
    let result = (|| {
        let selected = select_pack(pack, directories)?;
        if let Some(name) = entry {
            let document = read_entry(selected.directory(), name)?;
            let declaration = document
                .entry
                .expect("read_entry always returns a declaration");
            Ok(
                json!({"entry":{"name":declaration.name,"description":declaration.description,"path":document.path,"content":document.content}}),
            )
        } else if let Some(path) = path {
            let document = read_reference(selected.directory(), path)?;
            Ok(json!({"reference":{"path":document.path,"content":document.content}}))
        } else {
            Ok(json!({"entries":list(selected.directory())?}))
        }
    })();
    match result {
        Ok(result) => response::prepare_success(result),
        Err(error) => response::prepare_cli_failure(error),
    }
}

pub(super) fn select_pack(name: &str, directories: Vec<PathBuf>) -> Result<DiscoveredPack> {
    let root = crate::locate_skill_root().into_diagnostic()?;
    let home = crate::locate_data_home().into_diagnostic()?;
    let packs = crate::sdk::discover(PackDiscoveryPaths {
        skill_pack_search_directory: root.join("assets/packs"),
        data_home_pack_search_directory: home.join("packs"),
        additional_pack_directories: directories,
    })
    .into_diagnostic()
    .wrap_err("PACK discovery failed")?;
    packs
        .into_pack(name)
        .ok_or_else(|| miette::miette!("unknown PACK: {name}"))
}

fn list(root: &Path) -> Result<Vec<Entry>> {
    let directory = root.join("skills");
    let members = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err("cannot enumerate knowledge entries");
        }
    };
    let mut members = members
        .collect::<std::result::Result<Vec<_>, _>>()
        .into_diagnostic()?;
    members.sort_by_key(fs::DirEntry::path);
    let mut entries = Vec::new();
    for member in members {
        if !member.file_type().into_diagnostic()?.is_dir() {
            continue;
        }
        match fs::symlink_metadata(member.path().join("SKILL.md")) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err("cannot inspect knowledge declaration");
            }
        }
        let name = member.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| miette::miette!("knowledge entry name must be UTF-8"))?;
        let document = read_entry(root, name)?;
        entries.push(
            document
                .entry
                .expect("read_entry always returns a declaration"),
        );
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

pub(super) fn read_entry(root: &Path, name: &str) -> Result<Document> {
    validate_name(name)?;
    let path = format!("skills/{name}/SKILL.md");
    let content = read_text(root, &path, false)?;
    let entry = declaration(name, &content)?;
    Ok(Document {
        path,
        entry: Some(entry),
        content,
    })
}

pub(super) fn declaration(name: &str, content: &str) -> Result<Entry> {
    validate_name(name)?;
    let mut lines = content.split_inclusive('\n');
    if lines.next().map(|line| line.trim_end_matches(['\r', '\n'])) != Some("---") {
        bail!("knowledge entry must start with YAML frontmatter");
    }
    let mut header = String::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            closed = true;
            break;
        }
        header.push_str(line);
    }
    if !closed {
        bail!("knowledge entry frontmatter is not closed");
    }
    let mut entry: Entry = serde_yaml_ng::from_str(&header)
        .into_diagnostic()
        .wrap_err("invalid knowledge declaration")?;
    if entry.name != name {
        bail!("knowledge declaration name must match entry directory {name:?}");
    }
    entry.description = entry.description.trim().to_owned();
    if entry.description.is_empty() {
        bail!("knowledge description must not be empty");
    }
    if lines.collect::<String>().trim().is_empty() {
        bail!("knowledge entry body must not be empty");
    }
    Ok(entry)
}

pub(super) fn validate_name(name: &str) -> Result<()> {
    validate_pack_name(name, Path::new("SKILL.md"))
        .into_diagnostic()
        .wrap_err("invalid knowledge entry name")
}

pub(super) fn validate_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains(['\\', ':', '\0'])
        || path.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' '])
        })
        || Path::new(path).is_absolute()
    {
        bail!("knowledge path must be a normalized PACK-relative path using '/'");
    }
    Ok(())
}

pub(super) fn read_reference(root: &Path, path: &str) -> Result<Document> {
    validate_reference_path(path)?;
    Ok(Document {
        path: path.to_owned(),
        entry: None,
        content: read_text(root, path, true)?,
    })
}

pub(super) fn validate_reference_path(path: &str) -> Result<()> {
    validate_path(path)?;
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() == 3
        && parts[0].eq_ignore_ascii_case("skills")
        && parts[2].eq_ignore_ascii_case("SKILL.md")
    {
        bail!("read a SKILL.md declaration with --entry, not --path");
    }
    Ok(())
}

fn read_text(root: &Path, path: &str, reference: bool) -> Result<String> {
    validate_path(path)?;
    let resolved = dunce::canonicalize(root.join(path))
        .into_diagnostic()
        .wrap_err_with(|| format!("cannot resolve knowledge file {path}"))?;
    let relative = resolved
        .strip_prefix(root)
        .into_diagnostic()
        .wrap_err("knowledge file escapes its PACK")?;
    if reference {
        let normalized = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        validate_reference_path(&normalized)?;
    }
    if let Some(extension) = resolved
        .extension()
        .and_then(|extension| extension.to_str())
        && matches!(
            extension.to_ascii_lowercase().as_str(),
            "pdf"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "svg"
                | "zip"
                | "gz"
                | "exe"
                | "dll"
                | "parquet"
                | "bin"
        )
    {
        bail!("knowledge supports UTF-8 text, not binary or image files: {path}");
    }
    if !fs::metadata(&resolved).into_diagnostic()?.is_file() {
        bail!("knowledge must be a regular file: {path}");
    }
    let content = fs::read_to_string(resolved)
        .into_diagnostic()
        .wrap_err_with(|| format!("cannot read UTF-8 knowledge file {path}"))?;
    if content.contains('\0') {
        bail!("knowledge file contains binary data: {path}");
    }
    Ok(content)
}
