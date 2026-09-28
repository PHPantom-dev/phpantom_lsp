/// Type resolution sub-modules.
///
/// This group contains modules related to type resolution:
/// - **aliases**: expanding `@phpstan-type` aliases into member signatures
/// - **resolution**: Type-hint string to `ClassInfo` mapping (unions,
///   intersections, generics, type aliases, object shapes, property types)
/// - **narrowing**: instanceof / assert / custom type guard narrowing
/// - **conditional**: PHPStan conditional return type resolution at call sites
/// - **flag_returns**: builtin return types decided by a bit of a flags
///   argument, which a conditional return type cannot express
/// - **const_fold**: folding a constant expression to the value PHP would
///   compute for it, so a constant defined from other constants is readable
pub mod aliases;
pub mod conditional;
pub mod const_fold;
pub mod flag_returns;
pub mod narrowing;
pub mod resolution;
