use super::*;
use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;

use crate::atom::{Atom, atom, bytes_to_str};
use crate::parser::unwrap_parens;
use crate::php_type::{LiteralValue, PhpType, TypeKind};
use crate::type_engine::resolver::VarResolutionCtx;
use crate::type_engine::types::narrowing;
use crate::types::{MethodInfo, PropertyInfo, ResolvedType};

mod apply;
mod assertion_aliases;
mod assertions;
mod cursor_ternary;
mod emptiness;
mod in_array;
mod instanceof;
mod key_types;
mod logical_chains;
mod member_exists;
mod nested_assignments;
mod null_identity;
mod null_narrowing;
mod phpstan_assert;
mod predicates;
mod preg;
mod property_checks;
mod scope_edits;
mod seeding;
mod type_guards;

use assertions::*;
use emptiness::*;
use instanceof::*;
use key_types::*;
use logical_chains::*;
use null_identity::*;
use property_checks::*;

pub(crate) use apply::*;
pub(crate) use assertion_aliases::*;
pub(crate) use cursor_ternary::*;
pub(crate) use emptiness::apply_switch_arm_narrowing;
pub(crate) use in_array::*;
pub(crate) use member_exists::*;
pub(crate) use nested_assignments::*;
pub(crate) use null_narrowing::*;
pub(crate) use phpstan_assert::*;
pub(crate) use predicates::*;
pub(crate) use preg::*;
pub(crate) use scope_edits::*;
pub(crate) use seeding::*;
pub(crate) use type_guards::*;
