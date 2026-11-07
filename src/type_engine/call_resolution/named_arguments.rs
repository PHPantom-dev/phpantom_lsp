//! The parameter a named-argument label names.
//!
//! A label — the `name` in `new SelectField(name: 1)`, in
//! `$field->render(name: 1)`, or in the attribute `#[Field(name: 1)]`, which
//! constructs its class the same way — is the only place a parameter's name
//! is written outside the parameter's own declaration, so it has to resolve,
//! navigate, and rename the way a member access does. The symbol map records
//! the label and the call it belongs to; which parameter it names depends on
//! the callee, whose text the call's own
//! [`CallSite`](crate::symbol_map::CallSite) spells and
//! [`resolve_callable_target_at_offset`](Backend::resolve_callable_target_at_offset)
//! resolves.
//!
//! Everything here resolves *through* the callee to the parameter's
//! declaration token, which makes "do these two labels name the same
//! parameter?" an exact question about bytes rather than a name comparison:
//! a subclass that inherits a constructor shares the parent's token, and one
//! that redeclares the method with a parameter of its own does not.
//!
//! Where the declaration has no text to point at — a built-in an embedded
//! stub describes, which has no file to open — the location is `None` and the
//! label still hovers and still shows the type its parameter takes, because
//! the parameter itself came from the resolved callee.

use std::sync::Arc;

use crate::Backend;
use crate::atom::{Atom, atom};
use crate::class_lookup::Fqn;
use crate::hover::MemberKindForOrigin;
use crate::symbol_map::{SymbolKind, SymbolSpan};
use crate::types::{ParameterInfo, ResolvedCallableTarget};
use crate::util::short_name;

/// A function's name as the engine reads the callee of a `new`.
const CONSTRUCT: &str = "__construct";

/// The declaration a named-argument label binds to.
///
/// A constructor-promoted property is declared by the same token as its
/// parameter, so [`Self::name_token`] is that property's declaration site
/// too, and [`Self::promotes_property`] says the label names the property as
/// well as the parameter.
#[derive(Debug, Clone)]
pub(crate) struct NamedArgumentDeclaration {
    /// The file that declares the parameter.
    pub uri: Arc<str>,
    /// `[start, end)` of the parameter's `$name` token.
    ///
    /// `None` when the declaration is one the engine can read but cannot
    /// show: a built-in whose declaration is an embedded stub, which has no
    /// file to open.  What needs a cursor to land on — go-to-definition,
    /// references, rename — has nothing to offer then; hover and the type a
    /// label takes do not need it.
    pub name_token: Option<(u32, u32)>,
    /// The class that declares the function the parameter belongs to, `None`
    /// for a standalone function.
    pub class: Option<Atom>,
    /// Whether the parameter promotes a property of the same name.
    pub promotes_property: bool,
    /// The parameter as the callee declares it, for the hover, the type
    /// definition and the signature a label is worth showing.
    ///
    /// `None` for a location taken by
    /// [`named_argument_location_at`](Self::named_argument_location_at),
    /// which answers what a reference search needs without paying for a
    /// parameter's types and descriptions per label it resolves.
    pub parameter: Option<ParameterInfo>,
}

impl NamedArgumentDeclaration {
    /// Whether both labels name the same parameter.
    ///
    /// Only a declaration with a token answers: two built-ins described by
    /// the same stub file share its URI without sharing a declaration.
    pub(crate) fn is_same_declaration(&self, other: &Self) -> bool {
        self.name_token.is_some() && self.uri == other.uri && self.name_token == other.name_token
    }
}

/// The callable a label's call invokes, as its callee spells it.
enum NamedArgumentCallee {
    /// `new C(...)`, and the attribute `#[C(...)]` that constructs the same
    /// way.
    Constructor { class: Atom },
    /// `$c->m(...)` and `C::m(...)`.
    Method { class: Atom, method: Atom },
    /// `f(...)` — a standalone function, by the name the call writes.
    Function { source: Atom },
}

impl Backend {
    /// The parameter a named-argument label binds to.
    ///
    /// `None` when the label names nothing the engine can read: the call does
    /// not resolve to a named callable (a call through a callable value or a
    /// variable class name), or the callee declares no parameter of that name.
    pub(crate) fn named_argument_declaration(
        &self,
        uri: &str,
        content: &str,
        label: &SymbolSpan,
    ) -> Option<NamedArgumentDeclaration> {
        let SymbolKind::NamedArgument {
            name,
            call_args_start,
        } = &label.kind
        else {
            return None;
        };
        self.named_argument_declaration_at(uri, content, *call_args_start, name)
    }

    /// [`named_argument_declaration`](Self::named_argument_declaration) for a
    /// label whose span a caller has already read apart, as a reference search
    /// scanning a file's labels does.
    pub(crate) fn named_argument_declaration_at(
        &self,
        uri: &str,
        content: &str,
        call_args_start: u32,
        name: &Atom,
    ) -> Option<NamedArgumentDeclaration> {
        self.label_binding(uri, content, call_args_start, name, true)
    }

    /// [`named_argument_declaration`](Self::named_argument_declaration) where
    /// only the declaration's place matters, as a reference, a rename, or a
    /// highlight needs it: the parameter's signature is left unread rather than
    /// cloned for every label a search resolves.
    pub(crate) fn named_argument_location(
        &self,
        uri: &str,
        content: &str,
        label: &SymbolSpan,
    ) -> Option<NamedArgumentDeclaration> {
        let SymbolKind::NamedArgument {
            name,
            call_args_start,
        } = &label.kind
        else {
            return None;
        };
        self.named_argument_location_at(uri, content, *call_args_start, name)
    }

    /// The declaration a label binds, without the parameter's signature.
    ///
    /// For a caller that only compares or reports *where* a label binds — a
    /// reference search reading every label of a file, a highlight matching
    /// labels against one another — where the full declaration's clone of
    /// types and description text would be paid once per label.
    pub(crate) fn named_argument_location_at(
        &self,
        uri: &str,
        content: &str,
        call_args_start: u32,
        name: &Atom,
    ) -> Option<NamedArgumentDeclaration> {
        self.label_binding(uri, content, call_args_start, name, false)
    }

    /// The declaration `name` binds at the call whose argument list opens at
    /// `call_args_start`, keeping the parameter's signature when asked.
    fn label_binding(
        &self,
        uri: &str,
        content: &str,
        call_args_start: u32,
        name: &Atom,
        keep_parameter: bool,
    ) -> Option<NamedArgumentDeclaration> {
        let (callee, target) = self.resolve_named_argument(uri, content, call_args_start, name)?;
        let (mut declaration, is_constructor) = match &callee {
            NamedArgumentCallee::Constructor { class } => {
                (self.declared_parameter(uri, class, CONSTRUCT, name)?, true)
            }
            NamedArgumentCallee::Method { class, method } => (
                self.declared_parameter(uri, class, method.as_str(), name)?,
                false,
            ),
            NamedArgumentCallee::Function { source } => (
                self.function_parameter_declaration(uri, call_args_start, source, name)?,
                false,
            ),
        };
        declaration.promotes_property =
            is_constructor && self.constructor_promotes(&declaration, name);
        declaration.parameter = keep_parameter
            .then(|| find_parameter(&target, name))
            .flatten();
        Some(declaration)
    }

    /// The parameter `name` binds, with the callee it binds it on.
    ///
    /// `None` when the call resolves to nothing, or to a callable that
    /// declares no parameter of that name.
    fn resolve_named_argument(
        &self,
        uri: &str,
        content: &str,
        call_args_start: u32,
        name: &Atom,
    ) -> Option<(NamedArgumentCallee, ResolvedCallableTarget)> {
        let target = self.resolved_call_target(uri, content, call_args_start)?;
        if !target
            .parameters
            .iter()
            .any(|parameter| parameter.bare_name() == name.as_str())
        {
            return None;
        }

        // A constructor and an attribute `#[C(...)]` both invoke
        // `__construct`; a member call names its method in the callee's last
        // segment; a call the resolver gave a class to reached a member
        // through something else, and a bare call is a function.
        let map = self.symbol_map_for(uri)?;
        let site = map.call_sites.site_at(call_args_start)?;
        let callee = if site.call_expression.starts_with("new ") {
            NamedArgumentCallee::Constructor {
                class: target.owner_class?,
            }
        } else if let Some(class) = target.owner_class {
            NamedArgumentCallee::Method {
                class,
                method: member_of_callee(site.call_expression)?,
            }
        } else {
            NamedArgumentCallee::Function {
                source: atom(site.call_expression),
            }
        };
        Some((callee, target))
    }

    /// The callable the label's own argument list belongs to.
    fn resolved_call_target(
        &self,
        uri: &str,
        content: &str,
        call_args_start: u32,
    ) -> Option<ResolvedCallableTarget> {
        let map = self.symbol_map_for(uri)?;
        let site = map.call_sites.site_at(call_args_start)?;
        let file_ctx = self.file_context(uri);
        self.resolve_callable_target_at_offset(
            site.call_expression,
            content,
            call_args_start,
            &file_ctx,
        )
    }

    /// The declaration of a standalone function's parameter.
    ///
    /// The name is looked up the way PHP looks a call up: the file's imports
    /// and namespace first, then the global namespace, which an unqualified
    /// call reaches whenever the namespaced name declares nothing.  A call in
    /// `namespace App;` to a global `helper()` is the usual arrangement, and
    /// the callee's parameter list comes from the global function, so
    /// resolving only the namespaced guess would find no declaration.
    fn function_parameter_declaration(
        &self,
        call_uri: &str,
        call_args_start: u32,
        source: &Atom,
        name: &Atom,
    ) -> Option<NamedArgumentDeclaration> {
        let candidates = self.function_lookup_candidates_at(call_uri, call_args_start, source);
        let (uri, name_offset): (Arc<str>, u32) = {
            let functions = self.global_functions().read();
            let (uri, info) = candidates
                .iter()
                .find_map(|fqn| functions.get(fqn.as_str()))?;
            (uri.as_str().into(), info.name_offset)
        };
        Some(self.parameter_declaration(uri, name_offset, name, None))
    }

    /// The parameter declared by a class' function, found through the class
    /// that declares that function rather than the class the call names: the
    /// two differ when the method is inherited, and only the declaring class
    /// owns the token every label naming it has to agree on.
    fn declared_parameter(
        &self,
        call_uri: &str,
        class: &Atom,
        function: &str,
        name: &Atom,
    ) -> Option<NamedArgumentDeclaration> {
        let owner = self.find_or_load_class(class)?;
        let class_loader = |name: Fqn<'_>| self.find_or_load_class(&name);
        let declaring = crate::hover::find_declaring_class(
            &owner,
            function,
            &MemberKindForOrigin::Method,
            &class_loader,
        );
        let name_offset = declaring.get_method(function)?.name_offset;
        let declaring_fqn = declaring.fqn();
        // Looked up by the exact FQN first: the file lookup by name prefers
        // the calling file when it declares a class of the same short name,
        // which a global `\Tag` and an `App\Tag` beside the call share.
        let uri: Arc<str> = self
            .class_declaration_uri(&declaring_fqn)
            .or_else(|| self.find_class_file_uri(&declaring_fqn, call_uri))?
            .as_str()
            .into();
        Some(self.parameter_declaration(uri, name_offset, name, Some(declaring_fqn)))
    }

    /// The label's view of a parameter declared at `name_offset` in the file
    /// `uri`, by the function-like whose name token sits there.
    ///
    /// A file the engine keeps no symbol map for — an embedded stub, which
    /// has no text to open — leaves the label naming no token, so it
    /// navigates to nothing rather than to a guess.
    fn parameter_declaration(
        &self,
        uri: Arc<str>,
        name_offset: u32,
        name: &Atom,
        class: Option<Atom>,
    ) -> NamedArgumentDeclaration {
        // The declaring file's own map holds the parameter's token. A file the
        // engine keeps no map for (an embedded stub) leaves the label naming
        // no token, so it navigates to nothing rather than to a guess.
        let name_token = self.symbol_map_for(&uri).and_then(|map| {
            map.function_parameter_definition(name_offset, name.as_str())
                .map(|def| def.token_range())
        });
        NamedArgumentDeclaration {
            uri,
            name_token,
            class,
            promotes_property: false,
            parameter: None,
        }
    }

    /// Whether the constructor parameter a declaration names promotes the
    /// property of that name, which is what makes a label a reference to the
    /// property and not only to the parameter.
    ///
    /// The declaration already names the class whose constructor runs, so
    /// this asks the same question the rest of the engine asks of a promoted
    /// parameter (see [`crate::class_lookup::constructor_promotes`]).
    fn constructor_promotes(&self, declaration: &NamedArgumentDeclaration, name: &Atom) -> bool {
        let Some(class) = declaration.class else {
            return false;
        };
        let Some(info) = self.find_or_load_class(&class) else {
            return false;
        };
        let class_loader = |name: Fqn<'_>| self.find_or_load_class(&name);
        crate::class_lookup::constructor_promotes(&info, name.as_str(), &class_loader)
    }
}

/// The parameter a resolved callable declares under this label.
///
/// PHP binds a label by the exact name a parameter spells, so a `$Name`
/// parameter takes no `name:`.
fn find_parameter(target: &ResolvedCallableTarget, name: &Atom) -> Option<ParameterInfo> {
    target
        .parameters
        .iter()
        .find(|parameter| parameter.bare_name() == name.as_str())
        .cloned()
}

/// The method a call expression invokes: the segment after the last `->` or
/// `::`, which is the member the callee text was built from.
fn member_of_callee(expression: &str) -> Option<Atom> {
    let method = expression
        .rsplit_once("->")
        .or_else(|| expression.rsplit_once("::"))?
        .1;
    if method.is_empty() {
        return None;
    }
    Some(atom(short_name(method)))
}
