//! Integration tests for typing a property read through the Reflection API.
//!
//! `ReflectionClass::getProperty()` returns a bare `ReflectionProperty` and
//! `ReflectionProperty::getValue()` a bare `mixed`, so a reflected read used
//! to lose the type the property declares. It is recoverable whenever the
//! reflected class is known and the property name is a literal, which is the
//! shape reflection-based accessors are written in.

use crate::common::{assert_assigned_types_on, create_test_backend_with_full_stubs};

fn assert_assigned_types(content: &str, expected: &[(&str, &str)]) {
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///reflection_property_types.php";
    // Reading a method body for its return type re-opens the file the
    // method was declared in, so the content has to be reachable by URI
    // and not just parsed into the symbol index.
    backend
        .open_files()
        .write()
        .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));
    backend.update_ast(uri, content);
    assert_assigned_types_on(&backend, uri, content, expected);
}

const FIXTURE: &str = r#"<?php
class Shell {
    const VERSION = 'v1.0.0';
}
class BaseConfiguration {
    protected ?Shell $inheritedShell = null;
}
class Configuration extends BaseConfiguration {
    private ?Shell $shell = null;
    private int $verbosity = 0;
    private $untyped = null;
}
function probe(Configuration $config, string $dynamicName, $unknown): void {
    $reflObject = new \ReflectionObject($config);
    $reflClass = new \ReflectionClass(Configuration::class);
    $reflNamed = new \ReflectionClass('Configuration');

    $property = $reflObject->getProperty('shell');
    $shellValue = $property->getValue($config);
    $verbosityValue = $reflClass->getProperty('verbosity')->getValue($config);
    $namedValue = $reflNamed->getProperty('shell')->getValue($config);
    $inheritedValue = $reflObject->getProperty('inheritedShell')->getValue($config);

    $dynamicValue = $reflObject->getProperty($dynamicName)->getValue($config);
    $untypedValue = $reflObject->getProperty('untyped')->getValue($config);
    $absentValue = $reflObject->getProperty('noSuchProperty')->getValue($config);

    $reflUnknown = new \ReflectionObject($unknown);
    $unknownValue = $reflUnknown->getProperty('shell')->getValue($unknown);

    $direct = new \ReflectionProperty(Configuration::class, 'shell');
    $directValue = $direct->getValue($config);
    $directNamedClass = (new \ReflectionProperty('Configuration', 'verbosity'))->getValue($config);
    $directInstance = (new \ReflectionProperty($config, 'inheritedShell'))->getValue($config);
    $directLabelled = (new \ReflectionProperty(property: 'shell', class: Configuration::class))->getValue($config);

    $directDynamic = (new \ReflectionProperty(Configuration::class, $dynamicName))->getValue($config);
    $directUnknownClass = (new \ReflectionProperty($unknown, 'shell'))->getValue($unknown);
}
"#;

/// `ReflectionObject` is `ReflectionClass` narrowed to an instance, but
/// phpstorm-stubs give it neither the `@template` nor the `@extends`, so it
/// used to forget the class it reflects.
#[test]
fn reflection_object_carries_the_class_it_reflects() {
    assert_assigned_types(
        FIXTURE,
        &[
            ("$reflObject", "ReflectionObject<Configuration>"),
            ("$reflClass", "ReflectionClass<Configuration>"),
            ("$reflNamed", "ReflectionClass<Configuration>"),
        ],
    );
}

/// A reflected read types as the property does, whether the property is
/// declared on the reflected class or inherited, and whichever spelling
/// produced the reflection.
#[test]
fn a_reflected_property_read_types_as_the_property_declares() {
    assert_assigned_types(
        FIXTURE,
        &[
            ("$property", "ReflectionProperty<Configuration, 'shell'>"),
            ("$shellValue", "?Shell"),
            ("$verbosityValue", "int"),
            ("$namedValue", "?Shell"),
            ("$inheritedValue", "?Shell"),
        ],
    );
}

/// `new ReflectionProperty(C::class, 'name')` and
/// `(new ReflectionClass(C::class))->getProperty('name')` are the same value
/// written two ways, so they carry the same class and name. The class may be
/// named by a `::class` constant, a quoted name, or an instance, and the two
/// arguments may arrive labelled.
#[test]
fn a_directly_constructed_reflection_property_carries_what_it_reflects() {
    assert_assigned_types(
        FIXTURE,
        &[
            ("$direct", "ReflectionProperty<Configuration, 'shell'>"),
            ("$directValue", "?Shell"),
            ("$directNamedClass", "int"),
            ("$directInstance", "?Shell"),
            ("$directLabelled", "?Shell"),
        ],
    );
}

/// Everything the rule cannot decide keeps `getValue()`'s declared `mixed`:
/// a property name that is not a literal, a property with no declared type,
/// a name that matches no property, and a reflected value whose class is
/// unknown.
#[test]
fn an_undecidable_reflected_read_stays_mixed() {
    assert_assigned_types(
        FIXTURE,
        &[
            ("$dynamicValue", "mixed"),
            ("$untypedValue", "mixed"),
            ("$absentValue", "mixed"),
            ("$unknownValue", "mixed"),
            ("$directDynamic", "mixed"),
            ("$directUnknownClass", "mixed"),
        ],
    );
}

/// A reflection-based accessor, which is where a reflected read is usually
/// written: the class and the property name arrive as arguments, so nothing
/// in the accessor's own signature can say what it hands back.
///
/// `Accessor::fetchProperty()` is `Psy\Sudo::fetchProperty()` verbatim, down
/// to the `@return mixed` and the untyped `$object`.
const ACCESSOR_FIXTURE: &str = r#"<?php
class Shell {
    const VERSION = 'v1.0.0';
}
class Configuration {
    private ?Shell $shell = null;
    private int $verbosity = 0;
}
class Accessor {
    /**
     * @return mixed Value of $object->property
     */
    public static function fetchProperty($object, string $property)
    {
        $prop = self::getProperty(new \ReflectionObject($object), $property);

        return $prop->getValue($object);
    }

    private static function getProperty(\ReflectionClass $refl, string $property): \ReflectionProperty
    {
        return $refl->getProperty($property);
    }

    public function forward(Configuration $config, string $property)
    {
        return self::fetchProperty($config, $property);
    }
}
function probe(Configuration $config, string $dynamicName): void {
    $shell = Accessor::fetchProperty($config, 'shell');
    $verbosity = Accessor::fetchProperty($config, 'verbosity');
    $dynamic = Accessor::fetchProperty($config, $dynamicName);
    $forwarded = (new Accessor())->forward($config, 'shell');
}
"#;

/// The accessor's result is whatever its arguments decided, so the same
/// method answers differently at each call site.
#[test]
fn an_accessor_returns_what_its_arguments_decide() {
    assert_assigned_types(
        ACCESSOR_FIXTURE,
        &[("$shell", "Shell"), ("$verbosity", "int")],
    );
}

/// A call that decides nothing keeps the declared `mixed`: reading the body
/// may only narrow what the signature promised, never contradict it.
#[test]
fn an_accessor_called_with_an_undecided_name_stays_mixed() {
    assert_assigned_types(ACCESSOR_FIXTURE, &[("$dynamic", "mixed")]);
}

/// A wrapper that forwards its own parameters on keeps the chain intact:
/// the name the outermost call fixed is what the innermost reflection read
/// looks up.
#[test]
fn a_wrapper_forwards_what_its_own_call_site_decided() {
    assert_assigned_types(ACCESSOR_FIXTURE, &[("$forwarded", "Shell")]);
}
