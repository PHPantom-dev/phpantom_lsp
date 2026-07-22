/// Context-specific completion sub-modules.
///
/// This group contains modules that provide completions for specific
/// syntactic contexts (as opposed to member-access completion):
/// - **catch_completion**: Smart exception type completion inside `catch()` clauses
/// - **class_completion**: Class name completions (class, interface, trait, enum)
/// - **constant_completion**: Global constant name completions
/// - **function_completion**: Standalone function name completions
/// - **namespace_completion**: Namespace declaration completions
/// - **type_hint_completion**: Type completion inside function/method parameter lists,
///   return types, and property declarations
pub(crate) mod catch_completion;
pub(crate) mod class_completion;
pub(crate) mod constant_completion;
pub(crate) mod function_completion;
pub(crate) mod keyword_completion;
pub(crate) mod namespace_completion;
pub(crate) mod override_completion;
pub(crate) mod symbol_ranking;
pub(crate) mod type_hint_completion;
