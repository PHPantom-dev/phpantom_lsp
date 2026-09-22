<?php

/**
 * PHP Showcase — Scaffolding
 *
 * Supporting fixtures for the demo files one directory up: models, iterators,
 * generic containers, exceptions, and every other small class a demo needs
 * to exercise a feature. Keep classes here NARROW (2-4 members) — see the
 * design note further down for why.
 */

namespace Demo\Scaffolding;

use Attribute;
use Closure;

// ┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
// ┃  SCAFFOLDING — Supporting definitions below this line.              ┃

// ── PHPUnit coverage metadata scaffolding ───────────────────────────────────

class CoverageCalculator
{
    public function add(int $a, int $b): int { return $a + $b; }

    public function subtract(int $a, int $b): int { return $a - $b; }
}

class CoverageLedger
{
    public function total(): int { return 0; }
}

function coverageTaxRate(): float { return 0.25; }

// ── ReflectionClass<T> instantiation scaffolding ────────────────────────────

class ReflectedWidget
{
    public function label(): string { return 'widget'; }
}

/** Holds the widget privately, so only reflection can read it back. */
class ReflectedHolder
{
    private ?ReflectedWidget $widget;

    public function __construct(?ReflectedWidget $widget = null)
    {
        $this->widget = $widget;
    }
}

// ── Promote Constructor Parameter scaffolding ───────────────────────────────
// Targets both a property and a parameter, because PHP applies an attribute
// written on a promoted parameter to both of them.

#[Attribute(Attribute::TARGET_PROPERTY | Attribute::TARGET_PARAMETER)]
class DemoColumn
{
    public function __construct(
        public string $type,
        public bool $nullable = false,
    ) {}
}

// ── Class-body root member completion scaffolding ───────────────────────────

trait WidgetSlug
{
    /**
     * @param list<string> $parts
     * @return $this
     */
    public function slug(array $parts) { return $this; }

    final public function slugSeparator(): string { return '-'; }
}

class OverridableWidget
{
    public const string ONE_TIME_TTL = '15m';

    public ?string $oneTimeToken = null;
    protected int $override = 0;
    public readonly string $onName;

    public function onChange(callable $callback): static { return $this; }
    protected function onInitialize(): void {}
    final public function onLock(): void {}

    /** @return $this */
    public function withLabel(string $label) { return $this; }
}

// ── @phpstan-require-extends scaffolding ────────────────────────────────────
// RequireExtendsConsumer, the class that actually `use`s the Demo\MocksServiceDemo
// trait, lives in assertions.php instead of here: it needs that trait already
// declared, but completion.php (which declares it) is required *after* this
// file, so declaring the consumer here would be a circular cross-file
// dependency.

class Mock
{
    public function path(): string { return '/tmp/mock'; }
}

class RequireExtendsTestCase
{
    public function makeMock(): Mock { return new Mock(); }
}

// ── SPL wrapper-iterator scaffolding ───────────────────────────────────────────

/**
 * A FilterIterator subclass with a three-argument generic annotation
 * (`<TKey, TValue, TIterator>`). The iterated value type is the middle
 * argument.
 *
 * @extends \FilterIterator<int, \SplFileInfo, \Iterator<int, \SplFileInfo>>
 */
class PhpFileFilter extends \FilterIterator
{
    public function accept(): bool
    {
        return $this->current() instanceof \SplFileInfo;
    }
}

// ── Member-existence narrowing scaffolding ─────────────────────────────────────

// Response object whose extra fields are populated dynamically at runtime, so
// they are not declared statically. `property_exists()` guards prove them.
#[\AllowDynamicProperties]
class ApiResponse
{
    public int $status = 0;
}

// Handler whose `customHook()` is not declared — `method_exists()` guards prove
// it. Deliberately has no `__call`, so only the guard makes the call resolve.
class DynamicHandler
{
    public function run(): void {}
}

// ── Method-Tag Template scaffolding ────────────────────────────────────────────

/**
 * @method TVal get<TVal of mixed>(TVal $default)
 */
class ScaffoldingMethodTagTemplate
{
    /** @return mixed */
    public function __call(string $name, array $args): mixed { return $args[0] ?? null; }
}

// ── Multi-line @method / @property scaffolding ─────────────────────────────────

/**
 * @method FluentCollection<
 *     int,
 *     Pen
 * > fetchAll(string $ink)
 * @property FluentCollection<
 *     int,
 *     Pen
 * > $penHolder
 */
class ScaffoldingMultiLineTags
{
    public function __call(string $name, array $args): mixed
    {
        return collect([new Pen(is_string($args[0] ?? null) ? $args[0] : 'black')]);
    }

    public function __get(string $name): mixed
    {
        return collect([new Pen()]);
    }
}

// ── Template-param @mixin scaffolding ─────────────────────────────────────────
interface ScaffoldingAstNodeInterface {
    public function getStartColumn(): int;
    public function getEndColumn(): int;
}

/**
 * @template-covariant TNode of ScaffoldingAstNodeInterface
 * @mixin TNode
 */
abstract class ScaffoldingAbstractAstNode {
    /** @return string */
    public function getMetric(): string { return ''; }
    /** @return mixed */
    public function __call(string $name, array $arguments): mixed {
        return match ($name) {
            'getStartColumn', 'getEndColumn', 'getParameterCount' => 0,
            default => null,
        };
    }
}

/**
 * @extends ScaffoldingAbstractAstNode<ScaffoldingAstNodeInterface>
 */
class ScaffoldingConcreteAstNode extends ScaffoldingAbstractAstNode {}

// A subclass tightens the template bound to a narrower interface that adds
// `getParameterCount()`.  The `@mixin TNode` still lives on the base
// `ScaffoldingAbstractAstNode` (bound to the looser interface), so resolving
// the tighter member exercises picking the most specific bound in the chain.
interface ScaffoldingCallableAstNodeInterface extends ScaffoldingAstNodeInterface {
    public function getParameterCount(): int;
}

/**
 * @template-covariant TNode of ScaffoldingCallableAstNodeInterface
 * @extends ScaffoldingAbstractAstNode<TNode>
 */
abstract class ScaffoldingAbstractCallableAstNode extends ScaffoldingAbstractAstNode {}

/**
 * @extends ScaffoldingAbstractCallableAstNode<ScaffoldingCallableAstNodeInterface>
 */
class ScaffoldingConcreteCallableAstNode extends ScaffoldingAbstractCallableAstNode {}

// ── Pseudo-type class-name collision scaffolding ─────────────────────────────
// `Number` collides with the `number` PHPDoc pseudo-type but is a real class.
class Number {
    public function __construct(public string $value) {}
    public function scaled(int $factor): Number {
        return new Number((string) ((int) $this->value * $factor));
    }
}

function scaleNumber(Number $n): Number {
    return $n->scaled(10);
}

// ── class-string<T> instantiation scaffolding ───────────────────────────────
class ScaffoldingClassStringFactory {
    /**
     * @template T of object
     * @param class-string<T> $class
     * @return T
     */
    public static function create(string $class): object { return new $class(); }
}

// ── Attribute Completion scaffolding ────────────────────────────────────────
#[\Attribute(\Attribute::TARGET_CLASS)]
class ClassOnlyAttr {}

#[\Attribute(\Attribute::TARGET_METHOD)]
class MethodOnlyAttr {}

#[\Attribute(\Attribute::TARGET_PROPERTY)]
class PropertyOnlyAttr {}

#[\Attribute(\Attribute::TARGET_CLASS | \Attribute::TARGET_METHOD)]
class ClassOrMethodAttr {}

#[\Attribute]
class AnyTargetAttr {}

// ── Constant Type Demo scaffolding ──────────────────────────────────────────
define('CT_ALLOWED_HOSTS', ['localhost', '127.0.0.1']);
const CT_APP_VERSION = '2.0.0';

// StaticPropHolder — used by MixedAccessorDemo
class StaticPropHolder
{
    public static string $shared = 'hello';

    /** @var self */
    public self $holder;
}

// PenDrawer — used by ConditionalReturnDemo (conditionals keyed on the type
// of the argument rather than its value, as Eloquent's `find()` is)
class PenDrawer
{
    /**
     * @param  int|list<int>  $id
     * @return ($id is list<int> ? TypedCollection<int, Pen> : Pen|null)
     */
    public function find(int|array $id): TypedCollection|Pen|null
    {
        return is_array($id) ? new TypedCollection([new Pen()]) : new Pen();
    }

    /**
     * @return ($name is null ? array<string, string> : string)
     */
    public function label(?string $name = null): array|string
    {
        return $name === null ? ['lid' => 'blue'] : 'blue';
    }
}

// TreeMapperImpl — used by ConditionalReturnDemo (literal string conditional)
class TreeMapperImpl
{
    /**
     * @return ($signature is "foo" ? Pen : Marker)
     */
    public function map(string $signature, mixed $source): Pen|Marker
    {
        return new Pen();
    }
}

// ┃  Everything below exists to support the demos above.               ┃
// ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
//
// Keep shared classes NARROW (2-4 members). The whole point of the demos
// is that a human can glance at the completion list and immediately tell
// whether the right type resolved. A 15-member class defeats that because
// the expected item could be buried on page two.
//
// If a demo needs a richer object, create a NEW class in a demo-specific
// section instead of expanding a shared one. Every member you add to a
// shared class leaks into every demo that uses it.
//
// RUNTIME ASSERTIONS: When adding a new demo, add matching assert() calls
// to runDemoAssertions() in ../assertions.php. This catches
// cases where our scaffolding stubs don't actually return what their
// docblocks claim. Run: php -d zend.assertions=1 examples/php/assertions.php
//
// HOISTING PITFALL: Do NOT add __toString() to any class that is
// forward-referenced via `extends` or `implements`. PHP implicitly adds
// `implements \Stringable`, which prevents class hoisting. This is a
// known PHP limitation (php-src#7873), not a bug that will be fixed.
// The same applies to `interface Foo extends \Stringable`.


// ── Untyped Property Inference Scaffolding ──────────────────────────────────

class ScaffoldingUntypedRepo
{
    public function findById(int $id): Pen { return new Pen('found'); }
    public function save(Pen $pen): void {}
}

class ScaffoldingUntypedLogger
{
    public function info(string $msg): void {}
    public function error(string $msg): void {}
}

// ── Demo-Specific Scaffolding ───────────────────────────────────────────────

// ── Body Return Type Inference scaffolding ──────────────────────────────────
/** A holder one hop further down a property path, for PropertyGuardDemo. */
class ScaffoldingHandle
{
    public string|false $name;

    public function __construct(string|false $name = false)
    {
        $this->name = $name;
    }
}

class ScaffoldingUntypedFactory
{
    public function createPen() { return new Pen(); }

    public static function createPenStatic() { return new Pen(); }

    /** @return mixed */
    public function createPenMixed() { return new Pen(); }

    public function createTool(bool $flag)
    {
        if ($flag) {
            return new Pen();
        }
        return new Pencil();
    }

    public function setup() { echo 'initializing'; }

    public function getPencils()
    {
        return [new Pencil()];
    }
}

// ── Inherited Docblock Scaffolding ──────────────────────────────────────────

interface ScaffoldingPenHolderInterface
{
    /** @return list<Pen> */
    public function getPens(): array;

    /** @param list<Pen> $pens */
    public function accept(array $pens): void;
}

class ScaffoldingConcreteHolder implements ScaffoldingPenHolderInterface
{
    public function getPens(): array { return [new Pen()]; }
    public function accept(array $pens): void {}
}

class ScaffoldingPenBox implements ScaffoldingPenHolderInterface
{
    public function getPens(): array { return [new Pen()]; }
    public function accept(array $items): void {}  // renamed param
}

class ScaffoldingBasePenHolder
{
    /** @return list<Pen> */
    public function getPens(): array { return [new Pen()]; }
}

class ScaffoldingChildHolder extends ScaffoldingBasePenHolder
{
    public function getPens(): array { return [new Pen()]; }
}

class ScaffoldingMidHolder extends ScaffoldingBasePenHolder
{
    public function getPens(): array { return [new Pen()]; }
}

class ScaffoldingDeepChild extends ScaffoldingMidHolder
{
    public function getPens(): array { return [new Pen()]; }
}

interface ScaffoldingPayloadSource
{
    /** @return array<string, mixed>|list<mixed>|string */
    public function payload();
}

class ScaffoldingArrayPayload implements ScaffoldingPayloadSource
{
    // No docblock of its own, so the interface's union is inherited — but
    // `: array` rules out the `string` half of it.
    public function payload(): array { return ['ok' => true]; }
}

class ScaffoldingPenDescriber
{
    /** @param \Closure(Pen): string $describe */
    public function describeWith(Closure $describe): string
    {
        return $describe(new Pen());
    }
}

class ScaffoldingAnimalStore
{
    /** @return list<Pen> */
    public function getAnimals(): array { return [new Pen()]; }
}

class ScaffoldingCatStore extends ScaffoldingAnimalStore
{
    /** @return list<Pencil> */
    public function getAnimals(): array { return [new Pencil()]; }
}


class ScaffoldingMotor
{
    public function start(): void {}
}

class ScaffoldingSedan extends ScaffoldingMotor
{
    public function cruise(): void {}
}

class ScaffoldingCoupe extends ScaffoldingMotor
{
    public function race(): string { return 'race'; }
}

// A subclass of a subclass, so a check on the middle class has something
// below it to keep on the way in and rule out on the way out.
class ScaffoldingSportSedan extends ScaffoldingSedan
{
    public function launch(): string { return 'launch'; }
}

abstract class ScaffoldingAbstractShape
{
    abstract public function area(): float;
    abstract protected function perimeter(): float;
}

interface ScaffoldingDrawable
{
    public function draw(string $color, float $opacity = 1.0): void;
}

class ScaffoldingSignatureHelp
{
    /**
     * Paginate a result set.
     *
     * @param int $page Current page number.
     * @param int $limit Max items per page.
     * @return array The paginated slice of results.
     */
    public function paginate(int $page = 1, int $limit = 25): array { return []; }

    /**
     * Search for items matching a query.
     *
     * @param non-empty-string $query The search keywords.
     * @param positive-int $page Page number to return.
     * @param int $perPage Results per page.
     * @return list<array{id: int, title: string}> Matching items.
     */
    public function search(string $query, int $page = 1, int $perPage = 20): array { return []; }
}

class ScaffoldingDeprecation
{
    /**
     * @deprecated Use sendAsync() instead.
     * @see ScaffoldingDeprecation::sendAsync()
     */
    public function sendLegacy(): void {}

    /**
     * @deprecated
     * @see ScaffoldingDeprecation::sendAsync()
     */
    public function oldProcess(): void {}

    public function sendAsync(): void {}

    /**
     * @deprecated Use isDebug() instead.
     * @see ScaffoldingDeprecation::sendAsync()
     */
    public bool $debugMode = false;

    /**
     * @deprecated Use MAX_LIMIT instead.
     * @see ScaffoldingDeprecation::MAX_LIMIT
     */
    const OLD_LIMIT = 100;

    const MAX_LIMIT = 500;

    // JetBrains stubs style
    #[\JetBrains\PhpStorm\Deprecated(reason: "Use modernMethod() instead", since: "8.1")]
    public function attrDeprecatedMethod(): void {}

    // Native PHP 8.4 style (\Deprecated)
    #[\Deprecated(message: "Use nativeModern() instead", since: "8.4")]
    public function nativeDeprecatedMethod(): void {}

    #[\Deprecated]
    public function attrBareMethod(): void {}

    #[\Deprecated("Use positionalModern() instead")]
    public function attrPositionalMethod(): void {}

    #[\JetBrains\PhpStorm\Deprecated(reason: "The property is deprecated", since: "8.4")]
    public string $attrProp = '';

    #[\Deprecated(reason: "Use NEW_SETTING instead")]
    const ATTR_OLD = 0;

    /**
     * @deprecated Docblock message wins.
     */
    #[\Deprecated(reason: "Attribute message loses")]
    public function bothDocAndAttr(): void {}

    #[\Deprecated(replacement: "%class%->setTimezone(%parametersList%)", since: "5.5")]
    public function legacySetTimezone(string $tz): void {}

    public function setTimezone(string $tz): void {}
}

/**
 * @property mixed $locale
 * @property mixed $timezone
 * @property mixed $retries
 */
trait ScaffoldingMixedDefaults {}

class ScaffoldingAppConfig
{
    use ScaffoldingMixedDefaults;

    public string $locale = 'en';
    public string $timezone = 'UTC';
    public int $retries = 3;
}

/**
 * @property string $gorilla
 * @method bool hyena(string $x)
 */
class Zoo extends ZooBase implements ZooContract
{
    use ZooTraitA;
    use ZooTraitB;

    public string $baboon = '';
    protected string $keeper = 'hidden';      // trip wire — must NOT appear on $zoo->
    private string $ceo = 'invisible';        // trip wire — must NOT appear on $zoo->

    public function aardvark(): void {}
    private function nocturnal(): void {}     // trip wire — must NOT appear on $zoo->

    public function __construct(
        public int $buffalo = 0,
    ) {
        parent::__construct();
    }

    public function __get(string $name): mixed
    {
        return match ($name) {
            'gorilla' => 'gorilla-value',   // @property string $gorilla
            'iguana'  => 'iguana-value',     // @property-read string $iguana (ZooContract)
            default   => null,
        };
    }

    public function __call(string $name, array $args): mixed
    {
        return match ($name) {
            'hyena'  => true,               // @method bool hyena(string $x)
            'jaguar' => 'jaguar-value',     // @method string jaguar() (ZooContract)
            default  => null,
        };
    }
}

abstract class ZooBase
{
    public function __construct(
        public readonly string $cheetah = '',
    ) {}

    public function falcon(): string { return ''; }
}

trait ZooTraitA
{
    public function dingo(): void {}
}

trait ZooTraitB
{
    public function elephant(string $value): string { return $value; }
}

trait MakesPageAssertions
{
    /** Fluent assertion with no declared return type — body returns $this. */
    public function assertSee(string $value)
    {
        $seen = $value !== '';
        return $this;
    }
}

class TestablePage
{
    use MakesPageAssertions;

    public int $status = 200;

    public function refresh(): static
    {
        return $this;
    }
}

/**
 * @property-read string $iguana
 * @method string jaguar()
 */
interface ZooContract {}

class ScaffoldingChainingDemo
{
    public Brush $brush;
    public Canvas $canvas;

    public function __construct()
    {
        $this->brush = new Brush();
        $this->canvas = new Canvas();
    }
}

class ScaffoldingExpressionType
{
    public ?Response $backup;
    public Response $primary;

    public function __construct()
    {
        $this->backup = new Response(500, 'Backup');
        $this->primary = new Response(200, 'OK');
    }
}

// ScaffoldingGenericShape — used by GenericShapeDemo
/**
 * @template T
 */
class ScaffoldingGenericShapeBase
{
    /** @return array{data: T, items: list<T>} */
    public function getResult(): array { return []; }
}

/**
 * @extends ScaffoldingGenericShapeBase<Gift>
 */
class ScaffoldingGenericShape extends ScaffoldingGenericShapeBase {}

// ScaffoldingTypedBox — used by TypeErrorDemo
/**
 * @template T
 */
class ScaffoldingTypedBox
{
    /** @param T $value */
    public function __construct(public mixed $value)
    {
    }
}

class ScaffoldingCollectionForeach
{
    public PenCollection $pens;

    public function allPens(): PenCollection
    {
        return new PenCollection();
    }
}

class ScaffoldingGenericContext
{
    /** @var Box<Gift> */
    public $chest;

    public function __construct() { $this->chest = new Box(new Gift()); }

    /** @return TypedCollection<int, Gift> */
    public function display(): TypedCollection { return new TypedCollection([new Gift()]); }
}

class ScaffoldingIteration
{
    /** @var list<Pen> */
    public array $batch;

    /** @return list<Pen> */
    public function allPens(): array { return []; }

    /** @return array<Pen, Pencil> */
    public function crossRef(): array { return []; }

    /**
     * Shorthand `Pen[]` names only the value type, so the keys stay open.
     *
     * @return Pen[]
     */
    public function openKeyed(): array { return ['blue' => new Pen('blue'), 7 => new Pen('red')]; }
}

class ScaffoldingArrayFunc
{
    /** @var list<Pen> */
    public array $members;

    /** @return list<Pen> */
    public function roster(): array { return []; }

    /** @return list<array{Pen, string}> */
    public function pairs(): array { return [[new Pen('blue'), 'ink'], [new Pen('red'), 'ink']]; }

    /** @return array<string, Pen> */
    public function byName(): array { return ['blue' => new Pen('blue'), 'red' => new Pen('red')]; }

    /** @return list<string> */
    public function labels(): array { return ['ink', 'gel']; }

    /** @return array<string, string|null> */
    public function optionalLabels(): array { return ['ink' => 'ink', 'gel' => null]; }

    /** @return array<string|int, string> */
    public function mixedKeys(): array { return ['ink' => 'gel', 7 => 'nib']; }

    /** @return list<Pen|Pencil> */
    public function mixedWriters(): array { return [new Pen('blue'), new Pencil(), new Pen('red')]; }

    /** @return list<int> */
    public function weights(): array { return [2, 3, 4]; }

    /** @return list<Marker> */
    public function markers(): array { return [new Marker('wide'), new Marker('slim')]; }
}

class ScaffoldingException
{
    protected function lookup(int $id): ?array { return null; }
    protected function riskyOperation(): void {}

    /** @throws AuthorizationException */
    protected function throwsException(): void { throw new AuthorizationException('forbidden'); }
}

class ScaffoldingClosureParamInference
{
    /** @var FluentCollection<int, Pen> */
    public FluentCollection $items;

    public function __construct() { $this->items = new FluentCollection([new Pen('red'), new Pen('blue')]); }
}

class ScaffoldingEventBus
{
    /**
     * @template T
     * @param Closure(T): void $callback
     * @return T
     */
    public function listen(Closure $callback): mixed
    {
        $params = (new \ReflectionFunction($callback))->getParameters();
        $type = $params[0]->getType();
        $class = $type instanceof \ReflectionNamedType ? $type->getName() : 'stdClass';
        return (new \ReflectionClass($class))->newInstanceWithoutConstructor();
    }
}

class ScaffoldingBatchProcessor
{
    /**
     * @template T
     * @param Closure(int, T): void $handler
     * @return T
     */
    public function process(Closure $handler): mixed
    {
        $params = (new \ReflectionFunction($handler))->getParameters();
        $type = $params[1]->getType();
        $class = $type instanceof \ReflectionNamedType ? $type->getName() : 'stdClass';
        return (new \ReflectionClass($class))->newInstanceWithoutConstructor();
    }
}

class ScaffoldingTemplateCallableHolder
{
    /** @var array<int, Pen> */
    public array $tools = [];
}

/**
 * @template TValue
 */
class ScaffoldingReducible
{
    /**
     * @template TReduceInitial
     * @template TReduceReturnType
     *
     * @param callable(TReduceInitial|TReduceReturnType, TValue): TReduceReturnType $callback
     * @param TReduceInitial $initial
     * @return TReduceReturnType
     */
    public function reduce(callable $callback, mixed $initial): mixed
    {
        return $initial;
    }
}

/**
 * Two parameters binding the same `@template T`. `T` is the union of
 * every binding site, so each argument is checked against what all of
 * them have in common rather than against whichever one resolved last.
 */
class ScaffoldingToolbox
{
    /**
     * @template T
     * @param T[] $first
     * @param T[] $second
     * @return list<T>
     */
    public function combine(array $first, array $second): array
    {
        return array_values(array_merge($first, $second));
    }
}

/**
 * Cache-like helper whose `@template T` is bound from the callback's
 * return type. Mirrors Laravel's `Cache::remember()` so an unannotated
 * `fn() => new Pen()` callback resolves the result to `Pen`.
 */
class ScaffoldingClosureCache
{
    /**
     * @template T
     *
     * @param \Closure(): T $callback
     * @return T
     */
    public function remember(string $key, \Closure $callback): mixed
    {
        return $callback();
    }
}

class ScaffoldingPipeline
{
    /**
     * @param callable($this, mixed): $this $callback
     * @return $this
     */
    public function when(bool $condition, callable $callback): static { return $this; }

    /**
     * @param callable($this): void $callback
     * @return $this
     */
    public function tap(callable $callback): static { return $this; }

    public function send(mixed $data): static { return $this; }
    public function through(array $pipes): static { return $this; }
}

// ScaffoldingClosureThisRoute / ScaffoldingClosureThisRouter — used by
// ParamClosureThisDemo. Each method that declares @param-closure-this also
// binds the callback with Closure::call() so the runtime matches the tag.
class ScaffoldingClosureThisRoute
{
    public function middleware(string $m): self { return $this; }
    public function prefix(string $p): self { return $this; }

    /**
     * @param-closure-this ScaffoldingClosureThisResource $callback
     */
    public function resource(string $name, \Closure $callback): void
    {
        $callback->call(new ScaffoldingClosureThisResource());
    }
}

class ScaffoldingClosureThisResource
{
    public function only(string $action): self { return $this; }
}

// A subclass the tag on `apiGroup()` cannot name, standing in for the base
// class a Pest suite's `pest()->extends(…)` binds.
class ScaffoldingClosureThisApiRoute extends ScaffoldingClosureThisRoute
{
    public function version(string $v): self { return $this; }
}

class ScaffoldingClosureThisRouter
{
    public function getDefaultDriver(): string { return ''; }

    /**
     * @param-closure-this ScaffoldingClosureThisRoute $callback
     */
    public function group(\Closure $callback): void
    {
        $callback->call(new ScaffoldingClosureThisRoute());
    }

    /**
     * Declares the base class but binds a subclass, the way Pest's `test()`
     * declares `PHPUnit\Framework\TestCase` and binds whatever
     * `pest()->extends(…)` names.  A closure body says which one it got by
     * asserting it.
     *
     * @param-closure-this ScaffoldingClosureThisRoute $callback
     */
    public function apiGroup(\Closure $callback): void
    {
        $callback->call(new ScaffoldingClosureThisApiRoute());
    }

    /**
     * @param string $driver
     * @param \Closure $callback
     * @param-closure-this $this $callback
     * @return $this
     */
    public function extend(string $driver, \Closure $callback): self
    {
        $callback->call($this);
        return $this;
    }
}

// ScaffoldingMacroTarget — a minimal Macroable-style class used by
// ParamClosureThisDemo. `macro()` stores the closure and `__call` binds it
// with this class as scope (like Laravel's Macroable / Carbon), so
// `self::`/`static::` inside a registered closure refer to this class.
class ScaffoldingMacroTarget
{
    /** @var array<string, callable> */
    private static array $macros = [];

    public static function make(): static { return new static(); }

    public function render(): string { return 'rendered'; }

    /**
     * @param-closure-this static $macro
     */
    public static function macro(string $name, callable $macro): void
    {
        static::$macros[$name] = $macro;
    }

    /** @param array<int, mixed> $args */
    public function __call(string $name, array $args): mixed
    {
        $macro = static::$macros[$name];
        if ($macro instanceof \Closure) {
            $macro = $macro->bindTo($this, static::class);
        }
        return $macro(...$args);
    }
}

class ScaffoldingFirstClassCallable
{
    public function dispatch(): Pen
    {
        return new Pen();
    }
}

class ScaffoldingArrayAccess
{
    /** @return Pen[] */
    public function fetchAll(): array { return []; }
}

class ScaffoldingPenArrayAccess implements \ArrayAccess
{
    /** @var Pen[] */
    private array $items = [];

    public function offsetExists(mixed $offset): bool { return isset($this->items[$offset]); }
    public function offsetGet(mixed $offset): Pen { return $this->items[$offset] ?? new Pen(); }
    public function offsetSet(mixed $offset, mixed $value): void { $this->items[$offset] = $value; }
    public function offsetUnset(mixed $offset): void { unset($this->items[$offset]); }
}

/**
 * @template T of Pen
 * @implements \ArrayAccess<int, T>
 */
class ScaffoldingGenericArrayAccess implements \ArrayAccess
{
    /** @param T[] $items */
    public function __construct(private array $items) {}

    public function offsetExists(mixed $offset): bool { return isset($this->items[$offset]); }
    /** @return T */
    public function offsetGet(mixed $offset): mixed { return $this->items[$offset]; }
    public function offsetSet(mixed $offset, mixed $value): void { $this->items[$offset] = $value; }
    public function offsetUnset(mixed $offset): void { unset($this->items[$offset]); }
}

/**
 * Named bare (no type arguments) by `BareGenericSubjectDemo`, so `TPen`
 * has to fall back to the bound its `@template` declares.
 *
 * @template TPen of Pen
 */
class ScaffoldingBoundedPenCollection
{
    /** @param list<TPen> $items */
    public function __construct(private array $items = []) {}

    /** @return TPen|null */
    public function first() { return $this->items[0] ?? null; }
}

/**
 * The same shape with no declared bound, where the widest guarantee is
 * `mixed`.
 *
 * @template TValue
 */
class ScaffoldingUnboundedBox
{
    /** @return TValue */
    public function unwrap() { return null; }
}

class ScaffoldingFormatter
{
    public function __invoke(): Pen { return new Pen(); }
}

class ScaffoldingPenFactory
{
    public function __invoke(): Pen { return new Pen(); }
}

class ScaffoldingPenFetcher
{
    /** @return Pen[] */
    public function __invoke(): array { return []; }
}


// ── AST Node (template bounds demo) ────────────────────────────────────────

class AstNode
{
    /** @return AstNode|null */
    public function getParent(): ?AstNode { return null; }

    /** @return AstNode[] */
    public function getChildren(): array { return []; }

    public function getType(): string { return ''; }
}

// ── ObjectMapper (method-level @template demo) ──────────────────────────────

class ObjectMapper
{
    /**
     * @template T
     * @param T $item
     * @return TypedCollection<int, T>
     */
    public function wrap(object $item): TypedCollection
    {
        /** @var TypedCollection<int, T> */
        return new TypedCollection([$item]);
    }

    /**
     * @template T
     * @param T $item
     * @return T
     */
    public function identity(mixed $item): mixed
    {
        return $item;
    }

    /**
     * An identity generic whose *constraint* is an array type: `T` is
     * never bound from an argument at a call site, only from its
     * declared bound.  Inside the method body, `$pens` (typed `T`)
     * must resolve to that bound so array-element functions like
     * `end()` can find the element's class.
     *
     * @template T of Pen[]
     * @param T $pens
     * @return T
     */
    public function peekLast(array $pens): array
    {
        end($pens)->write();      // T resolves to its bound (Pen[]) inside the body
        return $pens;
    }
}


// ── ScaffoldingMutableBox (@psalm-this-out / @phpstan-self-out demo) ─────────

/**
 * A box whose contents can be swapped for a value of another type.
 *
 * `replace()` carries `@psalm-this-out self<U>` / `@phpstan-self-out
 * self<U>`: the call re-binds the receiver's own template argument, so
 * after `$box->replace(new Pencil())` the box reads as
 * `ScaffoldingMutableBox<Pencil>` however it was annotated before.
 *
 * @template T
 */
class ScaffoldingMutableBox
{
    /** @var T */
    public mixed $value;

    /** @param T $value */
    public function __construct(mixed $value)
    {
        $this->value = $value;
    }

    /**
     * @template U
     * @param U $value
     * @psalm-this-out self<U>
     * @phpstan-self-out self<U>
     */
    public function replace(mixed $value): void
    {
        $this->value = $value;
    }
}


// ─── Interfaces ─────────────────────────────────────────────────────────────

/**
 * @method string render()
 * @property-read string $output
 */
interface Renderable
{
    public function format(string $template): string;
}

// ─── Traits ─────────────────────────────────────────────────────────────────

trait JsonSerializer {
    public function serialize(): string { return '{}'; }
    public function toJson(): string { return $this->serialize(); }
}

trait XmlSerializer {
    public function serialize(): string { return '<xml/>'; }
    public function toXml(): string { return $this->serialize(); }
}

trait HasTimestamps
{
    protected ?string $createdAt = null;

    public function getCreatedAt(): ?string
    {
        return $this->createdAt;
    }

    public function setCreatedAt(string $date): static
    {
        $this->createdAt = $date;
        return $this;
    }
}

trait HasSlug
{
    public function generateSlug(string $value): string
    {
        return strtolower(str_replace(' ', '-', $value));
    }
}

/**
 * @template TFactory
 */
trait HasFactory
{
    /** @return TFactory */
    public static function factory() {}
}

/**
 * @template TKey
 * @template TValue
 */
trait Indexable
{
    /** @return TValue */
    public function get() {}

    /** @return TKey */
    public function key() {}
}

// ─── Enums ──────────────────────────────────────────────────────────────────

enum Status: string
{
    case Active = 'active';
    case Inactive = 'inactive';
    case Pending = 'pending';

    public function label(): string
    {
        return match ($this) {
            self::Active   => 'Active',
            self::Inactive => 'Inactive',
            self::Pending  => 'Pending',
        };
    }

    public function isActive(): bool
    {
        return $this === self::Active;
    }

    /** Returns the raw backing value of the Active case. */
    public static function defaultValue(): string
    {
        return self::Active->value;  // self::CaseName->value resolved
    }

    /** A `self` parameter binds to the enum that declares the method. */
    public function canChangeTo(self $next): bool
    {
        return $this !== $next;
    }
}

enum Priority: int
{
    case Low = 1;
    case Medium = 2;
    case High = 3;
}

enum Mode
{
    case Automatic;
    case Manual;
}

// ─── Builder (@mixin target) ────────────────────────────────────────────────

class Builder
{
    /** @return static */
    public static function query(): self
    {
        return new static();
    }

    public function where(string $col, mixed $val): self
    {
        return $this;
    }
}

// ─── Abstract Base Class ────────────────────────────────────────────────────

/**
 * @property string $magicName
 * @method static static create(array $attributes)
 * @mixin Builder
 */
abstract class Model
{
    protected int $id;

    public const string CONNECTION = 'default';
    protected const int PER_PAGE = 15;

    public function __construct(
        protected string $name = '',
        public readonly string $uuid = '',
    ) {
        $this->id = rand(1, 99999);
    }

    public function getId(): int
    {
        return $this->id;
    }

    public function getName(): string
    {
        return $this->name;
    }

    /** @return static */
    public function setName(string $name): static
    {
        $this->name = $name;
        return $this;
    }

    /** @deprecated */
    public static function find(int $id): ?static
    {
        return null;
    }

    /** @return static */
    public static function make(string $name = ''): static
    {
        return new static($name, '');
    }

    abstract public function toArray(): array;
}


// ─── Concrete Classes ───────────────────────────────────────────────────────

/**
 * @property string $displayName
 * @property-read bool $isAdmin
 * @method bool hasPermission(string $permission)
 */
class User extends Model implements Renderable
{
    use HasTimestamps;
    use HasSlug;

    public string $email;
    protected Status $status;
    private array $roles = [];
    public static string $defaultRole = 'user';
    public const string TYPE_ADMIN = 'admin';
    public const string TYPE_USER = 'user';

    public function __construct(
        string $name,
        string $email,
        private readonly string $password = '',
        public int $age = 0,
    ) {
        parent::__construct($name);
        $this->email = $email;
        $this->status = Status::Active;
    }

    public function getEmail(): string
    {
        return $this->email;
    }

    public function getStatus(): Status
    {
        return $this->status;
    }

    public function setStatus(Status $status): self
    {
        $this->status = $status;
        return $this;
    }

    public function addRoles(string ...$roles): void
    {
        $this->roles = array_merge($this->roles, $roles);
    }

    public function getRoles(): array
    {
        return $this->roles;
    }

    public function getProfile(): UserProfile
    {
        return new UserProfile($this);
    }

    public function toArray(): array
    {
        return [
            'id' => $this->getId(),
            'name' => $this->getName(),
            'email' => $this->email,
            'status' => $this->status->value,
        ];
    }

    public function format(string $template): string
    {
        return str_replace('{name}', $this->getName(), $template);
    }

    public static function findByEmail(string $email): ?self
    {
        return null;
    }

    protected function hashPassword(string $raw): string
    {
        return password_hash($raw, PASSWORD_BCRYPT);
    }

    private function secretInternalMethod(): void {}
}

class UserProfile
{
    public string $bio = '';

    public function __construct(private User $user) {}

    public function getUser(): User
    {
        return $this->user;
    }

    public function setBio(string $bio): self
    {
        $this->bio = $bio;
        return $this;
    }

    public function getDisplayName(): string
    {
        return $this->user->getName() . ' (' . $this->user->getEmail() . ')';
    }
}

final class AdminUser extends User
{
    /** @var string[] */
    private array $permissions = [];

    public function __construct(string $name, string $email)
    {
        parent::__construct($name, $email);
    }

    public function toArray(): array
    {
        $base = parent::toArray();
        $base['connection'] = parent::CONNECTION;
        $base['permissions'] = $this->permissions;
        return $base;
    }

    public function grantPermission(string $permission): void
    {
        $this->permissions[] = $permission;
    }
}

class Response
{
    public function __construct(
        private string|int $statusCode,
        private string|array|null $body = null,
    ) {}

    public function getStatusCode(): string|int
    {
        return $this->statusCode;
    }

    public function getBody(): string|array|null
    {
        return $this->body;
    }

    public function isSuccess(): bool
    {
        return $this->statusCode >= 200 && $this->statusCode < 300;
    }
}

// ─── Generics (@template / @extends) ───────────────────────────────────────

/**
 * @template T
 */
class Repository
{
    /** @var T|null */
    protected $cached = null;

    /** @return T */
    public function find(int $id)
    {
        return $this->cached;
    }

    /** @return T|null */
    public function findOrNull(int $id)
    {
        return $this->cached;
    }

    /** @return T */
    public function first()
    {
        return $this->cached;
    }
}

/** @extends Repository<Pen> */
class PenRepository extends Repository {}

class CachingPenRepository extends PenRepository
{
    public function clearCache(): void {}
}

// ─── Constant Tables Read Through a Type Operator ───────────────────────────

/** The grades a gate accepts; a strict `in_array` against it names them. */
const GRADES = ['a', 'b', 'c'];

const TOOL_DEFAULTS = ['width' => 2, 'ink' => 'black'];

/**
 * @template T of key-of<TOOL_DEFAULTS>
 * @param T $setting
 * @return TOOL_DEFAULTS[T]
 */
function scaffoldingToolDefault(string $setting): int|string
{
    return TOOL_DEFAULTS[$setting];
}

/**
 * @template T of key-of<TOOL_DEFAULTS>
 * @param T $setting
 * @return TOOL_DEFAULTS[T]
 */
function scaffoldingDefaultToolSetting(string $setting = 'ink'): int|string
{
    return TOOL_DEFAULTS[$setting];
}

/** @param key-of<TOOL_DEFAULTS> $setting */
function scaffoldingToolSettingName(string $setting): string
{
    return $setting;
}

/** @return value-of<TOOL_DEFAULTS> */
function scaffoldingAnyToolDefault(): int|string
{
    return TOOL_DEFAULTS['width'];
}

/**
 * @param key-of<TOOL_DEFAULTS> $setting
 * @return value-of<TOOL_DEFAULTS>
 */
function scaffoldingToolDefaultFor(string $setting): int|string
{
    // Try: hover `$setting` — 'width'|'ink', the keys the table has.
    return TOOL_DEFAULTS[$setting];
}

class ScaffoldingLimits
{
    const LIMITS = ['retries' => 3, 'label' => 'off'];

    /**
     * @template T of key-of<self::LIMITS>
     * @param T $key
     * @return self::LIMITS[T]
     */
    public static function lookUp(string $key): int|string
    {
        return self::LIMITS[$key];
    }

    /** @return key-of<self::LIMITS> */
    public static function anyLimitName(): string
    {
        return 'retries';
    }
}

// ─── @implements Generic Resolution ─────────────────────────────────────────

/**
 * @template TEntity
 */
interface Storage
{
    /** @return TEntity */
    public function find(int $id);

    /** @return TEntity[] */
    public function findAll();
}

/** @implements Storage<Pen> */
class PenStorage implements Storage
{
    public function find(int $id) { return new Pen(); }
    public function findAll() { return [new Pen()]; }
}

/** @template-implements Storage<Pen> */
class PenCatalog implements Storage
{
    public function find(int $id) { return new Pen(); }
    public function findAll() { return [new Pen()]; }
}

/**
 * @template T
 * @implements \IteratorAggregate<int, T>
 */
class IterableCollection implements \IteratorAggregate
{
    /** @return \ArrayIterator<int, T> */
    public function getIterator(): \ArrayIterator { return new \ArrayIterator([]); }
}

/** @extends IterableCollection<Pen> */
class ItemIterableCollection extends IterableCollection {}

/**
 * @template TKey of array-key
 * @template-covariant TValue
 */
class TypedCollection
{
    /** @var array<TKey, TValue> */
    protected array $items;

    /** @param array<TKey, TValue> $items */
    public function __construct(array $items = []) { $this->items = $items; }

    /**
     * Static factory carrying a method-level `@template` into the
     * class-level `TValue` of the collection it returns.
     *
     * @template TMakeValue
     *
     * @param  array<array-key, TMakeValue>  $items
     * @return static<array-key, TMakeValue>
     */
    public static function make(array $items = []): static { return new static($items); }

    /** @return TValue */
    public function first() { return reset($this->items); }

    /** @return ?TValue */
    public function last() { return end($this->items) ?: null; }

    /** @return static */
    public function filter(callable $fn): static { return $this; }

    /** @return int */
    public function count(): int { return count($this->items); }

    /** @return array<TKey, TValue> */
    public function all(): array { return $this->items; }

    /**
     * Conditional @return whose branches are `static<...>` generics
     * (Laravel's `Collection::chunk()` shape). Chunking produces a
     * collection of collections, so the wrapped generic arguments must
     * survive the conditional to keep the inner element type resolvable.
     *
     * @return ($preserveKeys is true
     *     ? static<int, static>
     *     : static<int, static<int, TValue>>)
     */
    public function chunk(int $size, bool $preserveKeys = true): static
    {
        $chunks = array_chunk($this->items, $size, $preserveKeys);
        return new static(array_map(fn(array $c): static => new static($c), $chunks));
    }
}

/** @extends TypedCollection<int, Pen> */
class PenCollection extends TypedCollection
{
    public function thickOnly(): self
    {
        return $this;
    }
}

/** @phpstan-extends TypedCollection<string, Response> */
class ResponseCollection extends TypedCollection {}

// ─── Container (conditional return types) ───────────────────────────────────

/**
 * Throws unless the value is truthy, and says so in its return type: a
 * falsy `$condition` selects `never`, so nothing that reached the caller
 * afterwards could have been falsy.
 *
 * @template TValue
 * @param TValue $condition
 * @return ($condition is false ? never : ($condition is non-empty-mixed ? TValue : never))
 */
function throwUnless($condition, string $message = 'unexpected falsy value')
{
    if (!$condition) {
        throw new \RuntimeException($message);
    }

    return $condition;
}


class Container
{
    /** @var array<string, object> */
    private array $bindings = [];

    /**
     * @template TClass
     * @param string|null $abstract
     * @return ($abstract is class-string<TClass> ? TClass : mixed)
     */
    public function make(?string $abstract = null): mixed
    {
        if ($abstract === null) {
            return $this;
        }
        return $this->bindings[$abstract] ?? new $abstract();
    }

    public function bind(string $abstract, object $obj): void
    {
        $this->bindings[$abstract] = $obj;
    }

    public function getStatus(): int
    {
        return 200;
    }
}

// ─── Method-Level @template Classes ─────────────────────────────────────────

class ServiceLocator
{
    /**
     * @template T
     * @param class-string<T> $id
     * @return T
     */
    public function get(string $id): object
    {
        return new $id();
    }

    /**
     * @template T
     * @param class-string<T> ...$ids
     * @return T
     */
    public function getAny(string ...$ids): object
    {
        return new ($ids[0])();
    }

    /**
     * @template T
     * @param class-string<T> $id
     * @return Box<T>
     */
    public function wrap(string $id): object
    {
        return new Box(new $id());
    }

    /**
     * @template T of Pen
     * @param class-string<T> $id
     * @return T[]
     */
    public function getAll(string $id): array
    {
        return [new $id()];
    }

    /**
     * @template T of object
     * @param array<class-string<T>|T|array<T>> ...$args
     * @return T
     */
    public function build(mixed ...$args): object
    {
        $first = $args[0];

        return is_string($first) ? new $first() : $first;
    }
}

class Factory
{
    /**
     * @template T
     * @param class-string<T> $class
     * @return T
     */
    public static function create(string $class): object
    {
        return new $class();
    }
}

// ─── Generic Wrapper ────────────────────────────────────────────────────────

/**
 * @template T
 */
class Box
{
    /** @var T */
    public $value;

    /** @param T $value */
    public function __construct(mixed $value = null) { $this->value = $value; }

    /** @return T */
    public function unwrap() { return $this->value; }
}

class Gift
{
    public function open(): string { return 'surprise!'; }
    public function getTag(): string { return 'birthday'; }
}

// ─── Narrowing Demo Support Classes ─────────────────────────────────────────

class Rock
{
    public function crush(): string { return 'smash!'; }
    public function weigh(): float { return 5.0; }
}

class Banana
{
    public function peel(): string { return 'yum!'; }
    public function weigh(): float { return 0.2; }
}

/** The printed tag a specimen on the shelf gets. */
class SpecimenLabel
{
    public function __construct(private readonly string $text) {}

    public function render(): string { return $this->text; }
}

final class TextTag
{
    public string $tag = 'granite';

    public function letters(): int { return strlen($this->tag); }
}

final class NumberTag
{
    public int $tag = 42;

    public function digits(): int { return strlen((string) $this->tag); }
}

final class Weighed
{
    /** @var 'weighed' */
    public string $state = 'weighed';

    public float $grams = 5.0;
}

final class Unweighed
{
    /** @var 'unweighed' */
    public string $state = 'unweighed';

    public function reason(): string { return 'no scale'; }
}

interface Labelled
{
    public function label(): string;
}

class LabelledRock extends Rock implements Labelled
{
    public function label(): string { return 'granite'; }
}

class SpecimenHolder
{
    public Rock|Banana $item;

    public function __construct()
    {
        $this->item = new Rock();
    }

    public function maybe(): Rock|Banana|null
    {
        return null;
    }

    /** Looks a specimen up by name; `null` when the shelf has none. */
    public function lookUp(string $name): Rock|Banana|null
    {
        return $name === 'rock' ? new Rock() : null;
    }

    /** Prints a label for a specimen that is definitely on the shelf. */
    public function labelFor(Rock|Banana $specimen): SpecimenLabel
    {
        return new SpecimenLabel($specimen->weigh() . 'kg');
    }

    /** Restocks the shelf, so what `lookUp()` answers afterwards may differ. */
    public function restock(): void
    {
        $this->item = new Banana();
    }

    /** @phpstan-pure */
    public function shelfLabel(): string
    {
        return 'specimens';
    }

    /** Counts what is on the shelf without changing it, and says so only by
     * handing a value back. */
    public function shelfCount(): int
    {
        return 1;
    }

    /**
     * Turns the shelf, which changes what `lookUp()` answers. The `bool`
     * return says nothing about that, so the tag has to.
     *
     * @impure
     */
    public function rotate(): bool
    {
        $this->item = new Banana();

        return true;
    }
}

// ─── Ambiguous Variable Support Classes ─────────────────────────────────────

class Lamp
{
    public function dim(): void {}
    public function turnOff(): void {}
}

class Faucet
{
    public function drip(): void {}
    public function turnOff(): void {}
}

// ─── Intersection Demo Support Classes ──────────────────────────────────────

interface Printable
{
    public function print(): void;
}

class Envelope
{
    public function seal(): void {}
}

class SealedEnvelope extends Envelope implements Printable
{
    public function print(): void {}
}

function openSealedEnvelope(): ?SealedEnvelope
{
    return new SealedEnvelope();
}

// ─── Shared Narrow Classes ──────────────────────────────────────────────────
// These are small, purpose-built classes for demos. Keep them narrow (2-4
// members each). If a demo needs a richer object, create a new class in a
// demo-specific section below instead of expanding these.

class Pen
{
    public function __construct(public string $ink = 'black') {}
    public function write(): string { return ''; }
    public function color(): string { return $this->ink; }
    public function label(): string { return 'pen'; }
    /** @return static */
    public function rename(string $name): static { return $this; }
    /** @return static */
    public static function make(string $color = 'black'): static { return new static($color); }
    private function refill(): void {}            // trip wire — must NOT appear on external $pen->
}

class Pencil
{
    public function sketch(): string { return ''; }
    public function sharpen(): void {}
    public function label(): string { return 'pencil'; }
}

class Marker extends Pen
{
    public function highlight(): void {}
}

// ─── Loop Fold / Pre-Validation Support Classes ─────────────────────────────
// A tally that folds together, and the branch ends a loop folds tallies out
// of. The pre-validation classes below are the same shape a parser walks: a
// base node, one subtype that carries the extra members, and a holder of a
// list of them.

class InkTally
{
    public function __construct(public int $strokes = 0) {}

    public function mergeWith(InkTally $other): InkTally
    {
        return new InkTally($this->strokes + $other->strokes);
    }

    public function total(): int { return $this->strokes; }
}

class DrawingStep
{
    public function __construct(private readonly int $strokes = 1) {}

    public function tally(): InkTally { return new InkTally($this->strokes); }
}

class SketchNode
{
    public function kind(): string { return 'node'; }
}

class LabelledSketchNode extends SketchNode
{
    public function __construct(public string $caption = 'untitled') {}

    public function kind(): string { return 'labelled'; }

    public function caption(): string { return $this->caption; }
}

class SketchGroup
{
    /** @param list<SketchNode> $nodes */
    public function __construct(public array $nodes = []) {}

    /** @param callable(SketchNode): void $visit */
    public function walk(callable $visit): void
    {
        foreach ($this->nodes as $node) {
            $visit($node);
        }
    }
}

// ─── Chaining Demo Support Classes ──────────────────────────────────────────

class Brush
{
    public function setSize(string $size): static { return $this; }
    public function setStyle(string $style): static { return $this; }
    public function stroke(): string { return ''; }
    public function getCanvas(): Canvas { return new Canvas(); }
    protected function calibrate(): void {}       // trip wire — must NOT appear on $studio->brush->
    public static function find(int $id): ?static { return null; }
}

class Canvas
{
    public Easel $easel;

    public function __construct() { $this->easel = new Easel(); }
    public function getBrush(): Brush { return new Brush(); }
    public function title(): string { return ''; }
}

class Easel
{
    public string $material = 'wood';
    public function height(): string { return '150cm'; }
}

// ─── Expression Type Support Classes ────────────────────────────────────────

class ElasticProductReviewIndexService
{
    public function index(array $markets = []): void {}
    public function reindex(): void {}
}

class ElasticBrandIndexService
{
    public function index(array $markets = []): void {}
    public function bulkDelete(array $ids): void {}
}

// ─── Param Override Support Classes ─────────────────────────────────────────

class Ingredient
{
    public function __construct(
        public string $name = '',
        public float $quantity = 0.0,
    ) {}

    public function format(): string
    {
        return "{$this->quantity}x {$this->name}";
    }
}

class Recipe
{
    /**
     * @param list<Ingredient> $ingredients
     */
    public function __construct(
        public string $name = '',
        public array $ingredients = [],
    ) {}
}

// ─── Trait Generic Support Classes ──────────────────────────────────────────

class UserFactory
{
    public function create(): User { return new User('', ''); }
    public function count(int $n): static { return $this; }
    public function state(array $state): static { return $this; }
    public function make(): User { return new User('', ''); }
}

/** @use HasFactory<UserFactory> */
class Product
{
    use HasFactory;

    public function getPrice(): float { return 0.0; }
    public function getSku(): string { return ''; }
}

// ─── Mixin Generic Scaffolding ─────────────────────────────────────────────

/**
 * @template TModel
 */
class ScaffoldingMixinBuilder
{
    /** @return TModel */
    public function firstOrFail(): mixed { return null; }
    /** @return TModel */
    public function find(): mixed { return null; }
}

/**
 * @template TRelatedModel
 * @mixin ScaffoldingMixinBuilder<TRelatedModel>
 */
class ScaffoldingMixinRelation
{
}

/**
 * @extends ScaffoldingMixinRelation<Product>
 */
class ScaffoldingMixinBelongsTo extends ScaffoldingMixinRelation
{
}

class ScaffoldingOrderLine
{
    public function product(): ScaffoldingMixinBelongsTo { return new ScaffoldingMixinBelongsTo(); }
}

/** @use Indexable<int, Pen> */
class PenIndex
{
    use Indexable;
}

// ─── Exception Classes ──────────────────────────────────────────────────────

class NotFoundException extends \RuntimeException {}
class ValidationException extends \RuntimeException {}
class AuthorizationException extends \RuntimeException {}

// ─── Standalone Functions ───────────────────────────────────────────────────

/**
 * @template TClass
 * @param string|null $abstract
 * @return ($abstract is class-string<TClass> ? TClass : Container)
 */
function app(?string $abstract = null): mixed
{
    static $container = null;
    if ($container === null) {
        $container = new Container();
    }
    return $abstract !== null ? $container->make($abstract) : $container;
}

/**
 * A conditional return type whose non-null branch is `mixed`. When `$key`
 * is a string, the value is of unknown type (`mixed`); otherwise it is
 * `null`. Mirrors Laravel's `session($key)` helper.
 *
 * @return ($key is string ? mixed : null)
 */
function sessionValue(?string $key = null)
{
    return $key !== null ? 'value' : null;
}

function createUser(string $name, string $email): User
{
    return new User($name, $email);
}

function makePen(): Pen
{
    return new Pen();
}

function pickPenOrPencil(): Pen|Pencil
{
    return new Pen();
}

function pickPenOrNull(): ?Pen
{
    return new Pen();
}

function getUnknownValue(): mixed
{
    return new AdminUser('', '');
}

/**
 * @template T
 * @param class-string<T> $class The class name
 * @return T
 */
function resolve(string $class): object
{
    return new $class();
}

/**
 * @return array{logger: Pen, debug: bool}
 */
function getAppConfig(): array { return []; }

function pickRockOrBanana(): Rock|Banana
{
    return new Rock();
}

/** Its one method never returns, so calling it ends the code path. */
class DemoAborter
{
    public function fail(string $why): never
    {
        throw new \RuntimeException($why);
    }
}

function demoAborter(): DemoAborter
{
    return new DemoAborter();
}

/**
 * A union that mixes a class with an array of that class, the shape a
 * conditional return type produces (`$key is null ? Rock[] : Rock`).
 *
 * @return Rock|array<Rock>|null
 */
function pickRockOrRocks(bool $one)
{
    return $one ? new Rock() : [new Rock()];
}

function crushOneRock(Rock $rock): string
{
    return $rock->crush();
}

/**
 * A union that names no class at all, the shape a framework lookup
 * returns when it may hand back either an object or the raw key it was
 * asked for (Laravel's `Route::parameter()` is annotated this way).
 *
 * @return object|string
 */
function lookUpSpecimen(bool $found)
{
    return $found ? new Rock() : 'rock';
}

function pickTag(): TextTag|NumberTag
{
    return new TextTag();
}

function pickWeighing(): Weighed|Unweighed
{
    return new Weighed();
}

/** @param numeric $value */
function takesNumeric(int|float|string $value): void {}

/** @param 'a'|'b'|'c' $grade */
function takesGrade(string $grade): void {}

/** @param non-empty-string $value */
function takesNonEmptyString(string $value): string { return $value; }

function takesGradeString(string $value): string { return $value; }

function takesStringOrArray(string|array $value): string
{
    return is_array($value) ? 'array' : $value;
}

/**
 * @param  string|array|null  $raw
 * @return string|array|null
 */
function scaffoldingReadPayload($raw)
{
    return $raw;
}

/** A setting that may not have been supplied at all. */
function scaffoldingReadSetting(string $name): ?string
{
    return $name === 'markets' ? 'dk,se' : null;
}

/** @param 'a'|'b'|'c' $mark
 *  @return 'a'|'b'|'c' */
function scaffoldingGradeOnOpeningLine(string $mark): string
{
    // Try: hover `$mark` — 'a'|'b'|'c', read off the tag the `/**` shares.
    takesGrade($mark);

    return $mark;
}

/** @param list<Pen> $tools
 *  A description sharing its line with the tag above it. */
function scaffoldingWriteAllOnOpeningLine(array $tools): string
{
    $written = '';
    foreach ($tools as $tool) {
        $written .= $tool->write();    // Pen, the element type of the declared list
    }

    return $written;
}

function takesScalar(int|float|string|bool $value): void {}

/** @return array<int, Pen>|false */
function loadPensOrFail(): array|false
{
    return [new Pen()];
}

/**
 * The next line of a stream, `false` once it is exhausted, the shape
 * `fgets()` and `fgetcsv()` have.
 *
 * @param list<string> $lines
 *
 * @return string|false
 */
function demoNextLine(array &$lines): string|false
{
    return array_shift($lines) ?? false;
}

/** @phpstan-assert Rock $value */
function assertRock(mixed $value): void
{
    if (!$value instanceof Rock) {
        throw new \InvalidArgumentException('Expected Rock');
    }
}

/** @phpstan-assert-if-true Rock $value */
function isRock(mixed $value): bool
{
    return $value instanceof Rock;
}

/** @phpstan-assert-if-false Rock $value */
function isNotRock(mixed $value): bool
{
    return !$value instanceof Rock;
}

/**
 * The shape Laravel's `filled()` helper is annotated with: the asserted
 * type is a union, and it is written in the equality form (`!=`), which
 * only speaks for the branch its tag names.
 *
 * @phpstan-assert-if-true !=null|'' $value
 *
 * @phpstan-assert-if-false !=numeric|bool $value
 */
function demoFilled(mixed $value): bool
{
    return $value !== null && $value !== '';
}

class StaticAssert
{
    /** @phpstan-assert Rock $value */
    public static function assertRock(mixed $value): void
    {
        if (!$value instanceof Rock) {
            throw new \InvalidArgumentException('Expected Rock');
        }
    }

    /** @phpstan-assert-if-true Rock $value */
    public static function isRock(mixed $value): bool
    {
        return $value instanceof Rock;
    }

    /** @phpstan-assert-if-false Rock $value */
    public static function isNotRock(mixed $value): bool
    {
        return !$value instanceof Rock;
    }

    /** @phpstan-assert object $value */
    public static function assertIsObject(mixed $value): void
    {
        if (!is_object($value)) {
            throw new \InvalidArgumentException('Expected object');
        }
    }

    /** @phpstan-assert true $condition */
    public static function assertTrue(mixed $condition): void
    {
        if ($condition !== true) {
            throw new \InvalidArgumentException('Expected true');
        }
    }

    /** @phpstan-assert false $condition */
    public static function assertFalse(mixed $condition): void
    {
        if ($condition !== false) {
            throw new \InvalidArgumentException('Expected false');
        }
    }

    /** @phpstan-assert !string $value */
    public static function assertIsNotString(mixed $value): void
    {
        if (is_string($value)) {
            throw new \InvalidArgumentException('Did not expect string');
        }
    }

    /** @phpstan-assert resource $value */
    public static function assertIsResource(mixed $value): void
    {
        if (!is_resource($value)) {
            throw new \InvalidArgumentException('Expected resource');
        }
    }

    /** @psalm-assert null $value */
    public static function assertIsNull(mixed $value): void
    {
        if ($value !== null) {
            throw new \InvalidArgumentException('Expected null');
        }
    }
}

// ─── Pipe Operator / Pass-by-Reference / Interface Template / Generic Assert ─

function createPenFromString(string $input): Pen
{
    return new Pen();
}

function initPen(?Pen &$pen): void
{
    $pen = new Pen();
}

function initPenWhen(bool $write, ?Pen &$pen): void
{
    if ($write) {
        $pen = new Pen();
    }
}

function describePen(Pen $pen): string
{
    return $pen->color();
}

class PenFactory
{
    public static function create(?Pen &$pen): void
    {
        $pen = new Pen();
    }
}

class PenBuilder
{
    public function __construct(?Pen &$pen)
    {
        $pen = new Pen();
    }
}

interface ScaffoldingEntityFinder
{
    /**
     * @template T
     * @param class-string<T> $class
     * @return T
     */
    public function find(string $class): object;
}

class ScaffoldingEntityLocator implements ScaffoldingEntityFinder
{
    public function find(string $class): object
    {
        return new $class();
    }
}

class ScaffoldingAssert
{
    /**
     * `$actual` is `mixed` rather than `object`, matching PHPUnit's own
     * signature: the whole point of the assertion is that the caller does
     * not yet know what it holds, so a nullable subject has to be able to
     * reach it.
     *
     * @template ExpectedType of object
     * @param class-string<ExpectedType> $expected
     * @phpstan-assert ExpectedType $actual
     */
    public static function assertInstanceOf(string $expected, mixed $actual): void
    {
        if (!$actual instanceof $expected) {
            throw new \InvalidArgumentException('Type mismatch');
        }
    }
}

// ─── Multi-line @return & Broken Docblock Recovery ──────────────────────────

/**
 * @template TKey of array-key
 * @template TValue
 */
class FluentCollection
{
    /** @var array<TKey, TValue> */
    private array $items;

    /** @param array<TKey, TValue> $items */
    public function __construct(array $items = []) { $this->items = $items; }

    /**
     * @template TGroupKey of array-key
     *
     * @param  (callable(TValue, TKey): TGroupKey)|array|string  $groupBy
     * @param  bool  $preserveKeys
     * @return static<
     *  ($groupBy is (array|string)
     *      ? array-key
     *      : TGroupKey),
     *  static<($preserveKeys is true ? TKey : int), TValue>
     * >
     */
    public function groupBy($groupBy, $preserveKeys = false)
    {
    }

    /**
     * @param  TKey  $key
     * @return TValue|null
     */
    public function get($key)
    {
        return $this->items[$key] ?? null;
    }

    /**
     * @template TMapValue
     *
     * @param  callable(TValue, TKey): TMapValue  $callback
     * @return static<TKey, TMapValue>
     */
    public function map(callable $callback)
    {
    }

    /**
     * @param  callable(TValue, TKey): void  $callback
     * @return static<TKey, TValue>
     */
    public function each(callable $callback)
    {
        foreach ($this->items as $key => $value) {
            $callback($value, $key);
        }
        return $this;
    }

    /** @return TValue|null */
    public function first(): mixed
    {
        return $this->items[array_key_first($this->items)] ?? null;
    }

    /**
     * @return array<
     *   string,
     *   FluentCollection<int, TValue>
     * >
     */
    public function toGroupedArray()
    {
    }

    /**
     * @return static<TKey, TValue>
     */
    public function values()
    {
    }
}

/**
 * @template TKey of array-key
 * @template TValue
 * @param array<TKey, TValue> $value
 * @return FluentCollection<TKey, TValue>
 */
function collect(array $value = []): FluentCollection
{
    return new FluentCollection($value);
}

class BrokenDocRecovery
{
    /**
     * Broken multi-line @return — base `static` is recovered.
     * @return static<
     */
    public function broken(): static
    {
        return $this;
    }

    public function working(): string
    {
        return 'hello';
    }
}

// ── Readonly write scaffolding ──────────────────────────────────────────────

class ScaffoldingCoordinate
{
    public string $label = '';

    public function __construct(
        public readonly int $x,
        public readonly int $y,
        public readonly array $tags = [],
    ) {}
}

readonly class ScaffoldingReadonlyPoint
{
    public function __construct(
        public int $x,
        public int $y,
    ) {}
}


// ── Reconstructed-proof scaffolding ─────────────────────────────────────────
// A node whose key is optional and whose name may or may not be a string is
// the shape guards are written around: the condition proves the pair together
// and a later check picks which half of it applies.

class ScaffoldingNameNode
{
    /** @var string|ScaffoldingNameNode */
    public $name = '';
}

class ScaffoldingLoopNode
{
    public ScaffoldingNameNode $valueVar;

    /** @var ScaffoldingNameNode|null */
    public $keyVar = null;

    public function __construct()
    {
        $this->valueVar = new ScaffoldingNameNode();
    }
}

class ScaffoldingArgumentAcceptor
{
    /** @param string[] $args */
    public function describe(array $args): string
    {
        return implode(', ', $args);
    }
}

/** @param string[] $args */
function scaffoldingSelectAcceptor(array $args): ScaffoldingArgumentAcceptor
{
    return new ScaffoldingArgumentAcceptor();
}

class ScaffoldingOptionalLabel
{
    public ?string $label = null;
}

/**
 * A subclass is what makes an `instanceof` guard's two paths look alike:
 * the path that failed the check keeps the parent, which spans the child,
 * so the types alone do not say which one ran.
 */
class ScaffoldingQualifiedName extends ScaffoldingNameNode
{
    public string $namespacePrefix = '';
}

// ── Member visibility scaffolding ───────────────────────────────────────────

class ScaffoldingVault
{
    public const REGION = 'eu';

    protected const ROTATION = 'monthly';

    private const MASTER_KEY = 'master';

    public string $label = 'vault';

    protected string $branch = 'main';

    private string $pin = '0000';

    private static int $openCount = 0;

    public function open(): string
    {
        return $this->pin;
    }

    protected function audit(): string
    {
        return $this->branch;
    }

    private function rotate(): string
    {
        return self::MASTER_KEY;
    }
}

class ScaffoldingBranchVault extends ScaffoldingVault
{
    private string $ledger = 'empty';

    public function ledger(): string
    {
        return $this->ledger;
    }
}
