use std::{fs, io};

use kat_datasource::{DatasetWriteTarget, import_hitrace, inspect_dataset};
use prost::Message;
use tempfile::tempdir;

const HEADER_SIZE: usize = 1024;
const HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;

#[derive(Clone, PartialEq, Message)]
struct Envelope {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(bytes = "vec", tag = "3")]
    data: Vec<u8>,
}

#[test]
fn import_hitrace_keeps_public_entry_and_writes_relational_dataset() {
    let root = tempdir().expect("tempdir");
    let trace = root.path().join("input.htrace");
    let dataset = root.path().join("dataset");
    fs::write(&trace, fixture(["z-plugin", "a-plugin"])).expect("fixture is written");

    let mut observed = Vec::new();
    let imported = import_hitrace(
        &trace,
        DatasetWriteTarget::write_to_empty(&dataset),
        |content| {
            observed.push((
                content.kind().to_owned(),
                content.value().to_owned(),
                content.byte_offset(),
            ));
            Ok(())
        },
    )
    .expect("relational hitrace import succeeds");

    assert_eq!(imported.path(), dataset);
    assert_eq!(imported.unsupported_plugins(), ["a-plugin", "z-plugin"]);
    assert!(imported.unsupported_section_types().is_empty());
    assert_eq!(observed.len(), 2);
    assert!(dataset.join("catalog.json").is_file());
    assert!(dataset.join(".kat-dataset").is_file());
    assert!(
        inspect_dataset(&dataset)
            .expect("dataset is inspectable")
            .tables()
            .is_empty()
    );
}

#[test]
fn import_hitrace_preserves_observer_error_variant() {
    let root = tempdir().expect("tempdir");
    let trace = root.path().join("input.htrace");
    fs::write(&trace, fixture(["future-plugin"])).expect("fixture is written");

    let error = import_hitrace(
        &trace,
        DatasetWriteTarget::write_to_empty(root.path().join("dataset")),
        |_| Err(io::Error::other("observer failed")),
    )
    .expect_err("observer failure is returned separately");

    assert!(matches!(
        error,
        kat_datasource::HitraceImportError::ObserveUnsupportedContent { .. }
    ));
}

fn fixture<const N: usize>(plugins: [&str; N]) -> Vec<u8> {
    let envelopes = plugins
        .into_iter()
        .map(|name| {
            Envelope {
                name: name.to_owned(),
                data: Vec::new(),
            }
            .encode_to_vec()
        })
        .collect::<Vec<_>>();
    let body_length = envelopes
        .iter()
        .map(|envelope| 4 + envelope.len())
        .sum::<usize>();
    let mut bytes = vec![0; HEADER_SIZE];
    bytes[0..8].copy_from_slice(&HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((HEADER_SIZE + body_length) as u64).to_le_bytes());

    for envelope in envelopes {
        append_frame(&mut bytes, &envelope);
    }
    bytes
}

fn append_frame(bytes: &mut Vec<u8>, envelope: &[u8]) {
    bytes.extend_from_slice(&(envelope.len() as u32).to_le_bytes());
    bytes.extend_from_slice(envelope);
}
