use std::path::PathBuf;

use pyo3::{
    create_exception, exceptions::PyRuntimeError, prelude::*, types::PyModule, wrap_pyfunction,
};

create_exception!(_native, _DecodeError, PyRuntimeError);

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

#[pymodule]
#[pyo3(name = "_native")]
fn datasource_native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("_DecodeError", module.py().get_type::<_DecodeError>())?;
    module.add_function(wrap_pyfunction!(decode, module)?)?;
    Ok(())
}
