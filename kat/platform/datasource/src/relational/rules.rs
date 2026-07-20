#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExpansionRule {
    RootScalars,
    RepeatedScalar,
    RepeatedMessage,
    MessageFieldTable,
    OneofVariantTable,
}
