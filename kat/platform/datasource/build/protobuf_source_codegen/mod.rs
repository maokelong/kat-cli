//! 固定 descriptor 驱动的 protobuf relation compiler。
//!
//! 这个仅供构建期使用的模块暴露 root [`compile`]，以及在生成结果上追加 plan 外
//! 手写 relation 所需枚举定义的窄操作。Descriptor closure、关系计划、prost binding
//! 与 Rust renderer 都留在模块内部，避免 production code 生长出第二套映射规则。

mod diagnostic;
mod names;
mod plan;
mod prost_binding;
mod render;

use std::{collections::BTreeSet, error::Error, fmt};

use prost_reflect::{DescriptorPool, EnumDescriptor};
use prost_types::FileDescriptorSet;

use diagnostic::Diagnostic;

/// 一个 canonical descriptor root 与显式 root relation 的构建期绑定。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RootSpec<'a> {
    pub(crate) protobuf_fqn: &'a str,
    pub(crate) root_relation_name: &'a str,
}

impl<'a> RootSpec<'a> {
    pub(crate) const fn new(protobuf_fqn: &'a str, root_relation_name: &'a str) -> Self {
        Self {
            protobuf_fqn,
            root_relation_name,
        }
    }
}

/// 从完整关系计划生成的 Rust 源码。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GeneratedRust {
    source: String,
}

impl GeneratedRust {
    /// 为 plan 外手写的 relation 输出 descriptor 驱动的完整枚举定义，不把该 relation 伪装成 protobuf root。
    pub(crate) fn with_enum_symbol_accessor(
        mut self,
        descriptors: &FileDescriptorSet,
        enum_fqn: &str,
        accessor_name: &str,
    ) -> Result<Self, CompileError> {
        if !names::valid_generated_function_name(accessor_name) {
            return Err(enum_compile_error(
                enum_fqn,
                format!(
                    "generated enum-symbol accessor name {accessor_name:?} must be a safe lower_snake Rust identifier"
                ),
            ));
        }
        if enum_fqn.starts_with('.') || enum_fqn.is_empty() {
            return Err(enum_compile_error(
                enum_fqn,
                "enum type must be a canonical protobuf FQN without a leading dot",
            ));
        }
        let descriptors =
            DescriptorPool::from_file_descriptor_set(descriptors.clone()).map_err(|error| {
                enum_compile_error(enum_fqn, format!("invalid descriptor set: {error}"))
            })?;
        let enum_def = descriptors.get_enum_by_name(enum_fqn).ok_or_else(|| {
            enum_compile_error(
                enum_fqn,
                "canonical enum FQN does not identify an enum in the descriptor set",
            )
        })?;
        if enum_uses_aliases(&enum_def) {
            return Err(enum_compile_error(enum_fqn, "enum aliases are unsupported"));
        }
        render::render_enum_symbol_accessor(&mut self.source, accessor_name, &enum_def);
        Ok(self)
    }

    pub(crate) fn into_source(self) -> String {
        self.source
    }
}

/// 编译已注册 roots 时发现的全部构建期诊断。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompileError {
    messages: Vec<String>,
}

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, message) in self.messages.iter().enumerate() {
            if index != 0 {
                formatter.write_str("\n")?;
            }
            formatter.write_str(message)?;
        }
        Ok(())
    }
}

impl Error for CompileError {}

/// 为 profiler capture adapter 编译 descriptor-derived roots。
pub(crate) fn compile_for_profiler_capture(
    descriptors: &FileDescriptorSet,
    roots: &[RootSpec<'_>],
) -> Result<GeneratedRust, CompileError> {
    let descriptors =
        DescriptorPool::from_file_descriptor_set(descriptors.clone()).map_err(|error| {
            CompileError {
                messages: vec![format!("invalid protobuf descriptor set: {error}")],
            }
        })?;
    let relational_plan = plan::build(&descriptors, roots).map_err(compile_error)?;
    let bindings = prost_binding::bind(&descriptors, &relational_plan).map_err(compile_error)?;
    let source = render::render(&relational_plan, &bindings);
    Ok(GeneratedRust { source })
}

fn compile_error(diagnostics: Vec<Diagnostic>) -> CompileError {
    CompileError {
        messages: diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.to_string())
            .collect(),
    }
}

fn enum_compile_error(enum_fqn: &str, detail: impl fmt::Display) -> CompileError {
    CompileError {
        messages: vec![format!("protobuf enum {enum_fqn:?}: {detail}")],
    }
}

fn enum_uses_aliases(enum_def: &EnumDescriptor) -> bool {
    let alias_enabled = enum_def
        .enum_descriptor_proto()
        .options
        .as_ref()
        .and_then(|options| options.allow_alias)
        .unwrap_or(false);
    let mut numbers = BTreeSet::new();
    alias_enabled
        || enum_def
            .values()
            .any(|value| !numbers.insert(value.number()))
}
