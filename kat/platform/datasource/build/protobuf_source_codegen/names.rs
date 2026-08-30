const RESERVED_COLUMNS: &[&str] = &["_kat_row_id", "_kat_parent_row_id", "_kat_repeated_index"];

pub(super) fn valid_dataset_name(name: &str) -> bool {
    crate::table_name::valid_table_name(name)
}

pub(super) fn reserved_relationship_column(name: &str) -> bool {
    RESERVED_COLUMNS.contains(&name)
}

pub(super) fn valid_generated_function_name(name: &str) -> bool {
    valid_dataset_name(name) && rust_snake(name) == name
}

pub(super) fn relation_name(root: &str, path: &[String]) -> String {
    if path.is_empty() {
        root.to_string()
    } else {
        format!("{root}_{}", path.join("_"))
    }
}

pub(super) fn rust_snake(name: &str) -> String {
    sanitize_rust_identifier(&name.to_snake_case())
}

pub(super) fn rust_upper_camel(name: &str) -> String {
    sanitize_rust_identifier(&name.to_upper_camel_case())
}

// prost-build 0.14.4 的 ident::sanitize_identifier 是私有函数；这里逐项镜像
// 该固定版本，并由 generated emitter 的真实编译测试防止升级时静默漂移。
fn sanitize_rust_identifier(identifier: &str) -> String {
    match identifier {
        "as" | "break" | "const" | "continue" | "else" | "enum" | "false" | "fn" | "for" | "if"
        | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut" | "pub" | "ref"
        | "return" | "static" | "struct" | "trait" | "true" | "type" | "unsafe" | "use"
        | "where" | "while" | "dyn" | "abstract" | "become" | "box" | "do" | "final" | "macro"
        | "override" | "priv" | "typeof" | "unsized" | "virtual" | "yield" | "async" | "await"
        | "try" | "gen" => format!("r#{identifier}"),
        "_" | "super" | "self" | "Self" | "extern" | "crate" => {
            format!("{identifier}_")
        }
        value if value.starts_with(|character: char| character.is_numeric()) => {
            format!("_{identifier}")
        }
        _ => identifier.to_string(),
    }
}

use heck::{ToSnakeCase, ToUpperCamelCase};
