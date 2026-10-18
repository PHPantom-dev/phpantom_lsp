//! The syntactic position a class name is completed in, and which
//! class-likes each position accepts.

use crate::types::*;

use super::heuristics::{
    likely_attribute_name, likely_interface_name, likely_non_instantiable,
    likely_non_interface_name,
};

/// The syntactic context in which a class name is being completed.
///
/// Different PHP positions accept only certain kinds of class-like
/// declarations. For example, `extends` in a class declaration only
/// accepts non-final classes, while `implements` only accepts interfaces.
/// This enum lets `build_class_name_completions` filter out invalid
/// suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassNameContext {
    /// No specific context — any class-like is valid.
    Any,
    /// After `new` keyword — only concrete (non-abstract) classes.
    New,
    /// After `throw new` — only concrete (non-abstract) classes that
    /// implement `Throwable`.
    ThrowNew,
    /// Inside a `catch()` clause or `@throws` docblock tag — any
    /// class or interface that implements `Throwable`.
    Catch,
    /// After `extends` in a class declaration — only non-final,
    /// non-abstract-only classes.
    ExtendsClass,
    /// After `extends` in an interface declaration — only interfaces.
    ExtendsInterface,
    /// After `implements` — only interfaces.
    Implements,
    /// After `use` inside a class body — only traits.
    TraitUse,
    /// After `instanceof` — classes, interfaces, enums (not traits).
    Instanceof,
    /// In a type-hint position (parameter type, return type, property
    /// type) or PHPDoc type reference (`@param`, `@return`, `@var`).
    /// Accepts classes, interfaces, and enums but rejects traits.
    TypeHint,
    /// After `use` at the top level — any class (namespace import).
    /// Treated specially: FQN is always inserted, no `use` text-edit.
    UseImport,
    /// After `use function` — only functions (handled elsewhere).
    UseFunction,
    /// After `use const` — only constants (handled elsewhere).
    UseConst,
    /// After `namespace` keyword at the top level — namespace names
    /// (handled by `namespace_completion`).
    NamespaceDeclaration,
    /// Inside a PHP attribute list (`#[…]`).
    ///
    /// Only classes decorated with `#[\Attribute]` are valid here.
    /// The inner `u8` is the attribute target bitmask for the
    /// declaration the attribute is applied to (e.g.
    /// [`attribute_target::TARGET_CLASS`] when writing `#[…]` above a
    /// class).  Loaded classes are hard-filtered by target; unloaded
    /// classes are demoted when their short name does not contain
    /// "Attribute".
    Attribute(u8),
}

impl ClassNameContext {
    /// Check whether a `ClassInfo` matches this context.
    pub(crate) fn matches(&self, cls: &ClassInfo) -> bool {
        match self {
            Self::Any => true,
            Self::New => cls.kind == ClassLikeKind::Class && !cls.is_abstract,
            Self::ThrowNew => cls.kind == ClassLikeKind::Class && !cls.is_abstract,
            Self::Catch => matches!(cls.kind, ClassLikeKind::Class | ClassLikeKind::Interface),
            Self::ExtendsClass => cls.kind == ClassLikeKind::Class && !cls.is_final,
            Self::ExtendsInterface => cls.kind == ClassLikeKind::Interface,
            Self::Implements => cls.kind == ClassLikeKind::Interface,
            Self::TraitUse => cls.kind == ClassLikeKind::Trait,
            Self::Instanceof | Self::TypeHint => cls.kind != ClassLikeKind::Trait,
            Self::UseImport => true,
            Self::UseFunction | Self::UseConst | Self::NamespaceDeclaration => false,
            Self::Attribute(target) => {
                cls.attribute_targets != 0 && (cls.attribute_targets & target) != 0
            }
        }
    }

    /// Check whether a class-like matches this context given only kind
    /// flags (used by tests for `detect_stub_class_kind`).
    #[cfg(test)]
    pub(crate) fn matches_kind_flags(
        &self,
        kind: ClassLikeKind,
        is_abstract: bool,
        is_final: bool,
    ) -> bool {
        match self {
            Self::Any | Self::UseImport => true,
            Self::New => kind == ClassLikeKind::Class && !is_abstract,
            Self::ThrowNew => kind == ClassLikeKind::Class && !is_abstract,
            Self::Catch => matches!(kind, ClassLikeKind::Class | ClassLikeKind::Interface),
            Self::ExtendsClass => kind == ClassLikeKind::Class && !is_final,
            Self::ExtendsInterface => kind == ClassLikeKind::Interface,
            Self::Implements => kind == ClassLikeKind::Interface,
            Self::TraitUse => kind == ClassLikeKind::Trait,
            Self::Instanceof | Self::TypeHint => kind != ClassLikeKind::Trait,
            Self::UseFunction | Self::UseConst | Self::NamespaceDeclaration => false,
            // Cannot verify attribute status from kind flags alone.
            Self::Attribute(_) => true,
        }
    }

    /// Whether only class-like names are valid in this context (as
    /// opposed to constants and functions).
    pub(crate) fn is_class_only(&self) -> bool {
        matches!(
            self,
            Self::New
                | Self::ThrowNew
                | Self::Catch
                | Self::ExtendsClass
                | Self::ExtendsInterface
                | Self::Implements
                | Self::TraitUse
                | Self::Instanceof
                | Self::TypeHint
                | Self::UseImport
                | Self::Attribute(_)
        )
    }

    /// Whether this context implies instantiation (`New` or `ThrowNew`).
    pub(crate) fn is_new(&self) -> bool {
        matches!(self, Self::New | Self::ThrowNew)
    }

    /// Whether this context is inside an attribute list (`#[…]`).
    pub(crate) fn is_attribute(&self) -> bool {
        matches!(self, Self::Attribute(_))
    }

    /// Whether this context requires a very specific class-like kind
    /// (trait, interface, etc.) and should reject unverifiable entries.
    pub(crate) fn is_narrow_kind(&self) -> bool {
        matches!(
            self,
            Self::TraitUse | Self::Implements | Self::ExtendsInterface
        )
    }

    /// Heuristic: names that are unlikely to match this context.
    ///
    /// Used to demote (but not exclude) unloaded classes whose actual
    /// kind or attribute status cannot be verified.  For example,
    /// `new AbstractFoo` is very likely wrong, and `#[UserService]`
    /// is unlikely to be an attribute class.
    ///
    /// This should only be called for classes that could NOT be loaded
    /// (i.e. `matches_context_or_unloaded` returned `true` because the
    /// class was unloaded, not because it passed `matches()`).
    pub(crate) fn likely_mismatch(&self, short_name: &str) -> bool {
        match self {
            Self::New => likely_non_instantiable(short_name),
            Self::ThrowNew => {
                // Demote if it doesn't look instantiable OR doesn't look
                // like an exception class.
                likely_non_instantiable(short_name)
                    || (!short_name.ends_with("Exception")
                        && !short_name.ends_with("Error")
                        && !short_name.ends_with("Throwable"))
            }
            Self::Catch => {
                !short_name.ends_with("Exception")
                    && !short_name.ends_with("Error")
                    && !short_name.ends_with("Throwable")
            }
            Self::ExtendsClass => likely_interface_name(short_name),
            Self::Implements | Self::ExtendsInterface => likely_non_interface_name(short_name),
            Self::TraitUse => likely_non_instantiable(short_name),
            Self::Attribute(_) => !likely_attribute_name(short_name),
            _ => false,
        }
    }
}
