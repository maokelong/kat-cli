use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use blazesym::symbolize::{
    CodeInfo, Input, Symbolized, Symbolizer,
    source::{Elf, Source},
};
use walkdir::WalkDir;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleAddress {
    pub module: String,
    pub address: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingModule {
    pub module_path: String,
    pub occurrence_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolizationResult {
    pub symbols: Vec<String>,
    pub missing_modules: Vec<MissingModule>,
}

pub fn get_symbols(
    addr_list: &[String],
    symbol_dir: &Path,
    module_name_map: &HashMap<String, String>,
    include_source_location: bool,
) -> Result<SymbolizationResult> {
    if addr_list.is_empty() {
        return Ok(SymbolizationResult {
            symbols: Vec::new(),
            missing_modules: Vec::new(),
        });
    }
    let files = index_symbol_files(symbol_dir)?;
    let module_name_map = normalize_module_name_map(module_name_map)?;
    let mut output = addr_list.to_vec();
    let mut groups: BTreeMap<PathBuf, BTreeMap<u64, Vec<usize>>> = BTreeMap::new();
    let mut selected_modules: HashMap<String, Option<PathBuf>> = HashMap::new();
    let mut missing_modules: BTreeMap<String, usize> = BTreeMap::new();

    for (position, input) in addr_list.iter().enumerate() {
        let Ok(query) = parse_module_address(input) else {
            continue;
        };
        let mapped_module = map_module_name(&query.module, &module_name_map);
        let module = selected_modules
            .entry(query.module.clone())
            .or_insert_with(|| select_module(&files, mapped_module));
        let Some(module) = module.clone() else {
            *missing_modules.entry(query.module).or_default() += 1;
            continue;
        };
        groups
            .entry(module)
            .or_default()
            .entry(query.address)
            .or_default()
            .push(position);
    }

    let symbolizer = Symbolizer::builder()
        .enable_code_info(include_source_location)
        .enable_inlined_fns(include_source_location)
        .enable_demangling(true)
        .build();
    for (elf_path, addresses) in groups {
        let unique = addresses.keys().copied().collect::<Vec<_>>();
        let source = Source::Elf(Elf::new(&elf_path));
        let results = match symbolizer.symbolize(&source, Input::VirtOffset(&unique)) {
            Ok(results) => results,
            Err(error) => {
                eprintln!(
                    "warning: cannot symbolize {}: {error:#}",
                    elf_path.display()
                );
                continue;
            }
        };
        for (address, result) in unique.into_iter().zip(results) {
            let Symbolized::Sym(symbol) = result else {
                continue;
            };
            let formatted = format_symbol(&symbol, include_source_location);
            for position in &addresses[&address] {
                output[*position] = formatted.clone();
            }
        }
    }
    Ok(SymbolizationResult {
        symbols: output,
        missing_modules: missing_modules
            .into_iter()
            .map(|(module_path, occurrence_count)| MissingModule {
                module_path,
                occurrence_count,
            })
            .collect(),
    })
}

fn normalize_module_name_map(
    module_name_map: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let mut normalized = HashMap::with_capacity(module_name_map.len());
    for (source, target) in module_name_map {
        let source = normalize(source.trim());
        let target = normalize(target.trim());
        if source.is_empty()
            || target.is_empty()
            || !is_shared_object(&source)
            || !is_shared_object(&target)
        {
            bail!("invalid module name mapping: {source}={target}");
        }
        if let Some(previous) = normalized.insert(source.clone(), target.clone())
            && previous != target
        {
            bail!("conflicting module name mapping for {source}: {previous} and {target}");
        }
    }
    Ok(normalized)
}

fn map_module_name<'a>(module: &'a str, module_name_map: &'a HashMap<String, String>) -> &'a str {
    module_name_map
        .get(module)
        .or_else(|| {
            module
                .rsplit('/')
                .next()
                .and_then(|basename| module_name_map.get(basename))
        })
        .map(String::as_str)
        .unwrap_or(module)
}

pub fn parse_module_address(input: &str) -> Result<ModuleAddress> {
    let value = input.trim();
    let (module, address) = value
        .rsplit_once("+0x")
        .with_context(|| format!("expected MODULE+0xHEX: {value}"))?;
    if module.is_empty() || !is_shared_object(module) || address.is_empty() {
        bail!("invalid symbol query: {value}");
    }
    let address = u64::from_str_radix(address, 16)
        .with_context(|| format!("invalid hexadecimal address: {address}"))?;
    Ok(ModuleAddress {
        module: normalize(module),
        address,
    })
}

fn is_shared_object(module: &str) -> bool {
    let name = module.rsplit(['/', '\\']).next().unwrap_or(module);
    name.ends_with(".so")
        || name.rsplit_once(".so.").is_some_and(|(_, version)| {
            !version.is_empty()
                && version
                    .split('.')
                    .all(|p| p.chars().all(|c| c.is_ascii_digit()))
        })
}

fn index_symbol_files(symbol_dir: &Path) -> Result<Vec<PathBuf>> {
    if !symbol_dir.is_dir() {
        bail!(
            "symbol directory does not exist or is not a directory: {}",
            symbol_dir.display()
        );
    }
    std::fs::read_dir(symbol_dir)
        .with_context(|| format!("cannot open symbol directory {}", symbol_dir.display()))?;
    let mut files = Vec::new();
    for entry in WalkDir::new(symbol_dir).follow_links(false) {
        match entry {
            Ok(entry)
                if (entry.file_type().is_file()
                    || (entry.file_type().is_symlink() && entry.path().is_file()))
                    && is_shared_object(&entry.file_name().to_string_lossy()) =>
            {
                files.push(entry.into_path());
            }
            Ok(_) => {}
            Err(error) => eprintln!("warning: skipping inaccessible symbol path: {error}"),
        }
    }
    files.sort_by_key(|path| normalize(&path.to_string_lossy()));
    Ok(files)
}

fn select_module(files: &[PathBuf], requested: &str) -> Option<PathBuf> {
    let requested = normalize(requested);
    let basename = requested.rsplit('/').next()?;
    let suffix = files
        .iter()
        .filter(|path| normalize(&path.to_string_lossy()).ends_with(&requested))
        .cloned()
        .collect::<Vec<_>>();
    let candidates = if suffix.is_empty() {
        files
            .iter()
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy() == basename)
            })
            .cloned()
            .collect::<Vec<_>>()
    } else {
        suffix
    };
    if candidates.len() > 1 {
        eprintln!(
            "warning: multiple symbol modules match {requested}; using {}",
            candidates[0].display()
        );
    }
    candidates.into_iter().next()
}

fn normalize(value: &str) -> String {
    value.replace('\\', "/")
}

fn format_symbol(symbol: &blazesym::symbolize::Sym<'_>, include_source: bool) -> String {
    let mut frames = vec![format!("{}+0x{:x}", symbol.name, symbol.offset)];
    if include_source {
        append_location(&mut frames[0], symbol.code_info.as_deref());
        frames.extend(symbol.inlined.iter().map(|inline| {
            let mut frame = inline.name.to_string();
            append_location(&mut frame, inline.code_info.as_ref());
            frame
        }));
    }
    frames.join(" => ")
}

fn append_location(output: &mut String, code_info: Option<&CodeInfo<'_>>) {
    let Some(code_info) = code_info else { return };
    let Some(line) = code_info.line else { return };
    output.push_str(&format!(" ({}:{line})", code_info.to_path().display()));
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::Path};

    use super::{
        MissingModule, get_symbols, index_symbol_files, normalize_module_name_map,
        parse_module_address,
    };

    #[test]
    fn parses_only_strict_shared_object_queries() {
        assert_eq!(
            parse_module_address(" libfoo.so+0xA2 ").unwrap().address,
            0xa2
        );
        assert!(parse_module_address("libfoo.so.1.2+0xff").is_ok());
        assert!(parse_module_address("libfoo.so+42").is_err());
        assert!(parse_module_address("libfoo.a+0x42").is_err());
    }

    #[test]
    fn empty_input_does_not_validate_directory() {
        let result = get_symbols(&[], Path::new("missing"), &HashMap::new(), false).unwrap();
        assert!(result.symbols.is_empty());
        assert!(result.missing_modules.is_empty());
    }

    #[test]
    fn non_empty_input_rejects_missing_symbol_directory() {
        let inputs = vec!["libsymbols.so+0x1".to_owned()];

        let result = get_symbols(&inputs, Path::new("missing"), &HashMap::new(), false);

        assert!(result.is_err());
    }

    #[test]
    fn returns_missing_modules_with_occurrence_counts() {
        let directory = tempfile::tempdir().unwrap();
        let inputs = vec![
            "/system/lib/libmissing.so+0x1".to_owned(),
            "/system/lib/libmissing.so+0x2".to_owned(),
            "/vendor/lib/libother.so+0x3".to_owned(),
            "not a symbol query".to_owned(),
        ];

        let result = get_symbols(&inputs, directory.path(), &HashMap::new(), false).unwrap();

        assert_eq!(result.symbols, inputs);
        assert_eq!(
            result.missing_modules,
            vec![
                MissingModule {
                    module_path: "/system/lib/libmissing.so".to_owned(),
                    occurrence_count: 2,
                },
                MissingModule {
                    module_path: "/vendor/lib/libother.so".to_owned(),
                    occurrence_count: 1,
                },
            ]
        );
    }

    #[test]
    fn maps_trace_module_name_before_selecting_symbol_file() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("libsymbols.so"), []).unwrap();
        let inputs = vec!["/system/lib/libtrace.so+0x1".to_owned()];
        let module_name_map =
            HashMap::from([("libtrace.so".to_owned(), "libsymbols.so".to_owned())]);

        let result = get_symbols(&inputs, directory.path(), &module_name_map, false).unwrap();

        assert_eq!(result.symbols, inputs);
        assert!(result.missing_modules.is_empty());
    }

    #[test]
    fn rejects_invalid_module_name_mapping() {
        let module_name_map = HashMap::from([("libtrace.so".to_owned(), "symbols.elf".to_owned())]);

        assert!(normalize_module_name_map(&module_name_map).is_err());
    }

    #[test]
    fn indexes_file_symlinks_without_following_directory_symlinks() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("symbols.elf");
        let link = directory.path().join("libsymbols.so");
        std::fs::write(&target, []).unwrap();
        create_file_symlink(&target, &link).unwrap();

        let files = index_symbol_files(directory.path()).unwrap();

        assert_eq!(files, vec![link]);
    }

    #[cfg(unix)]
    fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }
}
