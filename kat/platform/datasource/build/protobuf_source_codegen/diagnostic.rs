use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Diagnostic {
    root_fqn: String,
    message_fqn: Option<String>,
    field_path: Option<String>,
    detail: String,
}

impl Diagnostic {
    pub(super) fn root(root_fqn: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            root_fqn: root_fqn.into(),
            message_fqn: None,
            field_path: None,
            detail: detail.into(),
        }
    }

    pub(super) fn message(
        root_fqn: impl Into<String>,
        message_fqn: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            root_fqn: root_fqn.into(),
            message_fqn: Some(message_fqn.into()),
            field_path: None,
            detail: detail.into(),
        }
    }

    pub(super) fn field(
        root_fqn: impl Into<String>,
        message_fqn: impl Into<String>,
        field_path: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            root_fqn: root_fqn.into(),
            message_fqn: Some(message_fqn.into()),
            field_path: Some(field_path.into()),
            detail: detail.into(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "protobuf root {:?}", self.root_fqn)?;
        if let Some(message_fqn) = &self.message_fqn {
            write!(formatter, ", message {message_fqn:?}")?;
        }
        if let Some(field_path) = &self.field_path {
            write!(formatter, ", field {field_path:?}")?;
        }
        write!(formatter, ": {}", self.detail)
    }
}
