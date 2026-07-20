use std::{collections::HashMap, path::PathBuf, process::ExitCode};

use anyhow::{Context, Result, bail};
use clap::Parser;
use kat_rs_native_hook_symbolize_poc::{MissingModule, get_symbols};
use rusqlite::Connection;
use rust_xlsxwriter::{Workbook, XlsxError};

const DATA_ROWS_PER_SHEET: usize = 1_048_575;

#[derive(Debug, Parser)]
#[command(about = "Symbolize Native Hook frames from a trace_streamer SQLite database")]
struct Cli {
    #[arg(value_name = "TRACE_DB")]
    trace_db: PathBuf,
    #[arg(long, value_name = "DIRECTORY")]
    symbol_dir: PathBuf,
    #[arg(long, value_name = "XLSX")]
    output: PathBuf,
    #[arg(long = "module-map", value_name = "FROM=TO")]
    module_maps: Vec<String>,
    #[arg(long)]
    include_source_location: bool,
}

#[derive(Debug)]
struct Frame {
    callchain_id: i64,
    depth: i64,
    original: String,
}

fn run(cli: Cli) -> Result<()> {
    let frames = read_frames(&cli.trace_db)?;
    if frames.is_empty() {
        eprintln!("warning: native_hook_frame contains no rows");
    }
    let inputs = frames
        .iter()
        .map(|frame| frame.original.clone())
        .collect::<Vec<_>>();
    let module_name_map = parse_module_name_map(&cli.module_maps)?;
    let result = get_symbols(
        &inputs,
        &cli.symbol_dir,
        &module_name_map,
        cli.include_source_location,
    )?;
    write_workbook(
        &cli.output,
        &frames,
        &result.symbols,
        &result.missing_modules,
    )
    .with_context(|| format!("cannot write {}", cli.output.display()))?;
    Ok(())
}

fn parse_module_name_map(values: &[String]) -> Result<HashMap<String, String>> {
    let mut mappings = HashMap::with_capacity(values.len());
    for value in values {
        let (source, target) = value
            .split_once('=')
            .with_context(|| format!("expected module mapping FROM=TO: {value}"))?;
        let source = source.trim();
        let target = target.trim();
        if source.is_empty() || target.is_empty() {
            bail!("expected non-empty module mapping FROM=TO: {value}");
        }
        if mappings
            .insert(source.to_owned(), target.to_owned())
            .is_some()
        {
            bail!("duplicate module mapping source: {source}");
        }
    }
    Ok(mappings)
}

fn read_frames(path: &PathBuf) -> Result<Vec<Frame>> {
    let connection = Connection::open(path)
        .with_context(|| format!("cannot open trace database {}", path.display()))?;
    let mut statement = connection.prepare(
        "SELECT f.callchain_id, f.depth, COALESCE(NULLIF(s.data, ''), m.data || '+0x' || lower(printf('%x', f.vaddr)), '') AS original_symbol
         FROM native_hook_frame f
         LEFT JOIN data_dict s ON s.id = f.symbol_id
         LEFT JOIN data_dict m ON m.id = f.file_id
         ORDER BY f.callchain_id, f.depth, f.id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(Frame {
            callchain_id: row.get(0)?,
            depth: row.get(1)?,
            original: row.get(2)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn write_workbook(
    path: &PathBuf,
    frames: &[Frame],
    resolved: &[String],
    missing_modules: &[MissingModule],
) -> Result<(), XlsxError> {
    let mut workbook = Workbook::new();
    let sheet_count = frames.len().max(1).div_ceil(DATA_ROWS_PER_SHEET);
    for sheet_index in 0..sheet_count {
        let name = symbol_sheet_name(sheet_count, sheet_index);
        let worksheet = workbook.add_worksheet().set_name(name)?;
        worksheet.set_freeze_panes(1, 0)?;
        for (column, header) in [
            "callchain_id",
            "depth",
            "original_symbol",
            "resolved_symbol",
        ]
        .iter()
        .enumerate()
        {
            worksheet.write_string(0, column as u16, *header)?;
        }
        let start = sheet_index * DATA_ROWS_PER_SHEET;
        let end = frames.len().min(start + DATA_ROWS_PER_SHEET);
        for (index, (frame, symbol)) in frames[start..end]
            .iter()
            .zip(&resolved[start..end])
            .enumerate()
        {
            let row = (index + 1) as u32;
            worksheet.write_number(row, 0, frame.callchain_id as f64)?;
            worksheet.write_number(row, 1, frame.depth as f64)?;
            worksheet.write_string(row, 2, &frame.original)?;
            worksheet.write_string(row, 3, symbol)?;
        }
        worksheet.autofilter(0, 0, (end - start) as u32, 3)?;
    }
    let worksheet = workbook.add_worksheet().set_name("missing_modules")?;
    worksheet.set_freeze_panes(1, 0)?;
    worksheet.set_column_width(0, 60)?;
    worksheet.set_column_width(1, 18)?;
    worksheet.write_string(0, 0, "module_path")?;
    worksheet.write_string(0, 1, "occurrence_count")?;
    for (index, module) in missing_modules.iter().enumerate() {
        let row = (index + 1) as u32;
        worksheet.write_string(row, 0, &module.module_path)?;
        worksheet.write_number(row, 1, module.occurrence_count as f64)?;
    }
    worksheet.autofilter(0, 0, missing_modules.len() as u32, 1)?;
    workbook.save(path)
}

fn symbol_sheet_name(sheet_count: usize, sheet_index: usize) -> String {
    if sheet_count == 1 {
        "symbols".to_owned()
    } else {
        format!("symbols_{}", sheet_index + 1)
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_module_name_map, symbol_sheet_name};

    #[test]
    fn parses_repeated_module_map_arguments() {
        let values = vec![
            "libtrace.so=libsymbols.so".to_owned(),
            "/system/lib/libfoo.so=/symbols/libbar.so".to_owned(),
        ];

        let mappings = parse_module_name_map(&values).unwrap();

        assert_eq!(mappings["libtrace.so"], "libsymbols.so");
        assert_eq!(mappings["/system/lib/libfoo.so"], "/symbols/libbar.so");
    }

    #[test]
    fn rejects_duplicate_module_map_sources() {
        let values = vec![
            "libtrace.so=libsymbols.so".to_owned(),
            "libtrace.so=libother.so".to_owned(),
        ];

        assert!(parse_module_name_map(&values).is_err());
    }

    #[test]
    fn names_single_and_paginated_symbol_sheets() {
        assert_eq!(symbol_sheet_name(1, 0), "symbols");
        assert_eq!(symbol_sheet_name(3, 0), "symbols_1");
        assert_eq!(symbol_sheet_name(3, 2), "symbols_3");
    }
}
