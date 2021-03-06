//! Types for handling information about C++ methods.

use crate::cpp_data::{CppPath, CppPathItem, CppVisibility};
use crate::cpp_ffi_data::CppCast;
pub use crate::cpp_operator::{CppOperator, CppOperatorInfo};
use crate::cpp_type::{CppPointerLikeTypeKind, CppType};
use crate::rust_info::RustQtReceiverType;
use itertools::Itertools;
use ritual_common::errors::{bail, err_msg, Result, ResultExt};
use ritual_common::utils::MapIfOk;
use serde_derive::{Deserialize, Serialize};
use std::fmt::Write;

/// Information about an argument of a C++ method
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub struct CppFunctionArgument {
    /// Identifier. If the argument doesn't have a name
    /// (which is allowed in C++), this field contains
    /// generated name "argX" (X is position of the argument).
    pub name: String,
    /// Argument type
    pub argument_type: CppType,
    /// Flag indicating that the argument has default value and
    /// therefore can be omitted when calling the method
    pub has_default_value: bool,
}

impl CppFunctionArgument {
    /// Generates C++ code for the argument declaration
    pub fn to_cpp_code(&self) -> Result<String> {
        if let CppType::FunctionPointer(..) = self.argument_type {
            Ok(self.argument_type.to_cpp_code(Some(&self.name))?)
        } else {
            Ok(format!(
                "{} {}",
                self.argument_type.to_cpp_code(None)?,
                self.name
            ))
        }
    }
}

/// Enumerator indicating special cases of C++ methods.
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub enum CppFunctionKind {
    /// Just a class method
    Regular,
    /// Constructor
    Constructor,
    /// Destructor
    Destructor,
}

/// Information about a C++ class member method
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub struct CppFunctionMemberData {
    /// Whether this method is a constructor, a destructor or an operator
    pub kind: CppFunctionKind,
    /// True if this is a virtual method
    pub is_virtual: bool,
    /// True if this is a pure virtual method (requires is_virtual = true)
    pub is_pure_virtual: bool,
    /// True if this is a const method, i.e. "this" pointer receives by
    /// this method has const type
    pub is_const: bool,
    /// True if this is a static method, i.e. it doesn't receive "this" pointer at all.
    pub is_static: bool,
    /// Method visibility
    pub visibility: CppVisibility,
    /// True if the method is a Qt signal
    pub is_signal: bool,
    /// True if the method is a Qt slot
    pub is_slot: bool,
}

impl CppFunctionMemberData {
    fn is_same(&self, other: &CppFunctionMemberData) -> bool {
        self.kind == other.kind
            && self.is_const == other.is_const
            && self.is_static == other.is_static
    }
}

/// Information about a C++ method
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub struct CppFunction {
    /// Identifier. For class methods, this field includes
    /// only the method's own name. For free functions,
    /// this field also includes namespaces (if any).
    ///
    /// Last part of the path contains the template parameters of the function itself.
    /// For a template method, this fields contains its template arguments
    /// in the form of `CppTypeBase::TemplateParameter` types.
    /// For an instantiated template method, this field contains the types
    /// used for instantiation.
    ///
    /// For example, `T QObject::findChild<T>()` would have
    /// a `TemplateParameter` type in `template_arguments`
    /// because it's not instantiated, and
    /// `QWidget* QObject::findChild<QWidget*>()` would have `QWidget*` type in
    /// `template_arguments`.
    ///
    /// This field is `None` if this is not a template method.
    /// If the method belongs to a template class,
    /// the class's template arguments are not included here.
    /// Instead, they are available in `member.class_type`.
    pub path: CppPath,
    /// Additional information about a class member function
    /// or None for free functions
    pub member: Option<CppFunctionMemberData>,
    /// If the method is a C++ operator, indicates its kind
    pub operator: Option<CppOperator>,
    /// Return type of the method.
    /// Return type is reported as void for constructors and destructors.
    pub return_type: CppType,
    /// List of the method's arguments
    pub arguments: Vec<CppFunctionArgument>,
    /// Whether the argument list is terminated with "..."
    pub allows_variadic_arguments: bool,
    pub cast: Option<CppCast>,
    /// C++ code of the method's declaration.
    /// None if the method was not explicitly declared.
    pub declaration_code: Option<String>,
}

/// Chosen type allocation place for the method
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash, Serialize, Deserialize)]
pub enum ReturnValueAllocationPlace {
    /// The method returns a class object by value (or is a constructor), and
    /// it's translated to "output" FFI argument and placement new
    Stack,
    /// The method returns a class object by value (or is a constructor), and
    /// it's translated to pointer FFI return type and plain new
    Heap,
    /// The method does not return a class object by value, so
    /// the direct equivalent of the value is used in FFI.
    NotApplicable,
}

impl CppFunctionKind {
    /// Returns true if this method is a constructor
    pub fn is_constructor(&self) -> bool {
        matches!(self, CppFunctionKind::Constructor)
    }

    /// Returns true if this method is a destructor
    pub fn is_destructor(&self) -> bool {
        matches!(self, CppFunctionKind::Destructor)
    }

    /// Returns true if this method is a regular method or a free function
    pub fn is_regular(&self) -> bool {
        matches!(self, CppFunctionKind::Regular)
    }
}

impl CppFunction {
    /// Checks if two methods have exactly the same set of input argument types
    pub fn argument_types_equal(&self, other: &CppFunction) -> bool {
        if self.arguments.len() != other.arguments.len() {
            return false;
        }
        if self.allows_variadic_arguments != other.allows_variadic_arguments {
            return false;
        }
        for (i, j) in self.arguments.iter().zip(other.arguments.iter()) {
            if i.argument_type != j.argument_type {
                return false;
            }
        }
        true
    }

    pub fn is_same(&self, other: &CppFunction) -> bool {
        let member_is_same = match (&self.member, &other.member) {
            (Some(m1), Some(m2)) => m1.is_same(m2),
            (None, None) => true,
            _ => false,
        };
        self.path == other.path
            && member_is_same
            && self.operator == other.operator
            && self.return_type == other.return_type
            && self.argument_types_equal(other)
    }

    pub fn class_path(&self) -> Result<CppPath> {
        if self.member.is_some() {
            Ok(self.path.parent().with_context(|_| {
                err_msg("CppFunction is a class member but its path is not nested.")
            })?)
        } else {
            bail!("not a member function")
        }
    }

    pub fn class_path_parts(&self) -> Result<&[CppPathItem]> {
        if self.member.is_some() {
            Ok(self.path.parent_parts().with_context(|_| {
                err_msg("CppFunction is a class member but its path is not nested.")
            })?)
        } else {
            bail!("not a member function")
        }
    }

    pub fn pseudo_declaration(&self) -> String {
        let mut s = String::new();
        if let Some(info) = &self.member {
            if info.is_virtual {
                write!(s, " virtual").unwrap();
            }
            if info.is_static {
                write!(s, " static").unwrap();
            }
            if info.visibility == CppVisibility::Protected {
                write!(s, " protected").unwrap();
            }
            if info.visibility == CppVisibility::Private {
                write!(s, " private").unwrap();
            }
        }
        write!(s, " {}", self.return_type.to_cpp_pseudo_code()).unwrap();
        write!(s, " {}", self.path.to_cpp_pseudo_code()).unwrap();
        let args = self
            .arguments
            .iter()
            .map(|arg| {
                arg.to_cpp_code().unwrap_or_else(|_| {
                    format!("{} {}", arg.argument_type.to_cpp_pseudo_code(), arg.name,)
                })
            })
            .join(", ");
        write!(
            s,
            "({}{})",
            args,
            if self.allows_variadic_arguments {
                ", ..."
            } else {
                ""
            }
        )
        .unwrap();
        if let Some(info) = &self.member {
            if info.is_const {
                write!(s, " const").unwrap();
            }
        }
        s.trim().to_string()
    }

    /// Returns short text representing values in this method
    /// (only for debugging output).
    pub fn short_text(&self) -> String {
        let mut s = String::new();
        if let Some(info) = &self.member {
            if info.is_virtual {
                if info.is_pure_virtual {
                    s = format!("{} pure virtual", s);
                } else {
                    s = format!("{} virtual", s);
                }
            }
            if info.is_static {
                s = format!("{} static", s);
            }
            if info.visibility == CppVisibility::Protected {
                s = format!("{} protected", s);
            }
            if info.visibility == CppVisibility::Private {
                s = format!("{} private", s);
            }
            if info.is_signal {
                s = format!("{} [signal]", s);
            }
            if info.is_slot {
                s = format!("{} [slot]", s);
            }
            match info.kind {
                CppFunctionKind::Constructor => s = format!("{} [constructor]", s),
                CppFunctionKind::Destructor => s = format!("{} [destructor]", s),
                CppFunctionKind::Regular => {}
            }
        }
        if self.allows_variadic_arguments {
            s = format!("{} [var args]", s);
        }
        s = format!("{} {}", s, self.return_type.to_cpp_pseudo_code());
        s = format!("{} {}", s, self.path.to_cpp_pseudo_code());
        s = format!(
            "{}({})",
            s,
            self.arguments
                .iter()
                .map(|arg| format!(
                    "{} {}{}",
                    arg.argument_type.to_cpp_pseudo_code(),
                    arg.name,
                    if arg.has_default_value {
                        " = …".to_string()
                    } else {
                        String::new()
                    }
                ))
                .join(", ")
        );
        if let Some(info) = &self.member {
            if info.is_const {
                s = format!("{} const", s);
            }
        }
        s.trim().to_string()
    }

    /// Returns true if this method is a constructor.
    pub fn is_constructor(&self) -> bool {
        match &self.member {
            Some(info) => info.kind.is_constructor(),
            None => false,
        }
    }

    pub fn is_copy_constructor(&self) -> bool {
        if !self.is_constructor() {
            return false;
        }
        if self.arguments.len() != 1 {
            return false;
        }
        let arg = CppType::new_reference(true, CppType::Class(self.class_path().unwrap()));
        arg == self.arguments[0].argument_type
    }

    /// Returns true if this method is a destructor.
    pub fn is_destructor(&self) -> bool {
        match &self.member {
            Some(info) => info.kind.is_destructor(),
            None => false,
        }
    }

    /// Returns true if this method is static.
    pub fn is_static_member(&self) -> bool {
        match &self.member {
            Some(info) => info.is_static,
            None => false,
        }
    }

    pub fn is_virtual(&self) -> bool {
        match &self.member {
            Some(info) => info.is_virtual,
            None => false,
        }
    }

    pub fn is_private(&self) -> bool {
        match &self.member {
            Some(info) => info.visibility == CppVisibility::Private,
            None => false,
        }
    }

    pub fn is_signal(&self) -> bool {
        match &self.member {
            Some(info) => info.is_signal,
            None => false,
        }
    }

    pub fn is_slot(&self) -> bool {
        match &self.member {
            Some(info) => info.is_slot,
            None => false,
        }
    }

    pub fn patch_receiver_argument_type(type_text: &str) -> String {
        type_text.replace("QList< QModelIndex >", "QModelIndexList")
    }

    pub fn receiver_id_from_data<'a>(
        receiver_type: RustQtReceiverType,
        name: &'a str,
        arguments: impl IntoIterator<Item = &'a CppType>,
    ) -> Result<String> {
        let type_num = match receiver_type {
            RustQtReceiverType::Signal => "2",
            RustQtReceiverType::Slot => "1",
        };
        // Qt doesn't recognize `QList<QModelIndex>`, e.g. in `QListWidget::indexesMoved`.
        let arguments = arguments
            .map_if_ok(|arg| arg.to_cpp_code(None))?
            .into_iter()
            .map(|arg| Self::patch_receiver_argument_type(&arg))
            .join(",");
        Ok(format!("{}{}({})", type_num, name, arguments))
    }

    /// Returns the identifier that should be used in `QObject::connect`
    /// to specify this signal or slot.
    pub fn receiver_id(&self) -> Result<String> {
        let receiver_type = if let Some(info) = &self.member {
            if info.is_slot {
                RustQtReceiverType::Slot
            } else if info.is_signal {
                RustQtReceiverType::Signal
            } else {
                bail!("not a signal or slot");
            }
        } else {
            bail!("not a class method");
        };
        Self::receiver_id_from_data(
            receiver_type,
            &self.path.last().name,
            self.arguments.iter().map(|arg| &arg.argument_type),
        )
    }

    pub fn member(&self) -> Option<&CppFunctionMemberData> {
        self.member.as_ref()
    }

    /// Returns true if this method is an operator.
    pub fn is_operator(&self) -> bool {
        self.operator.is_some()
    }

    /// Returns collection of all types found in the signature of this method,
    /// including argument types, return type and type of `this` implicit parameter.
    pub fn all_involved_types(&self) -> Vec<CppType> {
        let mut result = Vec::<CppType>::new();
        if let Some(class_membership) = &self.member {
            result.push(CppType::PointerLike {
                is_const: class_membership.is_const,
                kind: CppPointerLikeTypeKind::Pointer,
                target: Box::new(CppType::Class(self.class_path().unwrap())),
            });
        }
        for t in self.arguments.iter().map(|x| x.argument_type.clone()) {
            result.push(t);
        }
        result.push(self.return_type.clone());

        if let Some(template_arguments) = &self.path.last().template_arguments {
            result.extend(template_arguments.clone());
        }
        result
    }

    pub fn can_infer_template_arguments(&self) -> bool {
        if let Some(args) = &self.path.last().template_arguments {
            for t in args {
                if let CppType::TemplateParameter(param) = t {
                    if !self
                        .arguments
                        .iter()
                        .any(|arg| arg.argument_type.contains_template_parameter(param))
                    {
                        return false;
                    }
                }
            }
        }
        true
    }
}
