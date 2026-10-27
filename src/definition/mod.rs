//! Goto definition, go-to-implementation, and go-to-type-definition support.
//!
//! This module contains the logic for resolving "go to definition",
//! "go to implementation", and "go to type definition" requests, allowing
//! users to jump from a symbol reference to its definition, concrete
//! implementations, or the declaration of its resolved type.
//!
//! The [`point_location`] helper constructs a zero-width `Location`
//! (start == end), which is the standard shape for "go to definition"
//! results.
//!
//! Supported symbols (definition):
//!   - **Class-like types**: class, interface, trait, enum references
//!   - **Methods**: `$this->method()`, `self::method()`, `MyClass::method()`, `$var->method()`
//!   - **Properties**: `$this->property`, `$var->property`, `MyClass::$staticProp`
//!   - **Constants**: `self::MY_CONST`, `MyClass::MY_CONST`, `parent::MY_CONST`
//!   - **Chained access**: `$this->prop->method()`
//!   - **Variables**: `$var` jumps to the most recent assignment or declaration
//!     (assignment, parameter, foreach, catch, static/global)
//!   - **Blade component tags**: `<x-alert>` / `<livewire:counter>` jump to the
//!     class backing the tag, or to the template an anonymous component renders
//!
//! Supported symbols (implementation):
//!   - **Interface names**: jumps to all classes that implement the interface
//!   - **Abstract class names**: jumps to all classes that extend the abstract class
//!   - **Method calls on interfaces/abstract classes**: jumps to the concrete
//!     method implementations in all implementing/extending classes
//!
//! Supported symbols (declaration):
//!   - **Member calls and member declarations**: jumps to the interface or
//!     abstract method the member implements, else to the definition
//!
//! Supported symbols (type definition):
//!   - **Variables**: `$var` jumps to the class declaration of the resolved type
//!   - **Member access**: `$var->method()` jumps to the return type's class
//!   - **Properties**: `$var->prop` jumps to the property type's class
//!   - **`self`/`static`/`parent`/`$this`**: jumps to the enclosing or parent class
//!   - **Function calls**: `foo()` jumps to the return type's class
//!   - For union types, multiple locations are returned (one per class)
//!
//! - [`blade_component`]: Component tags — resolving `<x-…>` / `<livewire:…>`
//!   against the Blade discovery index from the raw template source.
//! - [`resolve`]: Core entry points — symbol-map lookup, name resolution,
//!   same-file / PSR-4 definition lookup, `self`/`static`/`parent` handling,
//!   and variable definition
//!   resolution (`$var` jump-to-definition, assignment / parameter /
//!   foreach / catch detection, and type-hint resolution at definition
//!   sites).
//! - [`global_symbols`]: Standalone constant and function definitions —
//!   candidate lookup in the global maps, lazy parsing of autoload files,
//!   and the jump to the stored name offset.
//! - [`member`]: Member-access resolution — `->`, `?->`, `::` operator
//!   detection, subject extraction, member classification, inheritance-chain
//!   walking (parent classes, traits, mixins), and member position lookup.
//! - [`implementation`]: Go-to-implementation — finding concrete classes that
//!   implement an interface or extend an abstract class, and locating the
//!   concrete method definitions within those classes.
//! - [`declaration`]: Go-to-declaration — the interface or abstract
//!   prototype of a member, falling back to go-to-definition.
//! - [`type_definition`]: Go-to-type-definition — resolving the type of a
//!   variable, expression, or member access, then jumping to the class
//!   declaration of that type.

use tower_lsp_server::gen_lsp_types::{Location, Position, Range, Uri};

use crate::types::ClassInfo;

mod blade_component;
mod declaration;
mod global_symbols;
pub(crate) mod implementation;
pub(crate) mod member;
mod resolve;
mod type_definition;

/// Build an LSP `Location` with a zero-width range (start == end).
///
/// Almost every "go to definition" result points to a single position
/// rather than a span.  This helper eliminates the repeated 5-line
/// `Location { uri, range: Range { start: pos, end: pos } }` blocks
/// found throughout the definition modules.
pub(crate) fn point_location(uri: Uri, position: Position) -> Location {
    Location {
        uri,
        range: Range {
            start: position,
            end: position,
        },
    }
}

/// The location of a class declaration's keyword (`class`, `interface`,
/// `trait`, `enum`) in the file `uri` whose source is `content`.
///
/// Returns `None` for a class without a recorded keyword offset (a
/// synthesized class) or when `uri` does not parse.
pub(crate) fn class_keyword_location(
    class: &ClassInfo,
    uri: &str,
    content: &str,
) -> Option<Location> {
    if class.keyword_offset == 0 {
        return None;
    }
    let position = crate::text_position::offset_to_position(content, class.keyword_offset as usize);
    Some(point_location(crate::util::parse_url(uri)?, position))
}
