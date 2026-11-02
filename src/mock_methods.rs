//! Which class a mock expectation's method name belongs to.
//!
//! The symbol map records the string in `$repo->method('findOpen')` as a
//! [`MockedMethod`](crate::symbol_map::DocblockMemberRef::MockedMethod)
//! access on `$repo`, without knowing whether `$repo` is a mock at all.
//! This module is the one place that decides, from the classes the
//! receiver resolved to, which class the name is looked up on:
//!
//! - An instance receiver must be a mock: its type has to include one of
//!   PHPUnit's or Mockery's mock interfaces, as the `MockObject&Foo` that
//!   `createMock()` returns and the `MockInterface&Foo` that
//!   `Mockery::mock()` returns do.  The name belongs to the rest of the
//!   intersection, the mocked class.
//! - A static receiver must be a Laravel facade, whose `shouldReceive()`
//!   mocks the facade's target.  The facade itself is kept: its static
//!   members already forward to the target, exactly as for a call written
//!   `Cache::get()`.
//!
//! Anything else is not an expectation, and yields nothing.
//!
//! Which calls take a method name, and in which arguments, is listed here
//! too, so the symbol map's spans and completion agree on it.

use std::sync::Arc;

use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of};
use crate::types::ClassInfo;

/// How an expectation method names the methods it mocks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameArgs {
    /// The first argument names one method: PHPUnit's `method()`,
    /// Mockery's `expects()` and `shouldHaveReceived()`.
    First,
    /// The first argument names one method, or is a `['method' => $return]`
    /// map: Mockery's `allows()`.
    FirstOrMap,
    /// Every argument names a method, or the first is a map: Mockery's
    /// `shouldReceive()`.
    AllOrMap,
    /// Every argument names a method: Mockery's `shouldNotReceive()`.
    All,
}

impl NameArgs {
    /// Whether the argument at `index` can name a method.
    pub(crate) fn names_at(self, index: usize) -> bool {
        index == 0 || matches!(self, Self::AllOrMap | Self::All)
    }

    /// Whether the first argument can be a map keyed by method names.
    pub(crate) fn takes_map(self) -> bool {
        matches!(self, Self::FirstOrMap | Self::AllOrMap)
    }
}

/// The instance methods of PHPUnit's and Mockery's mocks that take the
/// name of a mocked method.
const INSTANCE_METHODS: &[(&str, NameArgs)] = &[
    ("method", NameArgs::First),
    ("expects", NameArgs::First),
    ("shouldHaveReceived", NameArgs::First),
    ("shouldNotHaveReceived", NameArgs::First),
    ("allows", NameArgs::FirstOrMap),
    ("shouldReceive", NameArgs::AllOrMap),
    ("shouldNotReceive", NameArgs::All),
];

/// The static methods of Laravel's `Facade` that take the name of a
/// method of the facade's target.
const FACADE_METHODS: &[(&str, NameArgs)] = &[
    ("shouldReceive", NameArgs::AllOrMap),
    ("expects", NameArgs::First),
];

/// How a call of `method` (through `::` when `is_static`) names mocked
/// methods, if it is an expectation method.  PHP method names are
/// case-insensitive.
pub(crate) fn expectation_name_args(method: &str, is_static: bool) -> Option<NameArgs> {
    let table = if is_static {
        FACADE_METHODS
    } else {
        INSTANCE_METHODS
    };
    table
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(method))
        .map(|&(_, kind)| kind)
}

/// The interfaces a PHPUnit or Mockery test double implements.
const MOCK_INTERFACES: &[&str] = &[
    "PHPUnit\\Framework\\MockObject\\Stub",
    "PHPUnit\\Framework\\MockObject\\MockObject",
    "Mockery\\LegacyMockInterface",
    "Mockery\\MockInterface",
];

const FACADE_FQN: &str = "Illuminate\\Support\\Facades\\Facade";

/// Whether `class` is part of the mocking framework rather than the
/// mocked class.
fn is_mock_interface(class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
    let fqn = class.fqn();
    MOCK_INTERFACES.contains(&fqn.as_str())
        || MOCK_INTERFACES
            .iter()
            .any(|interface| is_subtype_of(class, interface, class_loader))
}

/// The classes a mock expectation on a receiver that resolved to
/// `classes` names a method of; empty when the receiver is not a mock.
pub(crate) fn mocked_classes(
    classes: Vec<Arc<ClassInfo>>,
    is_static: bool,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<Arc<ClassInfo>> {
    if is_static {
        return classes
            .into_iter()
            .filter(|class| is_subtype_of(class, FACADE_FQN, class_loader))
            .collect();
    }
    let (framework, mocked): (Vec<_>, Vec<_>) = classes
        .into_iter()
        .partition(|class| is_mock_interface(class, class_loader));
    if framework.is_empty() {
        return Vec::new();
    }
    mocked
}

/// [`mocked_classes`] for a receiver resolved to class names.
pub(crate) fn mocked_class_names(
    fqns: Vec<String>,
    is_static: bool,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<String> {
    let load = |fqn: &str| class_loader(Fqn::new(fqn));
    if is_static {
        return fqns
            .into_iter()
            .filter(|fqn| {
                load(fqn).is_some_and(|class| is_subtype_of(&class, FACADE_FQN, class_loader))
            })
            .collect();
    }
    let (framework, mocked): (Vec<_>, Vec<_>) = fqns.into_iter().partition(|fqn| match load(fqn) {
        Some(class) => is_mock_interface(&class, class_loader),
        None => MOCK_INTERFACES.contains(&fqn.trim_start_matches('\\')),
    });
    if framework.is_empty() {
        return Vec::new();
    }
    mocked
}
