mod provider;

use std::{path::PathBuf, sync::Arc};

use datafusion_catalog::SchemaProvider;
use datafusion_ffi::schema_provider::FFI_SchemaProvider;
use datafusion_python_util::ffi_logical_codec_from_pycapsule;
use pyo3::exceptions::PyRuntimeError;
use pyo3::types::{PyAny, PyCapsule, PyModule, PyModuleMethods};
use pyo3::{Bound, PyResult, Python, pyclass, pymethods, pymodule};

use crate::provider::HitraceSchema;

#[pyclass(
    frozen,
    skip_from_py_object,
    name = "HitraceSchemaProvider",
    module = "_kat_hitrace"
)]
#[derive(Debug)]
pub struct HitraceSchemaProvider {
    inner: Arc<HitraceSchema>,
}

#[pymethods]
impl HitraceSchemaProvider {
    #[new]
    pub fn new(trace: PathBuf) -> PyResult<Self> {
        HitraceSchema::open(&trace)
            .map(|provider| Self {
                inner: Arc::new(provider),
            })
            .map_err(|error| PyRuntimeError::new_err(format!("{error:#}")))
    }

    pub fn __datafusion_schema_provider__<'py>(
        &self,
        py: Python<'py>,
        session: Bound<PyAny>,
    ) -> PyResult<Bound<'py, PyCapsule>> {
        let name = cr"datafusion_schema_provider".into();
        let provider = Arc::clone(&self.inner) as Arc<dyn SchemaProvider + Send>;
        let codec = ffi_logical_codec_from_pycapsule(session)?;
        let provider = FFI_SchemaProvider::new_with_ffi_codec(provider, None, codec);
        PyCapsule::new(py, provider, Some(name))
    }
}

#[pymodule]
fn _kat_hitrace(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<HitraceSchemaProvider>()?;
    Ok(())
}
