#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExpansionRule {
    RootRecord,
    RepeatedScalar,
    RepeatedMessage,
    OneofVariant,
}
