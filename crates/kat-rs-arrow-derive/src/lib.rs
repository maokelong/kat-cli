//! Derives Arrow row writers for prost-generated protobuf message structs.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Meta, parse_macro_input};

#[proc_macro_derive(ArrowRow)]
pub fn derive_arrow_row(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match expand_arrow_row(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_arrow_row(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let row_ident = input.ident;
    let writer_ident = format_ident!("{row_ident}ArrowWriter");
    let fields = match input.data {
        Data::Struct(data) => match data.fields {
            Fields::Named(fields) => fields.named,
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "ArrowRow only supports structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                row_ident,
                "ArrowRow only supports structs",
            ));
        }
    };

    let mut writer_fields = Vec::new();
    let mut writer_inits = Vec::new();
    let mut schema_fields = Vec::new();
    let mut append_values = Vec::new();
    let mut finish_arrays = Vec::new();

    for field in fields {
        let field_ident = field.ident.clone().ok_or_else(|| {
            syn::Error::new_spanned(&field, "ArrowRow only supports named fields")
        })?;
        let column_name = field_ident.to_string();
        let kind = ProstFieldKind::from_field(&field)?;
        let builder_type = kind.builder_type();
        let data_type = kind.data_type();
        let builder_init = kind.builder_init();
        let append_value = kind.append_value(&field_ident);

        writer_fields.push(quote! {
            #field_ident: #builder_type
        });
        writer_inits.push(quote! {
            #field_ident: #builder_init
        });
        schema_fields.push(quote! {
            ::arrow_schema::Field::new(#column_name, #data_type, false)
        });
        append_values.push(append_value);
        finish_arrays.push(quote! {
            ::std::sync::Arc::new(self.#field_ident.finish()) as ::arrow_array::ArrayRef
        });
    }

    Ok(quote! {
        struct #writer_ident {
            #(#writer_fields,)*
        }

        impl #writer_ident {
            fn new(capacity: usize) -> Self {
                Self {
                    #(#writer_inits,)*
                }
            }

            fn append(&mut self, row: &#row_ident) {
                #(#append_values)*
            }

            fn finish(mut self) -> ::anyhow::Result<::arrow_array::RecordBatch> {
                let arrays: ::std::vec::Vec<::arrow_array::ArrayRef> = vec![
                    #(#finish_arrays,)*
                ];

                Ok(::arrow_array::RecordBatch::try_new(
                    #row_ident::arrow_schema(),
                    arrays,
                )?)
            }
        }

        impl #row_ident {
            pub(crate) fn record_batch_from(rows: impl ::std::iter::IntoIterator<Item = Self>) -> ::anyhow::Result<::arrow_array::RecordBatch> {
                let rows = rows.into_iter();
                let capacity = rows.size_hint().0;
                let mut writer = #writer_ident::new(capacity);

                for row in rows {
                    writer.append(&row);
                }

                writer.finish()
            }

            fn arrow_schema() -> ::std::sync::Arc<::arrow_schema::Schema> {
                ::std::sync::Arc::new(::arrow_schema::Schema::new(vec![
                    #(#schema_fields,)*
                ]))
            }
        }
    })
}

#[derive(Clone, Copy)]
enum ProstFieldKind {
    Binary,
    Bool,
    F32,
    F64,
    I32,
    I64,
    String,
    U32,
    U64,
}

impl ProstFieldKind {
    fn from_field(field: &syn::Field) -> syn::Result<Self> {
        let prost_attr = field
            .attrs
            .iter()
            .find(|attr| attr.path().is_ident("prost"))
            .ok_or_else(|| syn::Error::new_spanned(field, "missing #[prost(...)] attribute"))?;

        let Meta::List(meta) = &prost_attr.meta else {
            return Err(syn::Error::new_spanned(
                prost_attr,
                "expected #[prost(...)] attribute list",
            ));
        };
        let tokens = meta.tokens.to_string();

        if has_prost_kind(&tokens, "repeated") || has_prost_kind(&tokens, "map") {
            return Err(syn::Error::new_spanned(
                field,
                "ArrowRow does not support repeated or map protobuf fields yet",
            ));
        }

        if has_prost_kind(&tokens, "bytes") {
            Ok(Self::Binary)
        } else if has_prost_kind(&tokens, "bool") {
            Ok(Self::Bool)
        } else if has_prost_kind(&tokens, "double") {
            Ok(Self::F64)
        } else if has_prost_kind(&tokens, "float") {
            Ok(Self::F32)
        } else if has_prost_kind(&tokens, "int32")
            || has_prost_kind(&tokens, "sint32")
            || has_prost_kind(&tokens, "sfixed32")
            || has_prost_kind(&tokens, "enumeration")
        {
            Ok(Self::I32)
        } else if has_prost_kind(&tokens, "int64")
            || has_prost_kind(&tokens, "sint64")
            || has_prost_kind(&tokens, "sfixed64")
        {
            Ok(Self::I64)
        } else if has_prost_kind(&tokens, "string") {
            Ok(Self::String)
        } else if has_prost_kind(&tokens, "uint32") || has_prost_kind(&tokens, "fixed32") {
            Ok(Self::U32)
        } else if has_prost_kind(&tokens, "uint64") || has_prost_kind(&tokens, "fixed64") {
            Ok(Self::U64)
        } else {
            Err(syn::Error::new_spanned(
                field,
                format!("unsupported prost field attribute: {tokens}"),
            ))
        }
    }

    fn builder_type(self) -> proc_macro2::TokenStream {
        match self {
            Self::Binary => quote!(::arrow_array::builder::BinaryBuilder),
            Self::Bool => quote!(::arrow_array::builder::BooleanBuilder),
            Self::F32 => quote!(::arrow_array::builder::Float32Builder),
            Self::F64 => quote!(::arrow_array::builder::Float64Builder),
            Self::I32 => quote!(::arrow_array::builder::Int32Builder),
            Self::I64 => quote!(::arrow_array::builder::Int64Builder),
            Self::String => quote!(::arrow_array::builder::StringBuilder),
            Self::U32 => quote!(::arrow_array::builder::UInt32Builder),
            Self::U64 => quote!(::arrow_array::builder::UInt64Builder),
        }
    }

    fn data_type(self) -> proc_macro2::TokenStream {
        match self {
            Self::Binary => quote!(::arrow_schema::DataType::Binary),
            Self::Bool => quote!(::arrow_schema::DataType::Boolean),
            Self::F32 => quote!(::arrow_schema::DataType::Float32),
            Self::F64 => quote!(::arrow_schema::DataType::Float64),
            Self::I32 => quote!(::arrow_schema::DataType::Int32),
            Self::I64 => quote!(::arrow_schema::DataType::Int64),
            Self::String => quote!(::arrow_schema::DataType::Utf8),
            Self::U32 => quote!(::arrow_schema::DataType::UInt32),
            Self::U64 => quote!(::arrow_schema::DataType::UInt64),
        }
    }

    fn builder_init(self) -> proc_macro2::TokenStream {
        match self {
            Self::Binary => quote!(::arrow_array::builder::BinaryBuilder::new()),
            Self::String => quote!(::arrow_array::builder::StringBuilder::new()),
            Self::Bool => quote!(::arrow_array::builder::BooleanBuilder::with_capacity(
                capacity
            )),
            Self::F32 => quote!(::arrow_array::builder::Float32Builder::with_capacity(
                capacity
            )),
            Self::F64 => quote!(::arrow_array::builder::Float64Builder::with_capacity(
                capacity
            )),
            Self::I32 => quote!(::arrow_array::builder::Int32Builder::with_capacity(
                capacity
            )),
            Self::I64 => quote!(::arrow_array::builder::Int64Builder::with_capacity(
                capacity
            )),
            Self::U32 => quote!(::arrow_array::builder::UInt32Builder::with_capacity(
                capacity
            )),
            Self::U64 => quote!(::arrow_array::builder::UInt64Builder::with_capacity(
                capacity
            )),
        }
    }

    fn append_value(self, field_ident: &syn::Ident) -> proc_macro2::TokenStream {
        match self {
            Self::Binary => quote! {
                self.#field_ident.append_value(row.#field_ident.as_slice());
            },
            Self::String => quote! {
                self.#field_ident.append_value(&row.#field_ident);
            },
            Self::Bool | Self::F32 | Self::F64 | Self::I32 | Self::I64 | Self::U32 | Self::U64 => {
                quote! {
                    self.#field_ident.append_value(row.#field_ident);
                }
            }
        }
    }
}

fn has_prost_kind(tokens: &str, kind: &str) -> bool {
    tokens
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .any(|part| part == kind)
}
