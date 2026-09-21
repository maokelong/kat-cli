use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use thiserror::Error;

use crate::{
    pack_discovery::{self, DiscoveredPacks, PackDiscoveryError, PackDiscoveryPaths},
    workflow_runtime::{self, RuntimeInfrastructureError},
};

const LOCATE_SDK: &str = r#"
import importlib.util
import json
from pathlib import Path

spec = importlib.util.find_spec("kat_sdk")
if spec is None:
    print("null")
    raise SystemExit(0)
if not spec.submodule_search_locations:
    raise RuntimeError("KAT SDK must be an installed package directory")
locations = list(spec.submodule_search_locations)
if len(locations) != 1:
    raise RuntimeError("KAT SDK must have one installed package directory")
print(json.dumps(str(Path(locations[0]).resolve(strict=True))))
"#;

pub(crate) fn discover(
    mut paths: PackDiscoveryPaths,
) -> Result<DiscoveredPacks, PackDiscoveryError> {
    let sdk = installed_pack().map_err(|source| PackDiscoveryError::Sdk {
        source: Box::new(source),
    })?;
    if let Some(sdk) = sdk {
        paths.additional_pack_directories.push(sdk);
    }
    pack_discovery::discover(paths)
}

fn installed_pack() -> Result<Option<PathBuf>, SdkError> {
    let python = workflow_runtime::bundled_python_path()?;
    let output = match Command::new(&python)
        .args(["-I", "-B", "-X", "utf8", "-c", LOCATE_SDK])
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) => output,
        // 尚未装配 Python Host 时，原有纯 manifest 发现仍可使用。
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(SdkError::Locate { python, source }),
    };
    if !output.status.success() {
        return Err(SdkError::HostFailed {
            details: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let path: Option<String> =
        serde_json::from_slice(&output.stdout).map_err(SdkError::InvalidResponse)?;
    let Some(path) = path else { return Ok(None) };
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(SdkError::RelativePath);
    }
    Ok(Some(path))
}

#[derive(Debug, Error)]
pub(crate) enum SdkError {
    #[error(transparent)]
    Host(#[from] RuntimeInfrastructureError),
    #[error("failed to locate KAT SDK with {python}")]
    Locate {
        python: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("KAT SDK installation is unavailable: {details}")]
    HostFailed { details: String },
    #[error("invalid KAT SDK location response")]
    InvalidResponse(#[source] serde_json::Error),
    #[error("KAT SDK location must be an absolute directory")]
    RelativePath,
}
