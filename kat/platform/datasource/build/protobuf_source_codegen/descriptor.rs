use std::collections::{BTreeMap, BTreeSet};

use prost_types::{DescriptorProto, EnumDescriptorProto, FieldDescriptorProto, FileDescriptorSet};

#[derive(Clone, Debug)]
pub(super) struct MessageDef {
    pub(super) fqn: String,
    pub(super) package: String,
    pub(super) nesting: Vec<String>,
    pub(super) syntax: Syntax,
    pub(super) descriptor: DescriptorProto,
}

#[derive(Clone, Debug)]
pub(super) struct EnumDef {
    pub(super) fqn: String,
    pub(super) descriptor: EnumDescriptorProto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Syntax {
    Proto2,
    Proto3,
    Other,
}

#[derive(Debug)]
pub(super) struct Catalog {
    messages: BTreeMap<String, MessageDef>,
    enums: BTreeMap<String, EnumDef>,
    duplicate_messages: BTreeSet<String>,
    duplicate_enums: BTreeSet<String>,
    extensions_by_extendee: BTreeMap<String, Vec<FieldDescriptorProto>>,
}

impl Catalog {
    pub(super) fn new(descriptors: &FileDescriptorSet) -> Self {
        let mut catalog = Self {
            messages: BTreeMap::new(),
            enums: BTreeMap::new(),
            duplicate_messages: BTreeSet::new(),
            duplicate_enums: BTreeSet::new(),
            extensions_by_extendee: BTreeMap::new(),
        };

        for file in &descriptors.file {
            let package = file.package.clone().unwrap_or_default();
            let syntax = match file.syntax.as_deref().unwrap_or("proto2") {
                "proto2" => Syntax::Proto2,
                "proto3" => Syntax::Proto3,
                _ => Syntax::Other,
            };
            for message in &file.message_type {
                catalog.index_message(&package, syntax, &[], message);
            }
            for enumeration in &file.enum_type {
                catalog.index_enum(&package, &[], enumeration);
            }
            for extension in &file.extension {
                if let Some(extendee) = extension.extendee.as_deref() {
                    catalog
                        .extensions_by_extendee
                        .entry(canonical(extendee).to_string())
                        .or_default()
                        .push(extension.clone());
                }
            }
        }
        catalog
    }

    pub(super) fn message(&self, fqn: &str) -> Option<&MessageDef> {
        (!self.duplicate_messages.contains(fqn))
            .then(|| self.messages.get(fqn))
            .flatten()
    }

    pub(super) fn enum_def(&self, fqn: &str) -> Option<&EnumDef> {
        (!self.duplicate_enums.contains(fqn))
            .then(|| self.enums.get(fqn))
            .flatten()
    }

    pub(super) fn message_is_ambiguous(&self, fqn: &str) -> bool {
        self.duplicate_messages.contains(fqn)
    }

    pub(super) fn enum_is_ambiguous(&self, fqn: &str) -> bool {
        self.duplicate_enums.contains(fqn)
    }

    pub(super) fn extensions_for(&self, message_fqn: &str) -> &[FieldDescriptorProto] {
        self.extensions_by_extendee
            .get(message_fqn)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(super) fn resolve_message<'a>(
        &'a self,
        containing_message_fqn: &str,
        type_name: &str,
    ) -> Option<&'a MessageDef> {
        self.resolve_name(containing_message_fqn, type_name, |candidate| {
            self.message(candidate)
        })
    }

    pub(super) fn resolve_enum<'a>(
        &'a self,
        containing_message_fqn: &str,
        type_name: &str,
    ) -> Option<&'a EnumDef> {
        self.resolve_name(containing_message_fqn, type_name, |candidate| {
            self.enum_def(candidate)
        })
    }

    pub(super) fn canonical_reference(
        &self,
        containing_message_fqn: &str,
        type_name: &str,
    ) -> String {
        if type_name.starts_with('.') {
            return canonical(type_name).to_string();
        }
        let mut scope = containing_message_fqn.to_string();
        loop {
            let candidate = format!("{scope}.{type_name}");
            if self.messages.contains_key(&candidate) || self.enums.contains_key(&candidate) {
                return candidate;
            }
            let Some((parent, _)) = scope.rsplit_once('.') else {
                break;
            };
            scope.truncate(parent.len());
        }
        canonical(type_name).to_string()
    }

    fn resolve_name<'a, T>(
        &'a self,
        containing_message_fqn: &str,
        type_name: &str,
        lookup: impl Fn(&str) -> Option<&'a T>,
    ) -> Option<&'a T> {
        if type_name.starts_with('.') {
            return lookup(canonical(type_name));
        }
        let mut scope = containing_message_fqn.to_string();
        loop {
            let candidate = format!("{scope}.{type_name}");
            if let Some(value) = lookup(&candidate) {
                return Some(value);
            }
            let Some((parent, _)) = scope.rsplit_once('.') else {
                break;
            };
            scope.truncate(parent.len());
        }
        lookup(canonical(type_name))
    }

    fn index_message(
        &mut self,
        package: &str,
        syntax: Syntax,
        parents: &[String],
        descriptor: &DescriptorProto,
    ) {
        let Some(name) = descriptor.name.as_deref() else {
            return;
        };
        let mut nesting = parents.to_vec();
        nesting.push(name.to_string());
        let fqn = qualify(package, &nesting);
        let definition = MessageDef {
            fqn: fqn.clone(),
            package: package.to_string(),
            nesting: nesting.clone(),
            syntax,
            descriptor: descriptor.clone(),
        };
        if self.messages.insert(fqn.clone(), definition).is_some() {
            self.duplicate_messages.insert(fqn.clone());
        }

        for nested in &descriptor.nested_type {
            self.index_message(package, syntax, &nesting, nested);
        }
        for enumeration in &descriptor.enum_type {
            self.index_enum(package, &nesting, enumeration);
        }
        for extension in &descriptor.extension {
            if let Some(extendee) = extension.extendee.as_deref() {
                self.extensions_by_extendee
                    .entry(canonical(extendee).to_string())
                    .or_default()
                    .push(extension.clone());
            }
        }
    }

    fn index_enum(&mut self, package: &str, parents: &[String], descriptor: &EnumDescriptorProto) {
        let Some(name) = descriptor.name.as_deref() else {
            return;
        };
        let mut nesting = parents.to_vec();
        nesting.push(name.to_string());
        let fqn = qualify(package, &nesting);
        let definition = EnumDef {
            fqn: fqn.clone(),
            descriptor: descriptor.clone(),
        };
        if self.enums.insert(fqn.clone(), definition).is_some() {
            self.duplicate_enums.insert(fqn);
        }
    }
}

pub(super) fn canonical(type_name: &str) -> &str {
    type_name.strip_prefix('.').unwrap_or(type_name)
}

fn qualify(package: &str, nesting: &[String]) -> String {
    if package.is_empty() {
        nesting.join(".")
    } else {
        format!("{package}.{}", nesting.join("."))
    }
}
