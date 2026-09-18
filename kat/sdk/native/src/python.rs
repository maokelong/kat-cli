use std::path::PathBuf;

use pyo3::{
    create_exception, exceptions::PyRuntimeError, prelude::*, types::PyModule, wrap_pyfunction,
};

create_exception!(_native, _DecodeError, PyRuntimeError);
create_exception!(_native, _TextFtraceDecodeError, PyRuntimeError);

#[pyfunction]
fn decode(
    py: Python<'_>,
    source: PathBuf,
    destination: PathBuf,
) -> PyResult<(Vec<String>, Vec<u32>)> {
    let report = py
        .detach(move || crate::decode_hitrace(source, destination))
        .map_err(|error| _DecodeError::new_err(error.to_string()))?;
    Ok((
        report.unsupported_plugins().to_vec(),
        report.unsupported_section_types().to_vec(),
    ))
}

#[pyfunction]
fn decode_text_ftrace(
    py: Python<'_>,
    source: PathBuf,
    destination: PathBuf,
    clock_domain: String,
) -> PyResult<Vec<String>> {
    let report = py
        .detach(move || crate::decode_text_ftrace(&source, &destination, &clock_domain))
        .map_err(|error| _TextFtraceDecodeError::new_err(format!("{error:#}")))?;
    Ok(report.unsupported_event_names().to_vec())
}

#[pymodule]
#[pyo3(name = "_native")]
fn datasource_native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("_DecodeError", module.py().get_type::<_DecodeError>())?;
    module.add(
        "_TextFtraceDecodeError",
        module.py().get_type::<_TextFtraceDecodeError>(),
    )?;
    module.add_function(wrap_pyfunction!(decode, module)?)?;
    module.add_function(wrap_pyfunction!(decode_text_ftrace, module)?)?;
    Ok(())
}
