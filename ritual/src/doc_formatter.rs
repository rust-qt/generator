//! Generates markdown code for documentation comments
//! of the output Rust crate.

#![allow(dead_code)]

use crate::cpp_ffi_data::{CppFfiFunctionKind, CppFieldAccessorType};
use crate::cpp_type::CppType;
use crate::database::{DatabaseClient, DbItem, DocItem};
use crate::rust_code_generator::rust_type_to_code;
use crate::rust_info::{
    RustEnumValue, RustFunction, RustFunctionKind, RustModule, RustModuleKind, RustQtReceiverType,
    RustSpecialModuleKind, RustStruct, RustStructKind, RustWrapperTypeKind,
};
use itertools::Itertools;
use ritual_common::errors::{bail, err_msg, Result};
use std::fmt::Write;

pub fn wrap_inline_cpp_code(code: &str) -> String {
    format!("<span style='color: green;'>```{}```</span>", code)
}

pub fn wrap_cpp_doc_block(html: &str) -> String {
    format!(
        "<div style='border: 1px solid #5CFF95; \
         background: #D6FFE4; padding: 16px;'>{}</div>",
        html
    )
}

pub fn module_doc(module: DbItem<&RustModule>, database: &DatabaseClient) -> Result<String> {
    let mut output = String::new();
    match module.item.kind {
        RustModuleKind::Special(kind) => match kind {
            RustSpecialModuleKind::CrateRoot => {
                // TODO: generate some useful docs for crate root
                write!(output, "Crate root")?;
            }
            RustSpecialModuleKind::Ffi => {
                write!(output, "Functions provided by the C++ wrapper library")?;
            }
            RustSpecialModuleKind::Ops => {
                write!(output, "Functions that provide access to C++ operators")?;
            }
            RustSpecialModuleKind::SizedTypes => {
                write!(
                    output,
                    "Types with the same size and alignment as corresponding C++ types"
                )?;
            }
        },
        RustModuleKind::CppNamespace { .. } => {
            let cpp_item = database
                .source_cpp_item(&module.id)?
                .ok_or_else(|| err_msg("source cpp item not found"))?
                .item
                .as_namespace_ref()
                .ok_or_else(|| err_msg("invalid source cpp item type"))?;

            let cpp_path_text = wrap_inline_cpp_code(&cpp_item.path.to_cpp_pseudo_code());
            write!(output, "C++ namespace: {}", cpp_path_text)?;
        }
        RustModuleKind::CppNestedTypes { .. } => {
            let cpp_item = database
                .source_cpp_item(&module.id)?
                .ok_or_else(|| err_msg("source cpp item not found"))?
                .item
                .as_type_ref()
                .ok_or_else(|| err_msg("invalid source cpp item type"))?;

            let cpp_path_text = wrap_inline_cpp_code(&cpp_item.path.to_cpp_pseudo_code());
            write!(output, "C++ type: {}", cpp_path_text)?;
        }
    };
    Ok(output)
}

fn first_phrase(html: &str) -> &str {
    if let Some(index) = html.find("</p>") {
        return &html[..index + "</p>".len()];
    }
    if let Some(index) = html.find('.') {
        return &html[..=index];
    }
    html
}

pub fn struct_doc(type1: DbItem<&RustStruct>, database: &DatabaseClient) -> Result<String> {
    let mut output = String::new();

    let doc_item = database.find_doc_for(&type1.id)?;
    if let Some(doc_item) = &doc_item {
        if !doc_item.item.html.is_empty() {
            writeln!(output, "{}\n", first_phrase(&doc_item.item.html))?;
        }
    }

    match &type1.item.kind {
        RustStructKind::WrapperType(kind) => {
            let cpp_item = database
                .source_cpp_item(&type1.id)?
                .ok_or_else(|| err_msg("source cpp item not found"))?;

            let cpp_type_code = cpp_item
                .item
                .path()
                .ok_or_else(|| err_msg("cpp item expected to have path"))?
                .to_cpp_pseudo_code();

            match kind {
                RustWrapperTypeKind::EnumWrapper => {
                    writeln!(
                        output,
                        "C++ enum: {}.\n",
                        wrap_inline_cpp_code(&cpp_type_code)
                    )?;
                }
                RustWrapperTypeKind::ImmovableClassWrapper => {
                    writeln!(
                        output,
                        "C++ class: {}.\n",
                        wrap_inline_cpp_code(&cpp_type_code)
                    )?;
                }
                RustWrapperTypeKind::MovableClassWrapper { .. } => {
                    // not supported now
                }
            }

            if let Some(raw_slot_wrapper) = &type1.item.raw_slot_wrapper_data {
                output.clear(); // remove irrelevant C++ type name
                writeln!(
                    output,
                    "Binds a Qt signal with {} to a Rust extern function.\n",
                    if raw_slot_wrapper.arguments.is_empty() {
                        "no arguments".to_string()
                    } else {
                        format!(
                            "arguments `{}`",
                            raw_slot_wrapper
                                .arguments
                                .iter()
                                .map(|arg| rust_type_to_code(arg, Some(database.crate_name())))
                                .join(",")
                        )
                    }
                )?;

                writeln!(
                    output,
                    "It's recommended to use `{}` instead \
                     because it provides a more high-level API.\n",
                    raw_slot_wrapper.closure_wrapper.last()
                )?;

                let ffi_item = database
                    .source_ffi_item(&cpp_item.id)?
                    .ok_or_else(|| err_msg("source ffi item not found"))?
                    .filter_map(|i| i.as_slot_wrapper_ref())
                    .ok_or_else(|| err_msg("invalid source ffi item type"))?;

                if !ffi_item.item.signal_arguments.is_empty() {
                    let cpp_args = ffi_item
                        .item
                        .signal_arguments
                        .iter()
                        .map(CppType::to_cpp_pseudo_code)
                        .join(", ");
                    writeln!(
                        output,
                        "Corresponding C++ argument types: ({}).\n",
                        wrap_inline_cpp_code(&cpp_args)
                    )?;
                }

                writeln!(
                    output,
                    "Create an object using `new()` \
                     and bind your function and payload using `set()`. \
                     The function will receive the payload as its first arguments, \
                     and the rest of arguments will be values passed \
                     through the Qt connection system. \
                     Use `connect()` method of a `qt_core::Signal` object \
                     to connect the signal to this slot. \
                     The callback function will be executed each time the slot is invoked \
                     until source signals are disconnected or the slot object is destroyed.\n\n\
                     If `set()` was not called, slot invocation has no effect.\n"
                )?;
            }
        }
        RustStructKind::QtSlotWrapper(wrapper) => {
            let cpp_item = database
                .source_cpp_item(&type1.id)?
                .ok_or_else(|| err_msg("source cpp item not found"))?;

            let ffi_item = database
                .source_ffi_item(&cpp_item.id)?
                .ok_or_else(|| err_msg("source ffi item not found"))?
                .item
                .as_slot_wrapper_ref()
                .ok_or_else(|| err_msg("invalid source ffi item type"))?;

            writeln!(
                output,
                "Binds a Qt signal with {} to a Rust closure.\n",
                if wrapper.arguments.is_empty() {
                    "no arguments".to_string()
                } else {
                    format!(
                        "arguments `{}`",
                        wrapper
                            .arguments
                            .iter()
                            .map(|arg| rust_type_to_code(
                                arg.api_type(),
                                Some(database.crate_name())
                            ))
                            .join(",")
                    )
                }
            )?;

            if !ffi_item.signal_arguments.is_empty() {
                let cpp_args = ffi_item
                    .signal_arguments
                    .iter()
                    .map(CppType::to_cpp_pseudo_code)
                    .join(", ");
                writeln!(
                    output,
                    "Corresponding C++ argument types: ({}).\n",
                    wrap_inline_cpp_code(&cpp_args)
                )?;
            }

            writeln!(
                output,
                "Create an object using `new()` \
                 and bind your closure using `set()`. \
                 The closure will be called with the signal's arguments \
                 when the slot is invoked. \
                 Use `connect()` method of a `qt_core::Signal` object to connect \
                 the signal to this slot. The closure will be executed each time \
                 the slot is invoked until source signals are disconnected \
                 or the slot object is destroyed. \n\n\
                 The slot object takes ownership of the passed closure. \
                 If `set()` is called again, \
                 previously set closure is dropped. \
                 Make sure that the slot object does not outlive \
                 objects referenced by the closure. \n\n\
                 If `set()` was not called, slot invocation has no effect.\n"
            )?;
        }
        // private struct, no doc needed
        RustStructKind::SizedType(_) => {}
    };

    if let Some(doc_item) = doc_item {
        write!(output, "{}", format_doc_item(doc_item.item))?;
    }
    Ok(output)
}

pub fn enum_value_doc(value: DbItem<&RustEnumValue>, database: &DatabaseClient) -> Result<String> {
    let cpp_item = database
        .source_cpp_item(&value.id)?
        .ok_or_else(|| err_msg("source cpp item not found"))?
        .item
        .as_enum_value_ref()
        .ok_or_else(|| err_msg("invalid source cpp item type"))?;

    let mut doc = format!(
        "C++ enum variant: {}",
        wrap_inline_cpp_code(&format!(
            "{} = {}",
            cpp_item.path.last().name,
            value.item.value
        ))
    );
    if let Some(doc_item) = database.find_doc_for(&value.id)? {
        doc = format!("{} ({})", doc_item.item.html, doc);
    }
    Ok(doc)
}

fn format_maybe_link(url: &Option<String>, text: &str) -> String {
    if let Some(url) = url {
        format!("<a href=\"{}\">{}</a>", url, text)
    } else {
        text.into()
    }
}

fn format_doc_item(cpp_doc: &DocItem) -> String {
    let mut output = if let Some(declaration) = &cpp_doc.mismatched_declaration {
        format!(
            "Warning: no exact match found in C++ documentation. \
             Below is the {} for {}:",
            format_maybe_link(&cpp_doc.url, "C++ documentation"),
            wrap_inline_cpp_code(declaration)
        )
    } else {
        format!("{}:", format_maybe_link(&cpp_doc.url, "C++ documentation"))
    };
    write!(output, "{}", wrap_cpp_doc_block(&cpp_doc.html)).unwrap();
    output
}

pub fn function_doc(function: DbItem<&RustFunction>, database: &DatabaseClient) -> Result<String> {
    let cpp_item = database
        .source_cpp_item(&function.id)?
        .ok_or_else(|| err_msg("source cpp item not found"))?;

    let is_trait_impl = database
        .item(&function.id)?
        .item
        .as_rust_item()
        .ok_or_else(|| err_msg("invalid item type"))?
        .is_trait_impl();

    let has_source_slot_wrapper = database
        .source_ffi_item(&cpp_item.id)?
        .map_or(false, |item| item.item.is_slot_wrapper());

    let mut output = String::new();

    if has_source_slot_wrapper
        && function.item.kind != RustFunctionKind::FfiFunction
        && !is_trait_impl
    {
        match function.item.path.last() {
            "custom_slot" => {
                writeln!(
                    output,
                    "Calls the slot directly, invoking the assigned handler (if any).\n"
                )?;
            }
            "new" => {
                writeln!(output, "Creates a new object.\n")?;
            }
            "set" => {
                writeln!(output, "Assigns `func` as the signal handler.\n")?;
                writeln!(
                    output,
                    "`func` will be called each time a connected signal is emitted. \
                     Any previously assigned function will be deregistered. \
                     Passing `None` will deregister the handler without setting a new one.\n"
                )?;
            }
            "meta_object" | "qt_metacall" | "qt_metacast" | "static_meta_object" | "tr"
            | "tr_utf8" => {
                // TODO: document or blacklist these methods for all Qt classes
            }
            other => bail!("unknown slot wrapper method: {}", other),
        }
        return Ok(output);
    }

    let doc_item = database.find_doc_for(&function.id)?;
    if let Some(doc_item) = &doc_item {
        if !doc_item.item.html.is_empty() {
            writeln!(output, "{}\n", first_phrase(&doc_item.item.html))?;
        }
    }

    match &function.item.kind {
        RustFunctionKind::FfiWrapper(_) => {
            let cpp_ffi_function = database
                .source_ffi_item(&function.id)?
                .ok_or_else(|| err_msg("source cpp item not found"))?
                .item
                .as_function_ref()
                .ok_or_else(|| err_msg("invalid source ffi item type"))?;

            match &cpp_ffi_function.kind {
                CppFfiFunctionKind::Function => {
                    let cpp_item = cpp_item
                        .item
                        .as_function_ref()
                        .ok_or_else(|| err_msg("invalid source cpp item type"))?;
                    write!(
                        output,
                        "Calls C++ function: {}.\n\n",
                        wrap_inline_cpp_code(&cpp_item.short_text())
                    )?;

                    // TODO: detect omitted arguments using source_id
                    /*if let Some(arguments_before_omitting) =
                        &cpp_function.doc.arguments_before_omitting
                    {
                        // TODO: handle singular/plural form
                        doc.push(format!(
                            "This version of the function omits some arguments ({}).\n\n",
                            arguments_before_omitting.len() - cpp_function.arguments.len()
                        ));
                    }*/
                }
                CppFfiFunctionKind::FieldAccessor { accessor_type } => {
                    let cpp_item = cpp_item
                        .item
                        .as_field_ref()
                        .ok_or_else(|| err_msg("invalid source cpp item type"))?;
                    let field_text =
                        wrap_inline_cpp_code(&cpp_item.path.last().to_cpp_pseudo_code());
                    match *accessor_type {
                        CppFieldAccessorType::CopyGetter => {
                            write!(output, "Returns the value of the {} field.", field_text)?;
                        }
                        CppFieldAccessorType::ConstRefGetter => {
                            write!(output, "Returns a reference to the {} field.", field_text)?;
                        }
                        CppFieldAccessorType::MutRefGetter => {
                            write!(
                                output,
                                "Returns a mutable reference to the {} field.",
                                field_text
                            )?;
                        }
                        CppFieldAccessorType::Setter => {
                            write!(output, "Sets the value of the {} field.", field_text)?;
                        }
                    };
                }
            }
        }
        RustFunctionKind::SignalOrSlotGetter(getter) => {
            let cpp_item = cpp_item
                .item
                .as_function_ref()
                .ok_or_else(|| err_msg("invalid source cpp item type"))?;

            writeln!(
                output,
                "Returns a built-in Qt {signal} `{cpp_path}` that can be passed to \
                 `qt_core::Signal::connect`.\n",
                signal = match getter.receiver_type {
                    RustQtReceiverType::Signal => "signal",
                    RustQtReceiverType::Slot => "slot",
                },
                cpp_path = cpp_item.path.to_cpp_pseudo_code()
            )?;
        }
        // FFI functions are private
        RustFunctionKind::FfiFunction => {}
    }
    if let Some(doc_item) = database.find_doc_for(&function.id)? {
        write!(output, "{}", format_doc_item(doc_item.item))?;
    }
    Ok(output)
}

// TODO: add docs for slot wrapper functions
/*
    for method in methods {
        if method.name.parts.len() != 1 {
            return Err(unexpected("method name should have one part here").into());
        }
        if method.variant_docs.len() != 1 {
            return Err(
                unexpected("method.variant_docs should have one item here").into()
            );
        }
        match method.name.parts[0].as_str() {
            "new" => {
                method.common_doc = Some("Constructs a new object.".into());
            }
            "set" => {
                method.common_doc = Some(
    "Sets `func` as the callback function \
     and `data` as the payload. When the slot is invoked, `func(data)` will be called. \
     Note that it may happen at any time and in any thread."
      .into(),
  );
            }
            "custom_slot" => {
                method.common_doc = Some(
    "Executes the callback function, as if the slot was invoked \
     with these arguments. Does nothing if no callback function was set."
      .into(),
  );
            }
            _ => {
                return Err(unexpected("unknown method for slot wrapper").into());
            }
        }
    }
*/
