/// Goto definition, go-to-implementation, and go-to-type-definition support.
///
/// This module contains the logic for resolving "go to definition",
/// "go to implementation", and "go to type definition" requests, allowing
/// users to jump from a symbol reference to its definition, concrete
/// implementations, or the declaration of its resolved type.
///
/// The [`point_location`] helper constructs a zero-width `Location`
/// (start == end), which is the standard shape for "go to definition"
/// results.
///
/// Supported symbols (definition):
///   - **Class-like types**: class, interface, trait, enum references
///   - **Methods**: `$this->method()`, `self::method()`, `MyClass::method()`, `$var->method()`
///   - **Properties**: `$this->property`, `$var->property`, `MyClass::$staticProp`
///   - **Constants**: `self::MY_CONST`, `MyClass::MY_CONST`, `parent::MY_CONST`
///   - **Chained access**: `$this->prop->method()`
///   - **Variables**: `$var` jumps to the most recent assignment or declaration
///     (assignment, parameter, foreach, catch, static/global)
///
/// Supported symbols (implementation):
///   - **Interface names**: jumps to all classes that implement the interface
///   - **Abstract class names**: jumps to all classes that extend the abstract class
///   - **Method calls on interfaces/abstract classes**: jumps to the concrete
///     method implementations in all implementing/extending classes
///
/// Supported symbols (type definition):
///   - **Variables**: `$var` jumps to the class declaration of the resolved type
///   - **Member access**: `$var->method()` jumps to the return type's class
///   - **Properties**: `$var->prop` jumps to the property type's class
///   - **`self`/`static`/`parent`/`$this`**: jumps to the enclosing or parent class
///   - **Function calls**: `foo()` jumps to the return type's class
///   - For union types, multiple locations are returned (one per class)
///
/// - [`resolve`]: Core entry points — word extraction, name resolution,
///   same-file / PSR-4 definition lookup, `self`/`static`/`parent` handling,
///   and standalone function definition resolution.
/// - [`member`]: Member-access resolution — `->`, `?->`, `::` operator
///   detection, subject extraction, member classification, inheritance-chain
///   walking (parent classes, traits, mixins), and member position lookup.
/// - [`variable`]: Variable definition resolution — `$var` jump-to-definition,
///   assignment / parameter / foreach / catch detection, and type-hint
///   resolution at definition sites.
/// - [`implementation`]: Go-to-implementation — finding concrete classes that
///   implement an interface or extend an abstract class, and locating the
///   concrete method definitions within those classes.
/// - [`type_definition`]: Go-to-type-definition — resolving the type of a
///   variable, expression, or member access, then jumping to the class
///   declaration of that type.
use tower_lsp::lsp_types::{Location, Position, Range, Url};

mod implementation;
pub(crate) mod member;
mod resolve;
mod type_definition;

/// Build an LSP `Location` with a zero-width range (start == end).
///
/// Almost every "go to definition" result points to a single position
/// rather than a span.  This helper eliminates the repeated 5-line
/// `Location { uri, range: Range { start: pos, end: pos } }` blocks
/// found throughout the definition modules.
pub(crate) fn point_location(uri: Url, position: Position) -> Location {
    Location {
        uri,
        range: Range {
            start: position,
            end: position,
        },
    }
}
