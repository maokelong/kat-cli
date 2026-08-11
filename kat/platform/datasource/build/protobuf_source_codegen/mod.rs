//! 固定 descriptor 驱动的 protobuf Source-table compiler。
//!
//! 这个仅供构建期使用的模块有意只暴露 [`compile`] 一个操作。Descriptor
//! closure、关系计划、prost binding 与 Rust renderer 都留在模块内部，避免
//! production code 生长出第二套映射规则。

mod descriptor;
mod diagnostic;
mod names;
mod plan;
mod prost_binding;
mod render;

use std::{error::Error, fmt};

use prost_types::FileDescriptorSet;

use diagnostic::Diagnostic;

/// 一个 canonical descriptor root 与显式 Dataset root table 的构建期绑定。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RootSpec<'a> {
    pub(crate) protobuf_fqn: &'a str,
    pub(crate) root_table_name: &'a str,
}

impl<'a> RootSpec<'a> {
    pub(crate) const fn new(protobuf_fqn: &'a str, root_table_name: &'a str) -> Self {
        Self {
            protobuf_fqn,
            root_table_name,
        }
    }
}

/// 从完整关系计划生成的 Rust 源码。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GeneratedRust {
    source: String,
}

impl GeneratedRust {
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

/// 把固定 descriptors 编译成 schemas 与 direct typed emitters。
///
/// `build.rs` 与 build-contract tests 只共享这一个入口。
pub(crate) fn compile(
    descriptors: &FileDescriptorSet,
    roots: &[RootSpec<'_>],
) -> Result<GeneratedRust, CompileError> {
    let catalog = descriptor::Catalog::new(descriptors);
    let relational_plan = plan::build(&catalog, roots).map_err(compile_error)?;
    let bindings = prost_binding::bind(&catalog, &relational_plan).map_err(compile_error)?;
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
