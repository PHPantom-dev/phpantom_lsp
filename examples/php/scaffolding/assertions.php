<?php

/**
 * PHP Showcase — Runtime Assertions
 *
 * Verifies that the type claims made in the demo files' comments match
 * reality.
 * Run: php -d zend.assertions=1 examples/php/scaffolding/assertions.php
 */

namespace Demo;

require_once __DIR__ . '/../autoload.php';

use Closure;
use Demo\Scaffolding;
use Demo\Scaffolding\UserProfile as Profile;
use Exception;

// `use`s Demo\MocksServiceDemo (declared in completion.php), so it must live
// here rather than in scaffolding.php: scaffolding.php is required before every
// demo file, and this file is required after all of them, so this is the only
// place a class composing that trait can be declared without a circular
// require.
final class RequireExtendsConsumer extends Scaffolding\RequireExtendsTestCase
{
    use MocksServiceDemo;
}

// ── Runtime Assertions ──────────────────────────────────────────────────────
// Verify that the type claims in demo comments match reality.
// Run: php -d zend.assertions=1 examples/php/scaffolding/assertions.php

function runDemoAssertions(): void
{
    // ── Body Return Type Inference ──────────────────────────────────────
    $factory = new Scaffolding\ScaffoldingUntypedFactory();
    $pen = $factory->createPen();
    assert($pen instanceof Scaffolding\Pen, 'createPen() must return Scaffolding\Pen (inferred from body)');
    $staticPen = Scaffolding\ScaffoldingUntypedFactory::createPenStatic();
    assert($staticPen instanceof Scaffolding\Pen, 'createPenStatic() must return Scaffolding\Pen (inferred from body of a static call)');
    $mixedPen = $factory->createPenMixed();
    assert($mixedPen instanceof Scaffolding\Pen, 'createPenMixed() must return Scaffolding\Pen (inferred from body despite the declared @return mixed)');
    $tool = $factory->createTool(true);
    assert($tool instanceof Scaffolding\Pen || $tool instanceof Scaffolding\Pencil, 'createTool() must return Scaffolding\Pen|Scaffolding\Pencil');

    // ── Leading-backslash (absolute) function call ─────────────────────
    assert(\Demo\Scaffolding\makePen() instanceof Scaffolding\Pen, 'absolute \\Demo\\Scaffolding\\makePen() resolves the same as Scaffolding\makePen()');

    // ── Bare generic class name → @template bound ──────────────────────
    $boundedPens = new Scaffolding\ScaffoldingBoundedPenCollection([new Scaffolding\Pen()]);
    assert($boundedPens->first() instanceof Scaffolding\Pen, 'a bare generic collection yields its @template bound');
    assert((new Scaffolding\ScaffoldingBoundedPenCollection())->first() === null, 'an empty bare generic collection yields null');

    // ── Constant table read through a type operator ─────────────────────
    assert(Scaffolding\scaffoldingToolDefault('width') === 2, "Scaffolding\TOOL_DEFAULTS['width'] really is the int 2");
    assert(Scaffolding\scaffoldingToolDefault('ink') === 'black', "Scaffolding\TOOL_DEFAULTS['ink'] really is the string 'black'");
    assert(Scaffolding\scaffoldingDefaultToolSetting() === 'black', "an omitted argument really reads the entry its default names");
    assert(Scaffolding\ScaffoldingLimits::lookUp('retries') === 3, "LIMITS['retries'] really is the int 3");
    assert(Scaffolding\ScaffoldingLimits::lookUp('label') === 'off', "LIMITS['label'] really is the string 'off'");
    assert(Scaffolding\scaffoldingToolSettingName('ink') === 'ink', "'ink' really is one of Scaffolding\TOOL_DEFAULTS' keys");
    assert(in_array(Scaffolding\scaffoldingAnyToolDefault(), Scaffolding\TOOL_DEFAULTS, true), "the untemplated return really is one of Scaffolding\TOOL_DEFAULTS' values");
    assert(Scaffolding\scaffoldingToolDefaultFor('ink') === 'black', "a key read inside the body really picks the table's own value");
    assert(array_key_exists(Scaffolding\ScaffoldingLimits::anyLimitName(), Scaffolding\ScaffoldingLimits::LIMITS), "the declared return really is one of LIMITS' keys");

    // ── List order ──────────────────────────────────────────────────────
    assert(array_is_list(['left', 'right']), 'a literal written without keys really is a list');
    assert(array_is_list([0 => 'left', 1 => 'right']), 'keys written in order really are a list');
    assert(!array_is_list([1 => 'right', 0 => 'left']), 'keys written the other way round really are not a list');

    // ── A tag on the docblock's opening line ────────────────────────────
    assert(Scaffolding\scaffoldingGradeOnOpeningLine('b') === 'b', "the opening-line @param really is handed one of 'a'|'b'|'c'");
    assert(Scaffolding\scaffoldingWriteAllOnOpeningLine([new Scaffolding\Pen(), new Scaffolding\Pen()]) === '', 'the opening-line @param really iterates Pens');

    // ── Receiver retyped by a call (@psalm-this-out) ─────────────────────
    /** @var Scaffolding\ScaffoldingMutableBox<Scaffolding\Pen> $selfOutBox */
    $selfOutBox = new Scaffolding\ScaffoldingMutableBox(new Scaffolding\Pen('red'));
    assert($selfOutBox->value instanceof Scaffolding\Pen, 'the box holds a Scaffolding\Pen before replace()');
    $selfOutBox->replace(new Scaffolding\Pencil());
    assert($selfOutBox->value instanceof Scaffolding\Pencil, 'replace() swaps the contents, so the box holds a Scaffolding\Pencil after the call');
    assert(!$selfOutBox->value instanceof Scaffolding\Pen, 'a Scaffolding\Pencil is not a Scaffolding\Pen — the template argument really changed');

    // ── Trait `return $this` fluent chain ───────────────────────────────
    $page = new Scaffolding\TestablePage();
    assert($page->assertSee('a') instanceof Scaffolding\TestablePage, 'trait return $this resolves to the using class');
    assert($page->assertSee('a')->assertSee('b')->status === 200, 'chained trait return $this keeps the using class members');

    // ── Override completion scaffolding (@return $this) ─────────────────
    $rootDemo = new ClassRootCompletionDemo();
    assert($rootDemo->withLabel('x') instanceof ClassRootCompletionDemo, '@return $this on withLabel() returns the instance itself');
    assert($rootDemo->slug(['a', 'b']) instanceof ClassRootCompletionDemo, 'trait slug() @return $this returns the using class');

    // ── Ternary Condition Narrowing ─────────────────────────────────────
    $penTool = new TernaryNarrowingDemo(new Scaffolding\Pen());
    assert($penTool->toolLabel() === '', 'ternary then-branch narrows property to Scaffolding\Pen; ->write() returns ""');
    assert($penTool->repeatedCall() === '', 'ternary truthy narrows method-call subject to Scaffolding\Pen');
    $pencilTool = new TernaryNarrowingDemo(new Scaffolding\Pencil());
    assert($pencilTool->toolLabel() === null, 'ternary else-branch yields null (Scaffolding\Pencil is not a Scaffolding\Pen)');
    assert($pencilTool->repeatedCall() === null, 'ternary else-branch yields null for repeated call');

    // ── instanceof on a repeated method call ────────────────────────────
    assert(
        (new CompoundNarrowingDemo())->repeatedCall(new Scaffolding\SpecimenHolder()) === 'smash!',
        "maybe() returns null so its branch is skipped, but lookUp('rock') is a Scaffolding\\Rock",
    );
    assert(
        (new Scaffolding\SpecimenHolder())->lookUp('banana') === null,
        'a name the shelf does not carry yields null, so nothing is proved about that call',
    );

    // ── A repeated call checked by a ternary condition ───────────────────
    assert(
        is_string((new CompoundNarrowingDemo())->repeatedCallInTernary(new Scaffolding\SpecimenHolder())),
        'both ternary arms re-evaluate the checked call, and the check rules out its null',
    );

    // ── A pair of `!` cancels ───────────────────────────────────────────
    $bare = new Scaffolding\Rock();
    assert((!(!$bare)) === (bool) $bare, 'double negation is the bare truthiness test');
    $absent = null;
    assert((!(!$absent)) === false, 'a doubly negated null is still falsy');

    // ── A proof lasts only as long as the state it was made about ───────
    assert(
        (new CompoundNarrowingDemo())->callInvalidation(new Scaffolding\SpecimenHolder()) === 'specimens1',
        'a pure call and a value-returning one both leave the checked call answering the same thing',
    );
    $shelf = new Scaffolding\SpecimenHolder();
    assert($shelf->shelfLabel() === 'specimens', 'the pure call is genuinely side-effect free');
    assert($shelf->shelfCount() === 1, 'the untagged accessor only reports what is on the shelf');
    assert($shelf->item instanceof Scaffolding\Rock, 'neither of them changed what the holder carries');
    $shelf->restock();
    assert($shelf->item instanceof Scaffolding\Banana, 'restock() really does change what the holder carries');
    $rotated = new Scaffolding\SpecimenHolder();
    assert($rotated->rotate() === true, 'the @impure call still hands a value back');
    assert($rotated->item instanceof Scaffolding\Banana, 'and changes what the holder carries, which is why it is tagged');

    // ── Two checks that both hold make one value of both types ──────────
    $compound = new CompoundNarrowingDemo();
    assert(
        $compound->bothAtOnce(new Scaffolding\LabelledRock()) === 'smash!:granite',
        'a value that is both a Scaffolding\Rock and a Scaffolding\Labelled satisfies either one alone',
    );
    assert($compound->bothAtOnce(new Scaffolding\Rock()) === '', 'a plain Scaffolding\Rock is not Scaffolding\Labelled');
    assert(
        $compound->assertedBoth(new Scaffolding\LabelledRock()) === 'smash!:granite',
        'assert() proves the extra interface without losing the declared class',
    );

    // ── `match (true)` arm narrowing ────────────────────────────────────
    $armDemo = new MatchArmNarrowingDemo();
    $shelfForArms = new Scaffolding\SpecimenHolder();
    assert(
        $armDemo->describe($shelfForArms, 'rock') === 'smash!',
        "lookUp('rock') is a Scaffolding\\Rock, so the instanceof arm runs",
    );
    assert(
        $armDemo->describe($shelfForArms, 'banana') === 'nothing',
        'a null specimen matches neither arm, so the default runs',
    );
    assert(
        $armDemo->weights($shelfForArms) === [5.0],
        "only lookUp('rock') answers, so the second arm builds a one-element list of floats",
    );
    assert(
        $armDemo->eitherCondition($shelfForArms) === 'smash!',
        'the first of the two conditions is enough to enter the arm',
    );
    assert($armDemo->fallback($shelfForArms) === 5.0, 'the default runs with the null already ruled out');

    // ── A `?->` chain compared to a value that cannot be null ───────────
    $chainDemo = new NullsafeComparisonDemo();
    $shelfForChain = new Scaffolding\SpecimenHolder();
    assert(
        $shelfForChain->lookUp('rock')?->weigh() === $shelfForChain->item->weigh(),
        'the demo relies on a Scaffolding\Rock weighing what the holder carries weighs',
    );
    assert($chainDemo->sameWeight($shelfForChain) === '5', 'the chain ran, so the comparison holds');
    assert(
        $chainDemo->bothMayBeNull($shelfForChain) === 'different',
        "lookUp('banana') is null, so the two chains are not identical",
    );

    // ── Variables filled together in one branch ─────────────────────────
    $correlated = new CorrelatedNullDemo();
    $shelfForPair = new Scaffolding\SpecimenHolder();
    assert(
        $correlated->describe($shelfForPair, 'rock') === '5kg',
        'the branch that finds the specimen is the one that labels it',
    );
    assert(
        $correlated->describe($shelfForPair, 'banana') === 'nothing on the shelf',
        'a name the shelf has none of leaves both variables null',
    );
    assert(
        $correlated->unrelated(true, false) === 'unlabelled',
        'the specimen can be there without the label being there too',
    );
    assert(
        $correlated->unrelated(true, true) === '1kg',
        'both conditions holding fills both variables',
    );

    // ── Discriminating-property narrowing ───────────────────────────────
    $discriminant = new PropertyDiscriminantDemo();
    assert(
        $discriminant->byTypeGuard() === 7,
        'is_string() on the tag picks Scaffolding\TextTag, whose letters() counts "granite"',
    );
    assert(
        $discriminant->byTagValue() === 5.0,
        "state === 'weighed' picks Scaffolding\Weighed, the only member with grams",
    );

    // ── Scalar guards in compound conditions and ternaries ─────────────
    $scalarGuards = new ScalarGuardNarrowingDemo();
    assert($scalarGuards->rejectEverythingElse('payload') === 'payload', 'a string payload survives both conjuncts');
    assert($scalarGuards->rejectEverythingElse('') === 'skipped', 'an empty string is rejected by the second conjunct');
    assert($scalarGuards->rejectEverythingElse([]) === 'skipped', 'an array is rejected by the first conjunct');
    assert($scalarGuards->rejectEverythingElse(null) === 'skipped', 'null is rejected by the first conjunct');
    assert($scalarGuards->eitherArm('week') === 'week', 'the then arm yields the checked string');
    assert($scalarGuards->eitherArm(null) === 'today', 'the else arm yields the literal fallback');
    assert($scalarGuards->nested('text') === 'text', 'a nested ternary picks the string arm first');
    assert($scalarGuards->nested(7) === '7', 'the inner arm casts the int the outer condition ruled out');
    assert($scalarGuards->nested(null) === 'none', 'both conditions failing reaches the innermost fallback');
    assert($scalarGuards->fromElseif(true, 'x') === 'skipped', 'the leading if wins when it holds');
    assert($scalarGuards->fromElseif(false, 'x') === 'x', 'the elseif assignment is truthy and reaches the body');
    assert($scalarGuards->fromElseif(false, null) === 'empty', 'a falsy elseif assignment falls through');
    assert(
        $scalarGuards->labelMap([new Scaffolding\LabelledRock()]) === ['granite' => 'smash!'],
        'a map written under a string key really is keyed by that string',
    );
    assert(
        $scalarGuards->numberedLines() === [1 => 'first', 2 => 'second'],
        'a pre-incremented counter really produces integer keys',
    );
    assert(
        $scalarGuards->onlyWhatCanBeTrue() === ['dk', 'se'],
        'the truthy branch really is reached with the string, never the empty array',
    );
    assert($scalarGuards->whatMayStillBeEither('b') === 'b', 'a non-empty label is truthy');
    assert($scalarGuards->whatMayStillBeEither('') === 'unlabelled', "'' is falsy, so a plain string spans both branches");

    // ── class-string guard keeps its type argument ─────────────────────
    $guarded = (new ClassStringVarDemo())->guardedInstantiation(Scaffolding\Pen::class);
    assert($guarded instanceof Scaffolding\Pen, 'new $className() resolves to Scaffolding\Pen after a class_exists() guard');

    // ── property_exists() narrowing ─────────────────────────────────────
    // The dynamic fields are populated through a variable property name so the
    // runtime setup mirrors how these responses are filled in real code.
    $memberDemo = new MemberExistsNarrowingDemo();
    $response = new Scaffolding\ApiResponse();
    $field = 'errorMessage';
    $response->$field = 'boom';                    // dynamically populated
    assert($memberDemo->property($response) === 'boom', 'property_exists guard reads the dynamic property');
    assert($memberDemo->property(new Scaffolding\ApiResponse()) === null, 'no property → guard is false');
    $withDetail = new Scaffolding\ApiResponse();
    $field = 'detail';
    $withDetail->$field = 'context';
    assert($memberDemo->guardClause($withDetail) === 'context', 'negated property_exists guard clause reads the property after it');
    assert($memberDemo->guardClause(new Scaffolding\ApiResponse()) === 'none', 'guard clause returns early when the property is absent');
    $issetResp = new Scaffolding\ApiResponse();
    $field = 'errorMessage';
    $issetResp->$field = 'boom';
    assert($memberDemo->issetGuard($issetResp) === 'boom', 'isset($obj->prop) guard reads the dynamic property');
    assert($memberDemo->issetGuard(new Scaffolding\ApiResponse()) === null, 'isset guard is false when the property is absent');
    $ternaryResp = new Scaffolding\ApiResponse();
    $field = 'detail';
    $ternaryResp->$field = 'd';
    $field = 'errorMessage';
    $ternaryResp->$field = 'e';
    assert($memberDemo->ternary($ternaryResp) === 'd|e', 'property_exists and isset ternaries read the proven properties');
    assert($memberDemo->ternary(new Scaffolding\ApiResponse()) === 'none|none', 'member-existence ternaries fall back when the properties are absent');

    // ── Wide `array` callback parameter narrows to the element shape ────
    $pairColors = (new ArrayFuncDemo())->pairColors(new Scaffolding\ScaffoldingArrayFunc());
    assert($pairColors === ['blue', 'red'], 'array_map callback reads Scaffolding\Pen::color() through a declared `array` parameter');

    // ── Array builtins answer in terms of the array they were handed ────
    $keyValue = (new ArrayFuncDemo())->keyValueSummary(new Scaffolding\ScaffoldingArrayFunc());
    assert($keyValue['names'] === ['blue', 'red'], 'array_keys() over array<string, Scaffolding\Pen> yields the string keys');
    assert($keyValue['first'] === 'ink', 'array_filter() with no callback drops the null entry, so array_values()[0] is the surviving string');
    assert($keyValue['total'] === 9, 'array_sum() over a list<int> is an int');
    assert(array_keys((new Scaffolding\ScaffoldingArrayFunc())->labels()) === [0, 1], 'array_keys() over a list yields int keys');
    assert(array_search('gel', (new Scaffolding\ScaffoldingArrayFunc())->labels()) === 1, 'array_search() over a list yields an int key');
    assert(array_key_first((new Scaffolding\ScaffoldingArrayFunc())->byName()) === 'blue', 'array_key_first() over a string-keyed array yields a string');
    $stringKeyed = array_filter((new Scaffolding\ScaffoldingArrayFunc())->mixedKeys(), fn($key) => is_string($key), ARRAY_FILTER_USE_KEY);
    assert(array_keys($stringKeyed) === ['ink'], 'array_filter() with ARRAY_FILTER_USE_KEY keeps only the keys its callback approves of');
    $named = array_filter((new Scaffolding\ScaffoldingArrayFunc())->optionalLabels(), fn($label) => $label !== null);
    assert($named === ['ink' => 'ink'], 'array_filter() with a value callback keeps only the entries it approves of, so no null survives');
    $writers = array_values(array_filter((new Scaffolding\ScaffoldingArrayFunc())->mixedWriters(), fn($writer) => $writer instanceof Scaffolding\Pen));
    assert(count($writers) === 2 && $writers[0] instanceof Scaffolding\Pen, 'array_filter() with an instanceof callback keeps only that class');
    $sparse = array_filter([3, 4, 5], fn($v) => $v > 3);
    assert(array_keys($sparse) === [1, 2], 'array_filter() keeps the key of every entry it keeps, so filtering a list leaves gaps rather than another list');
    $collected = [];
    foreach ([['ink'], ['gel']] as $batch) {
        $collected = array_merge($collected, $batch);
    }
    assert($collected === ['ink', 'gel'], 'array_merge() into an accumulator that started as [] holds everything that was merged in');
    $merged = array_merge([0 => 'nib'], ['ink' => 'gel', 3 => 'lead']);
    assert($merged === [0 => 'nib', 'ink' => 'gel', 1 => 'lead'], 'array_merge() renumbers integer keys as it appends and carries string keys over');

    // ── array<T>|false keeps its element type after a false check ────────
    $pens = Scaffolding\loadPensOrFail();
    assert($pens !== false && $pens[0] instanceof Scaffolding\Pen, 'Scaffolding\loadPensOrFail() returns array<int, Scaffolding\Pen> on success');

    // ── is_iterable() keeps arrays and \Traversable objects ─────────────
    assert(is_iterable(new Scaffolding\ItemIterableCollection()), 'an IteratorAggregate is iterable, so the guard keeps it');
    assert(!is_iterable(new Scaffolding\Pen()), 'a plain object is not iterable, so the guard drops it');
    assert(is_iterable([new Scaffolding\Pen()]), 'an array is iterable');
    assert(!is_iterable('ink'), 'a string is not iterable');

    // ── @phpstan-require-extends base members on $this ──────────────────
    $consumer = new RequireExtendsConsumer();
    assert($consumer->mockPath() === '/tmp/mock', 'trait method reaches base class member via @phpstan-require-extends');

    // ── Pseudo-type class-name collision ────────────────────────────────
    $num = new Scaffolding\Number('42');
    assert($num->scaled(2) instanceof Scaffolding\Number, 'Scaffolding\Number::scaled() must return Scaffolding\Number (class, not pseudo-type)');
    assert(Scaffolding\scaleNumber($num) instanceof Scaffolding\Number, 'Scaffolding\scaleNumber() must accept and return a Scaffolding\Number');

    // ── Return Type: static ─────────────────────────────────────────────
    $pen = Scaffolding\Pen::make();
    assert($pen instanceof Scaffolding\Pen, 'Scaffolding\Pen::make() must return Scaffolding\Pen');

    $marker = Scaffolding\Marker::make();
    assert($marker instanceof Scaffolding\Marker, 'Scaffolding\Marker::make() must return Scaffolding\Marker (not Scaffolding\Pen)');

    $fluent = $marker->rename('Bold');
    assert($fluent instanceof Scaffolding\Marker, 'Scaffolding\Marker::rename() returns static, must stay Scaffolding\Marker');

    // ── Return Type: late static binding stays open on a forwarding call ─
    $lsb = new LateStaticBindingDemo();
    foreach ($lsb->forwardedInstances() as $forwarded) {
        assert(
            $forwarded instanceof LateStaticBindingDemo,
            'self::, static::, and parent:: all forward late static binding'
        );
    }
    assert(
        !(Scaffolding\Pen::make() instanceof LateStaticBindingDemo),
        'naming the class pins it: Scaffolding\Pen::make() is exactly a Scaffolding\Pen'
    );

    // ── Return Type: function ───────────────────────────────────────────
    $created = Scaffolding\makePen();
    assert($created instanceof Scaffolding\Pen, 'Scaffolding\makePen() must return Scaffolding\Pen');

    $union = Scaffolding\pickPenOrPencil();
    assert($union instanceof Scaffolding\Pen || $union instanceof Scaffolding\Pencil, 'Scaffolding\pickPenOrPencil() must return Scaffolding\Pen|Scaffolding\Pencil');

    $rock = Scaffolding\pickRockOrBanana();
    assert($rock instanceof Scaffolding\Rock || $rock instanceof Scaffolding\Banana, 'Scaffolding\pickRockOrBanana() must return Scaffolding\Rock|Scaffolding\Banana');

    $user = Scaffolding\createUser('Alice', 'alice@example.com');
    assert($user instanceof Scaffolding\User, 'Scaffolding\createUser() must return Scaffolding\User');

    // ── Chaining ────────────────────────────────────────────────────────
    $brush = new Scaffolding\Brush();
    $sized = $brush->setSize('large');
    assert($sized instanceof Scaffolding\Brush, 'Scaffolding\Brush::setSize() returns static, must stay Scaffolding\Brush');
    $styled = $sized->setStyle('pointed');
    assert($styled instanceof Scaffolding\Brush, 'Scaffolding\Brush::setStyle() returns static, must stay Scaffolding\Brush');

    $canvas = $brush->getCanvas();
    assert($canvas instanceof Scaffolding\Canvas, 'Scaffolding\Brush::getCanvas() must return Scaffolding\Canvas');

    $backToBrush = $canvas->getBrush();
    assert($backToBrush instanceof Scaffolding\Brush, 'Scaffolding\Canvas::getBrush() must return Scaffolding\Brush');

    $easel = $canvas->easel;
    assert($easel instanceof Scaffolding\Easel, 'Scaffolding\Canvas::$easel must be Scaffolding\Easel');

    // ── Inferred nested tuple literals ──────────────────────────────────
    $rows = [[new Scaffolding\Pen(), new Scaffolding\Pencil()]];
    foreach ($rows as $row) {
        assert($row[0] instanceof Scaffolding\Pen, 'nested tuple index 0 must be Scaffolding\Pen');
        assert($row[1] instanceof Scaffolding\Pencil, 'nested tuple index 1 must be Scaffolding\Pencil');
    }

    // Indexing a position only some arms have, with a `?? Class::class`
    // fallback, stays a class-string that instantiates Scaffolding\Pen|Scaffolding\Pencil.
    $specs = [['pen', Scaffolding\Pen::class], ['pencil']];
    foreach ($specs as $spec) {
        $toolClass = $spec[1] ?? Scaffolding\Pencil::class;
        assert(class_exists($toolClass), 'index + ?? fallback must yield a class-string');
        $tool = new $toolClass();
        assert($tool instanceof Scaffolding\Pen || $tool instanceof Scaffolding\Pencil, 'class-string must instantiate Scaffolding\Pen|Scaffolding\Pencil');
    }

    // A row pushed as a literal destructures back into the values it was
    // written with, one per position.
    $entries = [];
    $entries[] = [new Scaffolding\Pen(), 'sketchbook'];
    foreach ($entries as $entry) {
        [$writer, $surface] = $entry;
        assert($writer instanceof Scaffolding\Pen, 'pushed tuple slot 0 must be Scaffolding\Pen');
        assert(is_string($surface), 'pushed tuple slot 1 must be a string');
    }

    // A key worked out at runtime still holds the value that was written
    // against it.
    $slot = 'ink';
    $bySlot = [$slot => new Scaffolding\Pen()];
    assert($bySlot['ink'] instanceof Scaffolding\Pen, 'a runtime key must hold the value written against it');

    // `+` keeps the left operand's keys and adds the right operand's.
    $merged = ['pen' => new Scaffolding\Pen()] + ['pencil' => new Scaffolding\Pencil()];
    assert($merged['pen'] instanceof Scaffolding\Pen, 'array union must keep the left key');
    assert($merged['pencil'] instanceof Scaffolding\Pencil, 'array union must add the right key');

    // Casting an empty array produces a property-less stdClass.
    $bare = (object) [];
    assert($bare instanceof \stdClass, '(object) [] must be a stdClass');
    assert(get_object_vars($bare) === [], '(object) [] must have no properties');

    // ── Indexing an ArrayAccess Object ───────────────────────────────────
    $penAccess = new Scaffolding\ScaffoldingPenArrayAccess();
    assert($penAccess[0] instanceof Scaffolding\Pen, 'ArrayAccess[0] must resolve via offsetGet(): Scaffolding\Pen');

    $genericAccess = new Scaffolding\ScaffoldingGenericArrayAccess([new Scaffolding\Pen()]);
    assert($genericAccess[0] instanceof Scaffolding\Pen, 'ArrayAccess<int, T>[0] must resolve T bound to Scaffolding\Pen');

    // ── Conditional @return with nested static<...> generic branches ────
    /** @var Scaffolding\TypedCollection<int, Scaffolding\Pen> $penItems */
    $penItems = new Scaffolding\TypedCollection([new Scaffolding\Pen()]);
    $penBatches = $penItems->chunk(1);
    assert(
        $penBatches->first()->first() instanceof Scaffolding\Pen,
        'Collection::chunk() conditional return keeps the nested collection element type'
    );

    // ── Fluent Scaffolding\Model chains (static return) ─────────────────────────────
    $userObj = new Scaffolding\User('Bob', 'bob@example.com');
    $renamed = $userObj->setName('Robert');
    assert($renamed instanceof Scaffolding\User, 'Scaffolding\User::setName() returns static, must stay Scaffolding\User');

    $timestamped = $userObj->setCreatedAt('2024-01-01');
    assert($timestamped instanceof Scaffolding\User, 'Scaffolding\HasTimestamps::setCreatedAt() returns static, must stay Scaffolding\User');

    // ── Scaffolding\User method return types ────────────────────────────────────────
    $profile = $userObj->getProfile();
    assert($profile instanceof Scaffolding\UserProfile, 'Scaffolding\User::getProfile() must return Scaffolding\UserProfile');

    $status = $userObj->getStatus();
    assert($status instanceof Scaffolding\Status, 'Scaffolding\User::getStatus() must return Scaffolding\Status');

    // ── Type narrowing: instanceof ──────────────────────────────────────
    $specimen = Scaffolding\pickRockOrBanana();
    if ($specimen instanceof Scaffolding\Rock) {
        assert(method_exists($specimen, 'crush'), 'Scaffolding\Rock must have crush()');
    } else {
        assert($specimen instanceof Scaffolding\Banana, 'Not Scaffolding\Rock must be Scaffolding\Banana');
        assert(method_exists($specimen, 'peel'), 'Scaffolding\Banana must have peel()');
    }

    // ── Type narrowing: instanceof over a class|array|null union ────────
    $onlyRock = Scaffolding\pickRockOrRocks(true);
    assert($onlyRock instanceof Scaffolding\Rock, 'the true branch really hands back a single Scaffolding\Rock');
    assert(Scaffolding\crushOneRock($onlyRock) === 'smash!', 'the narrowed value really is what Scaffolding\crushOneRock() accepts');
    assert(is_array(Scaffolding\pickRockOrRocks(false)), 'the false branch really is the array alternative instanceof rules out');
    (new TypeNarrowingDemo())->guardClause();

    // ── Type narrowing: instanceof over a union that names no class ─────
    $looked = Scaffolding\lookUpSpecimen(true);
    assert($looked instanceof Scaffolding\Rock, 'the found branch really hands back the object alternative');
    assert(Scaffolding\crushOneRock($looked) === 'smash!', 'the narrowed value really is what Scaffolding\crushOneRock() accepts');
    assert(is_string(Scaffolding\lookUpSpecimen(false)), 'the missing branch really is the string alternative instanceof rules out');

    // ── Type narrowing: inline && ───────────────────────────────────────
    $sample = Scaffolding\pickRockOrBanana();
    if ($sample instanceof Scaffolding\Rock && $sample->crush()) {
        assert($sample instanceof Scaffolding\Rock, 'RHS of && must see Scaffolding\Rock');
    }

    // ── Type narrowing: negated instanceof ──────────────────────────────
    $specimen2 = Scaffolding\pickRockOrBanana();
    if (!$specimen2 instanceof Scaffolding\Rock) {
        assert($specimen2 instanceof Scaffolding\Banana, 'Not Scaffolding\Rock must be Scaffolding\Banana');
    }

    // ── Type narrowing: a boolean standing for a check ──────────────────
    $stored = Scaffolding\pickRockOrBanana();
    $isRock = $stored instanceof Scaffolding\Rock;
    $chosen = $isRock ? $stored : new Scaffolding\Rock();
    assert($chosen instanceof Scaffolding\Rock, 'the ternary arm a boolean check picks really yields a Scaffolding\Rock');
    assert(Scaffolding\crushOneRock($chosen) === 'smash!', 'the value the arm yields really is what Scaffolding\crushOneRock() accepts');

    $shelved = Scaffolding\pickRockOrBanana();
    $isKnown = $shelved instanceof Scaffolding\Rock || $shelved instanceof Scaffolding\Banana;
    assert($isKnown, 'the `||` chain really covers everything Scaffolding\pickRockOrBanana() returns');
    assert(is_float($shelved->weigh()), 'both classes the chain lists really declare weigh()');

    // ── Type narrowing: a variable a guarded branch filled ──────────────
    $weighed = Scaffolding\pickRockOrBanana();
    $label = null;
    if ($weighed instanceof Scaffolding\Rock) {
        $label = new Scaffolding\SpecimenLabel('rock');
    }
    if ($label !== null) {
        assert($weighed instanceof Scaffolding\Rock, 'reaching the `!== null` test really means the branch ran');
    }

    // ── Type narrowing: assert() ────────────────────────────────────────
    $target = Scaffolding\pickRockOrBanana();
    if ($target instanceof Scaffolding\Banana) {
        assert(method_exists($target, 'peel'), 'assert narrowed Scaffolding\Banana must have peel()');
    }

    // ── Custom assert functions ─────────────────────────────────────────
    $unknown = new Scaffolding\Rock();
    Scaffolding\assertRock($unknown);
    assert($unknown instanceof Scaffolding\Rock, 'Scaffolding\assertRock() must narrow to Scaffolding\Rock');

    assert(Scaffolding\isRock(new Scaffolding\Rock()) === true, 'Scaffolding\isRock(Scaffolding\Rock) must return true');
    assert(Scaffolding\isRock(new Scaffolding\Banana()) === false, 'Scaffolding\isRock(Scaffolding\Banana) must return false');
    assert(Scaffolding\isNotRock(new Scaffolding\Rock()) === false, 'Scaffolding\isNotRock(Scaffolding\Rock) must return false');
    assert(Scaffolding\isNotRock(new Scaffolding\Banana()) === true, 'Scaffolding\isNotRock(Scaffolding\Banana) must return true');

    // ── Static assert functions ─────────────────────────────────────────
    $unknown2 = new Scaffolding\Rock();
    Scaffolding\StaticAssert::assertRock($unknown2);
    assert($unknown2 instanceof Scaffolding\Rock, 'Scaffolding\StaticAssert::assertRock() must narrow to Scaffolding\Rock');

    assert(Scaffolding\StaticAssert::isRock(new Scaffolding\Rock()) === true, 'Scaffolding\StaticAssert::isRock(Scaffolding\Rock) must return true');
    assert(Scaffolding\StaticAssert::isNotRock(new Scaffolding\Banana()) === true, 'Scaffolding\StaticAssert::isNotRock(Scaffolding\Banana) must return true');

    // ── Pseudo-type assertions ──────────────────────────────────────────
    $handle = fopen('php://memory', 'rb');
    Scaffolding\StaticAssert::assertIsResource($handle);
    assert(is_resource($handle), 'Scaffolding\StaticAssert::assertIsResource() accepts a real handle');
    fclose($handle);

    $nothing = null;
    Scaffolding\StaticAssert::assertIsNull($nothing);
    assert($nothing === null, 'Scaffolding\StaticAssert::assertIsNull() accepts null');

    // ── Null-init + foreach reassignment (B11) ──────────────────────────
    $pens = [new Scaffolding\Pen('blue'), new Scaffolding\Pen('red')];
    $found = null;
    foreach ($pens as $pen) {
        if ($pen->color() === 'blue') {
            $found = $pen;
        }
    }
    assert($found instanceof Scaffolding\Pen, 'Null-init + foreach reassign must resolve to Scaffolding\Pen');
    assert(method_exists($found, 'write'), 'Scaffolding\Pen from foreach must have write()');

    // ── instanceof self/static/parent ───────────────────────────────────
    $sedan = new Scaffolding\ScaffoldingSedan();
    assert($sedan instanceof Scaffolding\ScaffoldingMotor, 'Scaffolding\ScaffoldingSedan must extend Scaffolding\ScaffoldingMotor');
    assert(method_exists($sedan, 'cruise'), 'Scaffolding\ScaffoldingSedan must have cruise()');
    assert(method_exists($sedan, 'start'), 'Scaffolding\ScaffoldingSedan must inherit start()');

    // ── Negated disjunction and subclass subtraction ────────────────────
    $negated = new NegatedDisjunctionDemo();
    assert($negated->demo(new Scaffolding\ScaffoldingSedan()) === 'sedan', 'Sedan must take the sedan branch');
    assert($negated->demo(new Scaffolding\ScaffoldingCoupe()) === 'race', 'Coupe must take the coupe branch');
    assert($negated->demo(new Scaffolding\ScaffoldingMotor()) === 'other', 'A bare motor must be ruled out by the guard');
    assert($negated->keepsSubclass(new Scaffolding\ScaffoldingSportSedan()) === 'launch', 'A subclass must pass a check on its parent');
    assert($negated->keepsSubclass(new Scaffolding\ScaffoldingCoupe()) === 'race', 'A failed check must leave only the coupe');
    assert($negated->provesOneLegOnly(new Scaffolding\ScaffoldingMotor(), true) === 'motor', 'A flag leg must let a bare motor in');
    assert($negated->provesOneLegOnly(new Scaffolding\ScaffoldingCoupe(), false) === 'motor', 'A coupe must take the instanceof leg');
    assert(
        (new Scaffolding\ScaffoldingSportSedan()) instanceof Scaffolding\ScaffoldingSedan,
        'Scaffolding\ScaffoldingSportSedan must extend Scaffolding\ScaffoldingSedan'
    );

    $demo = new InstanceofSelfDemo();
    assert($demo instanceof Scaffolding\ScaffoldingSedan, 'InstanceofSelfDemo must extend Scaffolding\ScaffoldingSedan');
    assert(method_exists($demo, 'sport'), 'InstanceofSelfDemo must have sport()');
    assert(method_exists($demo, 'cruise'), 'InstanceofSelfDemo must inherit cruise()');

    // ── Method-level @template (runtime resolution) ─────────────────────
    $locator = new Scaffolding\ServiceLocator();
    $locatedPen = $locator->get(Scaffolding\Pen::class);
    assert($locatedPen instanceof Scaffolding\Pen, 'Scaffolding\ServiceLocator::get(Scaffolding\Pen::class) must return Scaffolding\Pen');

    // A single-quoted class-string literal names the class after the source
    // `\\` escape is collapsed to a single namespace separator.
    assert($locator->get('Demo\\Scaffolding\\Pen') instanceof Scaffolding\Pen, "Scaffolding\ServiceLocator::get('Demo\\\\Scaffolding\\\\Pen') must return Scaffolding\Pen");

    // A union of class-strings binds the bounded template to the union of
    // concrete classes; each member's stub returns an instance of itself.
    foreach ([Scaffolding\Pen::class, Scaffolding\Marker::class] as $penClass) {
        $group = $locator->getAll($penClass);
        assert($group[0] instanceof $penClass, 'getAll() must return an instance of each class in the union');
    }

    // Indexing the call result inline keeps the template binding.
    assert($locator->getAll(Scaffolding\Pen::class)[0] instanceof Scaffolding\Pen, 'getAll(Scaffolding\Pen::class)[0] must be a Scaffolding\Pen');

    // A class-string<T>|T union parameter accepts a class name or an
    // instance and returns an instance either way.
    assert($locator->build(Scaffolding\Pen::class) instanceof Scaffolding\Pen, 'build(Scaffolding\Pen::class) must return a Scaffolding\Pen instance');
    assert($locator->build(new Scaffolding\Pen()) instanceof Scaffolding\Pen, 'build(new Scaffolding\Pen()) must return a Scaffolding\Pen instance');

    $createdPen = Scaffolding\Factory::create(Scaffolding\Pen::class);
    assert($createdPen instanceof Scaffolding\Pen, 'Scaffolding\Factory::create(Scaffolding\Pen::class) must return Scaffolding\Pen');

    $resolved = Scaffolding\resolve(Scaffolding\Marker::class);
    assert($resolved instanceof Scaffolding\Marker, 'Scaffolding\resolve(Scaffolding\Marker::class) must return Scaffolding\Marker');

    // ── Scaffolding\TypedCollection::make() carries its element type ────────────────
    $madePens = Scaffolding\TypedCollection::make([new Scaffolding\Pen('blue')]);
    assert($madePens instanceof Scaffolding\TypedCollection, 'Scaffolding\TypedCollection::make() must return a Scaffolding\TypedCollection');
    assert($madePens->first() instanceof Scaffolding\Pen, 'Scaffolding\TypedCollection::make([Scaffolding\Pen])->first() must return Scaffolding\Pen');
    assert(Scaffolding\TypedCollection::make([new Scaffolding\Pen('red')])->first() instanceof Scaffolding\Pen, 'the same holds for a directly chained call');

    // ── Scaffolding\ObjectMapper::wrap() → Scaffolding\TypedCollection ──────────────────────────
    $mapper = new Scaffolding\ObjectMapper();
    $wrapped = $mapper->wrap(new Scaffolding\Pen());
    assert($wrapped instanceof Scaffolding\TypedCollection, 'Scaffolding\ObjectMapper::wrap() must return Scaffolding\TypedCollection');
    $first = $wrapped->first();
    assert($first instanceof Scaffolding\Pen, 'wrap(Scaffolding\Pen)->first() must return Scaffolding\Pen');

    // Untyped class constant argument binds the template to its value type (int).
    $constValue = $mapper->identity(ConstantTypeDemo::TIMEOUT);
    assert(is_int($constValue), 'identity(ConstantTypeDemo::TIMEOUT) must return int (constant value type, not owning class)');

    // A `::class` argument binds `@param T` to a class-string, so the
    // returned value is the fully-qualified class name string.
    $penClass = $mapper->identity(Scaffolding\Pen::class);
    assert($penClass === Scaffolding\Pen::class, 'identity(Scaffolding\Pen::class) must return the class-string Scaffolding\Pen::class');

    // An identity generic bound to an array type (`@template T of Scaffolding\Pen[]`)
    // must still return the array unchanged.
    $peeked = $mapper->peekLast([new Scaffolding\Pen('blue')]);
    assert(end($peeked) instanceof Scaffolding\Pen, 'peekLast() must return its argument unchanged');

    // ── Scaffolding\ScaffoldingReducible::reduce() — closure return type binding ────
    /** @var Scaffolding\ScaffoldingReducible<Scaffolding\Pencil> $reducible */
    $reducible = new Scaffolding\ScaffoldingReducible();
    $reduced = $reducible->reduce(
        fn(Scaffolding\Pen $carry, Scaffolding\Pencil $item): Scaffolding\Pen => $carry,
        new Scaffolding\Pen('starter')
    );
    assert($reduced instanceof Scaffolding\Pen, 'reduce() with fn(): Scaffolding\Pen must return Scaffolding\Pen');

    // Chained call: reduce() result used directly without intermediate variable.
    $chainedWrite = $reducible->reduce(fn(Scaffolding\Pen $carry, Scaffolding\Pencil $item): Scaffolding\Pen => $carry, new Scaffolding\Pen('starter'))->write();
    assert(is_string($chainedWrite), 'reduce()->write() chained must return string (Scaffolding\Pen::write() return type)');

    // Unannotated closure: the return type follows from the body expression
    // resolved through the closure's own typed parameter.
    $inferredReduce = $reducible->reduce(
        fn(Scaffolding\Pen $carry, $item) => $carry->rename('merged'),
        new Scaffolding\Pen('starter')
    );
    assert($inferredReduce instanceof Scaffolding\Pen, 'reduce(fn(Scaffolding\Pen $carry, $item) => $carry->rename(...)) must return Scaffolding\Pen (inferred from body)');

    // ── Scaffolding\ScaffoldingClosureCache::remember() — unannotated closure body ──
    $cache = new Scaffolding\ScaffoldingClosureCache();
    $cachedPen = $cache->remember('pen', fn() => new Scaffolding\Pen('cached'));
    assert($cachedPen instanceof Scaffolding\Pen, 'remember(fn() => new Scaffolding\Pen()) must return Scaffolding\Pen (T from arrow body)');
    $cachedMarker = $cache->remember('marker', function () {
        return new Scaffolding\Marker('cached');
    });
    assert($cachedMarker instanceof Scaffolding\Marker, 'remember(function () { return new Scaffolding\Marker(); }) must return Scaffolding\Marker (T from closure body)');
    $staticPen = $cache->remember('static-pen', static fn(): Scaffolding\Pen => new Scaffolding\Pen('cached'));
    assert($staticPen instanceof Scaffolding\Pen, 'remember(static fn(): Scaffolding\Pen => ...) must return Scaffolding\Pen (T from static arrow function)');
    $staticMarker = $cache->remember('static-marker', static function () {
        return new Scaffolding\Marker('cached');
    });
    assert($staticMarker instanceof Scaffolding\Marker, 'remember(static function () { return new Scaffolding\Marker(); }) must return Scaffolding\Marker (T from static closure body)');
    $madePen = $cache->remember('made', fn() => Scaffolding\Pen::make('blue'));
    assert($madePen instanceof Scaffolding\Pen, 'remember(fn() => Scaffolding\Pen::make(...)) must return Scaffolding\Pen (T from a static-call body)');
    $renamedMarker = new Scaffolding\Marker('yellow');
    $cachedRenamed = $cache->remember('renamed', fn() => $renamedMarker->rename('bold'));
    assert($cachedRenamed instanceof Scaffolding\Marker, 'remember(fn() => $marker->rename(...)) must return Scaffolding\Marker (T from a method-call body)');

    // ── Scaffolding\ScaffoldingToolbox::combine() — one template, two binding sites ─
    $toolbox = new Scaffolding\ScaffoldingToolbox();
    $mixedTools = $toolbox->combine([new Scaffolding\Pen('red')], [new Scaffolding\Pencil()]);
    assert($mixedTools[0] instanceof Scaffolding\Pen, 'combine() must keep the first argument\'s elements first');
    assert($mixedTools[1] instanceof Scaffolding\Pencil, 'combine() must return both arguments\' elements, so T is Scaffolding\Pen|Scaffolding\Pencil');
    $combinedPens = $toolbox->combine([new Scaffolding\Pen('red')], [new Scaffolding\Pen('blue')]);
    assert(count($combinedPens) === 2, 'combine() of two Scaffolding\Pen lists must return both, keeping T at Scaffolding\Pen');
    $secondOnly = $toolbox->combine([], [new Scaffolding\Pen('blue')]);
    assert($secondOnly[0] instanceof Scaffolding\Pen, 'combine([], [new Scaffolding\Pen()]) must return the Scaffolding\Pen — an empty literal contributes nothing to T');

    // ── Scaffolding\ScaffoldingEventBus::listen() — closure param type binding ──────
    $bus = new Scaffolding\ScaffoldingEventBus();
    $listened = $bus->listen(function(Scaffolding\Pen $p): void { $p->write(); });
    assert($listened instanceof Scaffolding\Pen, 'listen(fn(Scaffolding\Pen $p)) must return Scaffolding\Pen (T inferred from closure param)');

    $listenedUser = $bus->listen(function(Scaffolding\User $u): void { $u->getEmail(); });
    assert($listenedUser instanceof Scaffolding\User, 'listen(function(Scaffolding\User $u)) must return Scaffolding\User');

    // ── Scaffolding\ScaffoldingBatchProcessor::process() — second closure param ─────
    $proc = new Scaffolding\ScaffoldingBatchProcessor();
    $processed = $proc->process(function(int $i, Scaffolding\Pencil $p): void { $p->sketch(); });
    assert($processed instanceof Scaffolding\Pencil, 'process(fn(int, Scaffolding\Pencil)) must return Scaffolding\Pencil (T from position 1)');

    // ── Nested generic: Scaffolding\ServiceLocator::wrap → Scaffolding\Box::unwrap ──────────────
    $boxed = $locator->wrap(Scaffolding\Pen::class);
    assert($boxed instanceof Scaffolding\Box, 'Scaffolding\ServiceLocator::wrap() must return Scaffolding\Box');
    $unboxed = $boxed->unwrap();
    assert($unboxed instanceof Scaffolding\Pen, 'Scaffolding\Box::unwrap() must return Scaffolding\Pen (from wrap(Scaffolding\Pen::class))');

    // ── __invoke() return types ─────────────────────────────────────────
    $formatter = new Scaffolding\ScaffoldingFormatter();
    $invoked = $formatter();
    assert($invoked instanceof Scaffolding\Pen, 'Scaffolding\ScaffoldingFormatter::__invoke() must return Scaffolding\Pen');

    $factory = new Scaffolding\ScaffoldingPenFactory();
    $factoryResult = $factory();
    assert($factoryResult instanceof Scaffolding\Pen, 'Scaffolding\ScaffoldingPenFactory::__invoke() must return Scaffolding\Pen');

    // ── Enum from() ─────────────────────────────────────────────────────
    $active = Scaffolding\Status::from('active');
    assert($active instanceof Scaffolding\Status, 'Scaffolding\Status::from() must return Scaffolding\Status');
    assert($active === Scaffolding\Status::Active, 'Scaffolding\Status::from("active") must be Scaffolding\Status::Active');

    // ── Clone preserves type ────────────────────────────────────────────
    $original = new Scaffolding\Pen('blue');
    $copy = clone $original;
    assert($copy instanceof Scaffolding\Pen, 'clone must preserve Scaffolding\Pen type');
    assert($copy !== $original, 'clone must be a different instance');

    // ── class-string variable → new $var ────────────────────────────────
    $cls = Scaffolding\Pen::class;
    $fromClassString = new $cls();
    assert($fromClassString instanceof Scaffolding\Pen, 'new $cls where $cls = Scaffolding\Pen::class must be Scaffolding\Pen');

    // ── Scaffolding\Zoo: inheritance, traits, promoted properties ────────────────────
    $zoo = new Scaffolding\Zoo();
    assert($zoo instanceof Scaffolding\Zoo, 'new Scaffolding\Zoo() must be Scaffolding\Zoo');
    assert($zoo instanceof Scaffolding\ZooBase, 'Scaffolding\Zoo must extend Scaffolding\ZooBase');
    assert(method_exists($zoo, 'aardvark'), 'Scaffolding\Zoo must have own method aardvark()');
    assert(method_exists($zoo, 'dingo'), 'Scaffolding\Zoo must have trait method dingo()');
    assert(method_exists($zoo, 'elephant'), 'Scaffolding\Zoo must have trait method elephant()');
    assert(method_exists($zoo, 'falcon'), 'Scaffolding\Zoo must have inherited method falcon()');

    // @property and @method via __get/__call
    assert($zoo->gorilla === 'gorilla-value', '@property $gorilla must work via __get');
    assert($zoo->iguana === 'iguana-value', '@property-read $iguana (Scaffolding\ZooContract) must work via __get');
    assert($zoo->hyena('x') === true, '@method hyena() must work via __call');
    assert($zoo->jaguar() === 'jaguar-value', '@method jaguar() (Scaffolding\ZooContract) must work via __call');

    // Visibility: protected/private must not be accessible
    assert(property_exists($zoo, 'baboon'), 'Scaffolding\Zoo must have public $baboon');
    assert((new \ReflectionProperty($zoo, 'keeper'))->isProtected(), '$keeper must be protected');
    assert((new \ReflectionProperty($zoo, 'ceo'))->isPrivate(), '$ceo must be private');
    assert((new \ReflectionMethod($zoo, 'nocturnal'))->isPrivate(), 'nocturnal() must be private');

    // ── Expression types: null-coalescing ────────────────────────────────
    $src = new Scaffolding\ScaffoldingExpressionType();
    $fallback = $src->backup ?? $src->primary;
    assert($fallback instanceof Scaffolding\Response, 'Null-coalescing must resolve to Scaffolding\Response');

    // ── ChainingDemo scaffolding ────────────────────────────────────────
    $studio = new Scaffolding\ScaffoldingChainingDemo();
    assert($studio->brush instanceof Scaffolding\Brush, 'Scaffolding\ScaffoldingChainingDemo::$brush must be Scaffolding\Brush');
    assert($studio->canvas instanceof Scaffolding\Canvas, 'Scaffolding\ScaffoldingChainingDemo::$canvas must be Scaffolding\Canvas');

    // ── Trait conflict resolution ───────────────────────────────────────
    $tc = new TraitConflictDemo();
    assert(method_exists($tc, 'serialize'), 'TraitConflictDemo must have serialize()');
    assert(method_exists($tc, 'toJson'), 'TraitConflictDemo must have toJson()');
    assert(method_exists($tc, 'toXml'), 'TraitConflictDemo must have toXml()');

    // ── Scaffolding\AdminUser extends Scaffolding\User extends Scaffolding\Model ────────────────────────────
    $admin = new Scaffolding\AdminUser('Admin', 'admin@example.com');
    assert($admin instanceof Scaffolding\AdminUser, 'new Scaffolding\AdminUser() must be Scaffolding\AdminUser');
    assert($admin instanceof Scaffolding\User, 'Scaffolding\AdminUser must extend Scaffolding\User');
    assert($admin instanceof Scaffolding\Model, 'Scaffolding\AdminUser must extend Scaffolding\Model (via Scaffolding\User)');

    // ── ClassFilteringDemo extends Scaffolding\Model implements Scaffolding\Renderable ───────────
    $cfd = new ClassFilteringDemo();
    assert($cfd instanceof Scaffolding\Model, 'ClassFilteringDemo must extend Scaffolding\Model');
    assert($cfd instanceof Scaffolding\Renderable, 'ClassFilteringDemo must implement Scaffolding\Renderable');

    // ── ReflectionClass<T> instantiation ────────────────────────────────
    $widget = (new ReflectionInstantiationDemo())->build(Scaffolding\ReflectedWidget::class);
    assert($widget instanceof Scaffolding\ReflectedWidget, 'ReflectionClass::newInstance() must be Scaffolding\ReflectedWidget');

    // ── Reflected property read ─────────────────────────────────────────
    $holder = new Scaffolding\ReflectedHolder($widget);
    $reflectedWidget = (new ReflectionInstantiationDemo())->reflectedWidget($holder);
    assert($reflectedWidget instanceof Scaffolding\ReflectedWidget, 'ReflectionProperty::getValue() must be Scaffolding\ReflectedWidget');
    assert((new ReflectionInstantiationDemo())->reflectedWidget(new Scaffolding\ReflectedHolder()) === null, 'A null private property must read back as null');
    $directWidget = (new ReflectionInstantiationDemo())->directWidget($holder);
    assert($directWidget === $widget, 'new \ReflectionProperty() must read the same property getProperty() does');
    $accessedWidget = (new ReflectionInstantiationDemo())->accessedWidget($holder);
    assert($accessedWidget instanceof Scaffolding\ReflectedWidget, 'An accessor must return what its arguments name');
    assert(ReflectionInstantiationDemo::fetchProperty($holder, 'widget') === $widget, 'fetchProperty() must read the property the name argument gives');

    // ── Inline new chaining ─────────────────────────────────────────────
    $fromNew = (new Scaffolding\Canvas())->getBrush();
    assert($fromNew instanceof Scaffolding\Brush, '(new Scaffolding\Canvas())->getBrush() must be Scaffolding\Brush');

    // ── Parenthesized assignment ────────────────────────────────────────
    $parenPen = (new Scaffolding\Pen('red'));
    assert($parenPen instanceof Scaffolding\Pen, 'Parenthesized new must still be Scaffolding\Pen');

    // ── Constructor @param override (ParamOverrideDemo) ─────────────────
    $ingredient = new Scaffolding\Ingredient();
    assert($ingredient instanceof Scaffolding\Ingredient, 'new Scaffolding\Ingredient() must be Scaffolding\Ingredient');
    assert(property_exists($ingredient, 'name'), 'Scaffolding\Ingredient must have $name');

    $recipe = new Scaffolding\Recipe('Test', [new Scaffolding\Ingredient()]);
    assert($recipe instanceof Scaffolding\Recipe, 'new Scaffolding\Recipe() must be Scaffolding\Recipe');

    // ── Inline @var on promoted property (InlineVarPromotedDemo) ────────
    $inlineDemo = new InlineVarPromotedDemo([new Scaffolding\Ingredient()]);
    assert(is_array($inlineDemo->ingredients), 'InlineVarPromotedDemo->ingredients must be array');
    assert($inlineDemo->ingredients[0] instanceof Scaffolding\Ingredient, 'InlineVarPromotedDemo->ingredients[0] must be Scaffolding\Ingredient');

    // ── Scaffolding\Container / Scaffolding\app() conditional return types ──────────────────────
    $container = new Scaffolding\Container();
    $containerPen = $container->make(Scaffolding\Pen::class);
    assert($containerPen instanceof Scaffolding\Pen, 'Scaffolding\Container::make(Scaffolding\Pen::class) must return Scaffolding\Pen');

    $appPen = Scaffolding\app(Scaffolding\Pen::class);
    assert($appPen instanceof Scaffolding\Pen, 'Scaffolding\app(Scaffolding\Pen::class) must return Scaffolding\Pen');

    $appSelf = Scaffolding\app();
    assert($appSelf instanceof Scaffolding\Container, 'Scaffolding\app() with no args must return Scaffolding\Container');

    // ── `never` branch of a conditional return type ─────────────────────
    $maybePen = Scaffolding\pickPenOrNull();
    Scaffolding\throwUnless($maybePen, 'no pen');
    assert($maybePen instanceof Scaffolding\Pen,
        'a value that got past Scaffolding\throwUnless() cannot be the falsy half');
    $throwUnlessRejected = false;
    try {
        Scaffolding\throwUnless(null, 'no pen');
    } catch (\RuntimeException) {
        $throwUnlessRejected = true;
    }
    assert($throwUnlessRejected, 'Scaffolding\throwUnless(null) throws, so `never` is the honest return type');

    // ── Named-argument conditional return type ──────────────────────────
    $mapper = new Scaffolding\TreeMapperImpl();
    $named = $mapper->map(source: 'bar', signature: 'foo');
    assert($named instanceof Scaffolding\Pen, 'out-of-order named args bind $signature="foo" → Scaffolding\Pen');

    // ── Conditional branch selecting `mixed` flows through ──────────────
    $unknown = Scaffolding\sessionValue('file');
    assert(is_string($unknown), 'Scaffolding\sessionValue("file") returns a mixed value narrowable to string');
    assert(Scaffolding\sessionValue() === null, 'Scaffolding\sessionValue() with no args returns null');

    // ── Type-keyed conditional return type (PenDrawer) ──────────────────
    $drawer = new Scaffolding\PenDrawer();
    assert($drawer->find(7) instanceof Scaffolding\Pen, 'a single id finds one pen');
    assert($drawer->find([7, 8])->first() instanceof Scaffolding\Pen, 'a list of ids finds a collection of pens');
    assert(is_string($drawer->label('lid')), 'a named label is a string');
    assert(is_array($drawer->label()), 'no name asks for every label');

    // ── Value-keyed conditional return type (str_word_count) ────────────
    assert(str_word_count('two words') === 2, 'the default $format of 0 counts the words');
    assert(str_word_count('two words', 1) === ['two', 'words'], 'format 1 lists the words');
    assert(str_word_count('two words', 2) === [0 => 'two', 4 => 'words'], 'format 2 keys the words by offset');

    // ── Subject-keyed conditional return type (the replace family) ───────
    assert(str_replace('a', 'b', 'banana') === 'bbnbnb', 'a string subject replaces into a string');
    assert(str_replace('a', 'b', ['banana', 'apple']) === ['bbnbnb', 'bpple'], 'an array subject replaces into an array');

    // ── Other ordinary subject spellings (the replace family) ────────────
    /** @var array{message: string} $shapeData */
    $shapeData = ['message' => 'banana'];
    assert(str_replace('a', 'b', $shapeData['message']) === 'bbnbnb', 'an array-shape element subject replaces into a string');
    assert(is_string(str_replace('.', '-', PHP_VERSION)), 'a global constant subject replaces into a string');
    $maybeBody = null;
    assert(str_replace('a', 'b', $maybeBody ?: 'banana') === 'bbnbnb', 'an elvis-operator subject replaces into a string');

    assert(preg_replace('/\d/', '*', 'a1b2') === 'a*b*', 'a string subject is replaced into a string');
    assert(preg_replace('/\d/', '*', ['a1', 'b2']) === ['a*', 'b*'], 'an array subject is replaced into an array');
    assert(substr_replace('banana', 'x', 0, 1) === 'xanana', 'a string subject is spliced into a string');

    // ── Capture-group shape (preg_match) ─────────────────────────────────
    assert(preg_match('/(?<amount>\d+)(?<unit>\w*)/', '12kg', $size) === 1, 'the pattern matches the subject');
    assert($size[0] === '12kg', 'key 0 holds the whole match');
    assert($size['unit'] === 'kg', 'a named group is stored under its name');
    assert($size[1] === '12' && $size[2] === 'kg', 'every group is stored under its number too');
    assert(preg_match('/(\d+)/', 'kg', $noMatch) === 0, 'a pattern that does not match returns 0');
    assert($noMatch === [], 'a failed match leaves the empty array behind, so none of the keys are there');
    $matched = preg_match('/(?<port>\d+)/', 'host:8080', $address);
    assert($matched === 1, 'storing the result loses nothing: the call still reports the match');
    assert($address['port'] === '8080', 'and it still fills the array in, whether or not the result was stored');
    preg_match_all('/(\d+)/', '1, 2, 3', $numbers);
    assert($numbers[1] === ['1', '2', '3'], 'matching all collects every match of a group under its number');
    preg_match_all('/(\d+)/', 'none', $noNumbers);
    assert($noNumbers === [[], []], 'matching all still writes one empty list per group when nothing matches');

    // ── Flag-keyed return type (json_encode + JSON_THROW_ON_ERROR) ───────
    assert(json_encode(['ok' => true], JSON_THROW_ON_ERROR) === '{"ok":true}', 'the flag makes the failure branch a JsonException instead of false');
    assert(ConditionalReturnDemo::JSON_FLAGS === JSON_THROW_ON_ERROR, 'a constant holding the flag holds its value');
    assert((ConditionalReturnDemo::JSON_COMBO & JSON_THROW_ON_ERROR) !== 0, 'a constant OR-ing the flag with another still sets its bit');
    assert(json_encode(['ok' => true], ConditionalReturnDemo::JSON_FLAGS) === '{"ok":true}', 'the flag counts when it is reached through a constant');
    $jsonMask = JSON_UNESCAPED_SLASHES | ConditionalReturnDemo::JSON_FLAGS;
    assert(json_encode(['url' => 'a/b'], $jsonMask) === '{"url":"a/b"}', 'a mask kept in a variable sets the same bits');
    assert((ConditionalReturnDemo::JSON_TYPED & JSON_THROW_ON_ERROR) !== 0, 'a declared type does not replace the value the constant holds');
    assert(json_encode(['ok' => true], ConditionalReturnDemo::JSON_TYPED) === '{"ok":true}', 'the flag counts when the constant is typed');

    // ── More argument-decided builtins ───────────────────────────────────
    assert(is_array(pathinfo('/tmp/report.csv')), 'the all-elements form returns the component array');
    assert(pathinfo('/tmp/report.csv', PATHINFO_FILENAME) === 'report', 'a single component comes back as a string');
    assert(is_string(print_r(['a' => 1], true)), 'print_r renders to a string when asked to');
    assert(print_r('x') === true, 'print_r reports that it printed otherwise');
    assert(is_float(microtime(true)), 'microtime(true) is a float');
    assert(is_string(microtime()), 'microtime() is a string');
    assert(is_int(hrtime(true)) || is_float(hrtime(true)), 'hrtime(true) is a number');
    assert(is_array(hrtime()), 'hrtime() is the [seconds, nanoseconds] pair');
    assert(is_array(getenv()), 'getenv() with no name returns the whole environment');
    assert(is_string(getenv('PATH')), 'naming a variable returns its value');
    assert(abs(-7) === 7, 'abs of an int is an int');
    assert(abs(-7.5) === 7.5, 'abs of a float is a float');
    assert(is_string(var_export(['a' => 1], true)), 'var_export renders to a string when asked to');
    assert(var_export('x') === null, 'var_export returns nothing when it prints');
    assert(is_string(mb_internal_encoding()), 'mb_internal_encoding with no argument reads the encoding');
    assert(is_bool(mb_internal_encoding('UTF-8')), 'naming an encoding reports whether the write took');
    assert(is_int(version_compare('8.2.0', '8.1.0')), 'version_compare with no operator orders the versions');
    assert(is_bool(version_compare('8.2.0', '8.1.0', '>')), 'naming an operator asks a yes/no question');
    assert(sscanf('12 apples', '%d %s') === [12, 'apples'], 'sscanf with no targets collects into an array');
    assert(sscanf('12 apples', '%d %s', $scanCount, $scanFruit) === 2, 'passing targets reports how many were filled');
    assert(range(0, 10, 2) === [0, 2, 4, 6, 8, 10], 'an all-integer range stays integral');
    assert(range(0, 1, 0.25) === [0.0, 0.25, 0.5, 0.75, 1.0], 'a fractional step makes every element a float');
    assert(array_reduce([1, 2, 3], static fn (int $carry, int $n): int => $carry + $n, 0) === 6, 'a seeded reduce is never null');
    assert(array_reduce([], static fn ($carry, $n) => $carry) === null, 'an empty array with no initial value is the one null case');
    assert(pow(2, 10) === 1024, 'two numbers raise to a number');
    assert(is_string(ini_get('memory_limit')), 'a core directive is always set');
    assert(get_class(new Scaffolding\Pen()) === Scaffolding\Pen::class, 'get_class names the class of the object it was handed');

    // ── Closure / arrow function return types ───────────────────────────
    $makePenClosure = function(): Scaffolding\Pen { return new Scaffolding\Pen(); };
    assert($makePenClosure() instanceof Scaffolding\Pen, 'Closure returning Scaffolding\Pen must return Scaffolding\Pen');

    $makePencilArrow = fn(): Scaffolding\Pencil => new Scaffolding\Pencil();
    assert($makePencilArrow() instanceof Scaffolding\Pencil, 'Arrow fn returning Scaffolding\Pencil must return Scaffolding\Pencil');

    $builder = function(): Scaffolding\Pen { return new Scaffolding\Pen(); };
    $chained = $builder()->rename('Bold');
    assert($chained instanceof Scaffolding\Pen, 'Closure()->rename() must chain to Scaffolding\Pen');

    // ── Closure members ─────────────────────────────────────────────────
    $typedClosure = function(Scaffolding\Pen $pen): string { return $pen->write(); };
    assert(method_exists($typedClosure, 'bindTo'), 'Closure must have bindTo()');
    assert(method_exists($typedClosure, 'call'), 'Closure must have call()');
    assert($typedClosure instanceof \Closure, 'Function expression must be Closure');

    $typedArrow = fn(int $x): float => $x * 1.5;
    assert($typedArrow instanceof \Closure, 'Arrow function must be Closure');

    // ── Enum methods and properties ─────────────────────────────────────
    $activeStatus = Scaffolding\Status::Active;
    assert($activeStatus instanceof Scaffolding\Status, 'Scaffolding\Status::Active must be Scaffolding\Status');
    assert($activeStatus->name === 'Active', 'Scaffolding\Status::Active->name must be "Active"');
    assert($activeStatus->value === 'active', 'Scaffolding\Status::Active->value must be "active"');
    assert($activeStatus->label() === 'Active', 'Scaffolding\Status::Active->label() must return "Active"');
    assert($activeStatus->isActive() === true, 'Scaffolding\Status::Active->isActive() must be true');

    $pending = Scaffolding\Status::Pending;
    assert($pending->isActive() === false, 'Scaffolding\Status::Pending->isActive() must be false');

    $high = Scaffolding\Priority::High;
    assert($high instanceof Scaffolding\Priority, 'Scaffolding\Priority::High must be Scaffolding\Priority');
    assert($high->name === 'High', 'Scaffolding\Priority::High->name must be "High"');
    assert($high->value === 3, 'Scaffolding\Priority::High->value must be 3');

    $manual = Scaffolding\Mode::Manual;
    assert($manual instanceof Scaffolding\Mode, 'Scaffolding\Mode::Manual must be Scaffolding\Mode');
    assert($manual->name === 'Manual', 'Scaffolding\Mode::Manual->name must be "Manual"');

    // cases() returns a list of the enum's own instances; indexing it
    // inline resolves the element back to the enum.
    assert(Scaffolding\Status::cases()[0] instanceof Scaffolding\Status, 'Scaffolding\Status::cases()[0] must be a Scaffolding\Status');
    assert(Scaffolding\Status::cases()[0]->value === 'active', 'Scaffolding\Status::cases()[0]->value must be "active"');
    assert(Scaffolding\Priority::cases()[0]->name === 'Low', 'Scaffolding\Priority::cases()[0]->name must be "Low"');

    $fromString = Scaffolding\Status::from('active');
    assert($fromString === Scaffolding\Status::Active, 'Scaffolding\Status::from("active") must be Scaffolding\Status::Active');

    $tryFrom = Scaffolding\Status::tryFrom('nonexistent');
    assert($tryFrom === null, 'Scaffolding\Status::tryFrom("nonexistent") must be null');

    $defaultVal = Scaffolding\Status::defaultValue();
    assert($defaultVal === 'active', 'Scaffolding\Status::defaultValue() must return "active" (self::Active->value)');

    // ── Scaffolding\Response methods ────────────────────────────────────────────────
    $response = new Scaffolding\Response(200, 'OK');
    assert($response->getStatusCode() === 200, 'Scaffolding\Response::getStatusCode() must return 200');
    assert($response->getBody() === 'OK', 'Scaffolding\Response::getBody() must return "OK"');
    assert($response->isSuccess() === true, 'Scaffolding\Response(200) must be success');

    $errResponse = new Scaffolding\Response(500);
    assert($errResponse->isSuccess() === false, 'Scaffolding\Response(500) must not be success');

    // ── Scaffolding\UserProfile methods ─────────────────────────────────────────────
    $userForProfile = new Scaffolding\User('Eve', 'eve@example.com');
    $prof = $userForProfile->getProfile();
    assert($prof instanceof Scaffolding\UserProfile, 'Scaffolding\User::getProfile() must return Scaffolding\UserProfile');
    assert(method_exists($prof, 'getDisplayName'), 'Scaffolding\UserProfile must have getDisplayName()');
    assert(method_exists($prof, 'setBio'), 'Scaffolding\UserProfile must have setBio()');
    $bioResult = $prof->setBio('Hello');
    assert($bioResult instanceof Scaffolding\UserProfile, 'Scaffolding\UserProfile::setBio() returns static');

    // ── Generator yield types ───────────────────────────────────────────
    $genDemo = new GeneratorDemo();
    $gen = $genDemo->getPens();
    assert($gen instanceof \Generator, 'getPens() must return Generator');
    foreach ($gen as $genPen) {
        assert($genPen instanceof Scaffolding\Pen, 'Generator<int, Scaffolding\Pen> must yield Scaffolding\Pen');
        break;
    }

    $pencilGen = $genDemo->processPencils();
    foreach ($pencilGen as $genPencil) {
        assert($genPencil instanceof Scaffolding\Pencil, 'Generator<int, Scaffolding\Pencil, mixed, Scaffolding\Pen> must yield Scaffolding\Pencil');
        break;
    }

    // ── Generator yield inference (GeneratorYieldDemo) ───────────────────
    $yieldDemo = new GeneratorYieldDemo();
    foreach ($yieldDemo->findAll() as $yieldedPen) {
        assert($yieldedPen instanceof Scaffolding\Pen, 'GeneratorYieldDemo::findAll() must yield Scaffolding\Pen');
        break;
    }
    foreach ($yieldDemo->chainingThroughYieldInferred() as $chainPen) {
        assert($chainPen instanceof Scaffolding\Pen, 'chainingThroughYieldInferred() must yield Scaffolding\Pen');
        break;
    }
    $coroutineGen = $yieldDemo->coroutine();
    $yielded = $coroutineGen->current();
    assert($yielded === 'ready', 'coroutine() must yield string (TValue)');
    $coroutineGen->send(new Scaffolding\Pencil());

    // ── GenericContext: Scaffolding\Box<Scaffolding\Gift> and Scaffolding\TypedCollection<int, Scaffolding\Gift> ─────────
    $gcSrc = new Scaffolding\ScaffoldingGenericContext();
    $unwrapped = $gcSrc->chest->unwrap();
    assert($unwrapped instanceof Scaffolding\Gift, 'Scaffolding\Box<Scaffolding\Gift>::unwrap() must return Scaffolding\Gift');
    $displayFirst = $gcSrc->display()->first();
    assert($displayFirst instanceof Scaffolding\Gift, 'Scaffolding\TypedCollection<int, Scaffolding\Gift>::first() must return Scaffolding\Gift');

    // ── CompoundNegatedNarrowing ────────────────────────────────────────
    $compoundRock = new Scaffolding\Rock();
    $compoundDemo = new CompoundNegatedNarrowingDemo();
    // Scaffolding\Rock passes both negated checks (is Scaffolding\Rock, is not "not Scaffolding\Rock")
    // so it doesn't return early — weigh() must exist
    assert(method_exists($compoundRock, 'weigh'), 'Scaffolding\Rock must have weigh()');
    $compoundBanana = new Scaffolding\Banana();
    assert(method_exists($compoundBanana, 'weigh'), 'Scaffolding\Banana must have weigh()');
    // Scaffolding\Lamp would cause the early return — verify it lacks weigh()
    assert(!method_exists(new Scaffolding\Lamp(), 'weigh'), 'Scaffolding\Lamp must NOT have weigh()');

    // ── InArrayNarrowing ────────────────────────────────────────────────
    $rockList = [new Scaffolding\Rock()];
    $testRock = new Scaffolding\Rock();
    assert(in_array($testRock, $rockList, true) === false, 'Different Scaffolding\Rock instances are not strictly identical');
    $sameRock = $rockList[0];
    assert(in_array($sameRock, $rockList, true) === true, 'Same Scaffolding\Rock instance must be in_array strict');

    // ── MatchClassStringDemo: class-string through match → Scaffolding\Container ────
    $mcsContainer = new Scaffolding\Container();
    $mcsType = match (0) {
        0 => Scaffolding\ElasticProductReviewIndexService::class,
        1 => Scaffolding\ElasticBrandIndexService::class,
    };
    $mcsResult = $mcsContainer->make($mcsType);
    assert($mcsResult instanceof Scaffolding\ElasticProductReviewIndexService,
        'Scaffolding\Container::make(match class-string) must return the matched class');
    assert(method_exists($mcsResult, 'index'), 'Match-resolved instance must have index()');

    $mcsCls = Scaffolding\Pen::class;
    $mcsPen = $mcsContainer->make($mcsCls);
    assert($mcsPen instanceof Scaffolding\Pen, 'Scaffolding\Container::make(Scaffolding\Pen::class via variable) must return Scaffolding\Pen');

    $mcsTernary = true ? Scaffolding\Pen::class : Scaffolding\Pencil::class;
    $mcsObj = $mcsContainer->make($mcsTernary);
    assert($mcsObj instanceof Scaffolding\Pen, 'Scaffolding\Container::make(ternary class-string) must return Scaffolding\Pen');

    // ── ExceptionDemo: exception hierarchy ──────────────────────────────
    assert(is_subclass_of(Scaffolding\NotFoundException::class, \RuntimeException::class),
        'Scaffolding\NotFoundException must extend RuntimeException');
    assert(is_subclass_of(Scaffolding\ValidationException::class, \RuntimeException::class),
        'Scaffolding\ValidationException must extend RuntimeException');
    assert(is_subclass_of(Scaffolding\AuthorizationException::class, \RuntimeException::class),
        'Scaffolding\AuthorizationException must extend RuntimeException');

    try {
        throw new Scaffolding\ValidationException('test');
    } catch (Scaffolding\ValidationException $e) {
        assert($e instanceof Scaffolding\ValidationException, 'Caught exception must be Scaffolding\ValidationException');
        assert($e->getMessage() === 'test', 'Exception message must propagate');
    }

    // ── Closure parameter inference ─────────────────────────────────────
    $closureSrc = new Scaffolding\ScaffoldingClosureParamInference();
    $closureReceived = [];
    $closureSrc->items->each(function ($pen) use (&$closureReceived) {
        assert($pen instanceof Scaffolding\Pen, 'Closure param from Scaffolding\FluentCollection<int, Scaffolding\Pen>::each() must be Scaffolding\Pen');
        $closureReceived[] = $pen;
    });
    assert(count($closureReceived) === 2, 'each() must invoke callback for every item');

    // Function-level @template callable inference (array_any pattern)
    $tplHolder = new Scaffolding\ScaffoldingTemplateCallableHolder();
    $tplHolder->tools = [new Scaffolding\Pen('red'), new Scaffolding\Pen('blue')];
    $tplResult = array_any($tplHolder->tools, fn($t) => $t->color() === 'red');
    assert($tplResult === true, 'array_any with template callable must work');

    // ── Type alias resolution ───────────────────────────────────────────
    $aliasDemo = new TypeAliasDemo();
    $userData = $aliasDemo->getUserData();
    assert(is_string($userData['name']), 'UserData["name"] must be string');
    assert($userData['pen'] instanceof Scaffolding\Pen, 'UserData["pen"] must be Scaffolding\Pen');

    $statusInfo = $aliasDemo->getStatus();
    assert(is_int($statusInfo['code']), 'StatusInfo["code"] must be int');
    assert($statusInfo['owner'] instanceof Scaffolding\User, 'StatusInfo["owner"] must be Scaffolding\User');

    $importDemo = new TypeAliasImportDemo();
    $imported = $importDemo->fetchUser();
    assert($imported['pen'] instanceof Scaffolding\Pen, 'Imported UserData["pen"] must be Scaffolding\Pen');
    $importedStatus = $importDemo->fetchStatus();
    assert($importedStatus['owner'] instanceof Scaffolding\User, 'Imported StatusInfo["owner"] must be Scaffolding\User');

    // ── String interpolation ────────────────────────────────────────────
    $interpPen = new Scaffolding\Pen('blue');
    ob_start();
    echo "Ink is {$interpPen->color()}";
    $braceOutput = ob_get_clean();
    assert($braceOutput === 'Ink is blue', 'Brace interpolation must call method: got ' . $braceOutput);

    ob_start();
    echo "Tool: $interpPen->ink";
    $simpleOutput = ob_get_clean();
    assert($simpleOutput === 'Tool: blue', 'Simple interpolation must access property: got ' . $simpleOutput);

    ob_start();
    echo 'no $interpPen-> here';
    $singleOutput = ob_get_clean();
    assert($singleOutput === 'no $interpPen-> here', 'Single-quoted must stay literal: got ' . $singleOutput);

    // ── Diagnostics: class/method/property existence ────────────────────
    // These verify the claims made by the UnknownMemberDemo and related demos.
    assert(class_exists(Scaffolding\User::class), 'Scaffolding\User class must exist');
    assert(class_exists(Scaffolding\Pen::class), 'Scaffolding\Pen class must exist');
    assert(class_exists(Scaffolding\Model::class), 'Scaffolding\Model class must exist');
    assert(class_exists(Scaffolding\AdminUser::class), 'Scaffolding\AdminUser class must exist');
    assert(interface_exists(Scaffolding\Renderable::class), 'Scaffolding\Renderable interface must exist');
    assert(trait_exists(Scaffolding\HasTimestamps::class), 'Scaffolding\HasTimestamps trait must exist');
    assert(trait_exists(Scaffolding\HasSlug::class), 'Scaffolding\HasSlug trait must exist');
    assert(enum_exists(Scaffolding\Status::class), 'Scaffolding\Status enum must exist');
    assert(enum_exists(Scaffolding\Priority::class), 'Scaffolding\Priority enum must exist');
    assert(
        Scaffolding\Status::Pending->canChangeTo(Scaffolding\Status::Active),
        'A self parameter accepts another case of the declaring enum'
    );
    assert(
        !Scaffolding\Status::Active->canChangeTo(Scaffolding\Status::Active),
        'canChangeTo() rejects a transition to the current case'
    );

    // Scaffolding\User members that demos reference
    assert(method_exists(Scaffolding\User::class, 'getEmail'), 'Scaffolding\User must have getEmail()');
    assert(method_exists(Scaffolding\User::class, 'getName'), 'Scaffolding\User must have getName() (inherited)');
    assert(method_exists(Scaffolding\User::class, 'getProfile'), 'Scaffolding\User must have getProfile()');
    assert(method_exists(Scaffolding\User::class, 'getStatus'), 'Scaffolding\User must have getStatus()');
    assert(method_exists(Scaffolding\User::class, 'setName'), 'Scaffolding\User must have setName() (inherited)');
    assert(method_exists(Scaffolding\User::class, 'findByEmail'), 'Scaffolding\User must have static findByEmail()');
    assert(method_exists(Scaffolding\User::class, 'hashPassword'), 'Scaffolding\User must have static hashPassword()');
    assert(property_exists(Scaffolding\User::class, 'email'), 'Scaffolding\User must have $email');
    assert(property_exists(Scaffolding\User::class, 'defaultRole'), 'Scaffolding\User must have static $defaultRole');

    // UnknownMemberDemo: nonexistentMethod must NOT exist
    assert(!method_exists(Scaffolding\User::class, 'nonexistentMethod'), 'Scaffolding\User must NOT have nonexistentMethod()');

    // Scaffolding\Pen members
    assert(method_exists(Scaffolding\Pen::class, 'write'), 'Scaffolding\Pen must have write()');
    assert(method_exists(Scaffolding\Pen::class, 'color'), 'Scaffolding\Pen must have color()');
    assert(method_exists(Scaffolding\Pen::class, 'label'), 'Scaffolding\Pen must have label()');
    assert(method_exists(Scaffolding\Pen::class, 'rename'), 'Scaffolding\Pen must have rename()');
    assert(method_exists(Scaffolding\Pen::class, 'make'), 'Scaffolding\Pen must have static make()');

    // Scaffolding\Marker extends Scaffolding\Pen
    assert(method_exists(Scaffolding\Marker::class, 'highlight'), 'Scaffolding\Marker must have highlight()');
    assert(method_exists(Scaffolding\Marker::class, 'write'), 'Scaffolding\Marker must inherit write() from Scaffolding\Pen');

    // Scaffolding\Pencil members
    assert(method_exists(Scaffolding\Pencil::class, 'sketch'), 'Scaffolding\Pencil must have sketch()');
    assert(method_exists(Scaffolding\Pencil::class, 'sharpen'), 'Scaffolding\Pencil must have sharpen()');

    // Scaffolding\Rock and Scaffolding\Banana members (narrowing demos rely on these)
    assert(method_exists(Scaffolding\Rock::class, 'crush'), 'Scaffolding\Rock must have crush()');
    assert(method_exists(Scaffolding\Rock::class, 'weigh'), 'Scaffolding\Rock must have weigh()');
    assert(!method_exists(Scaffolding\Rock::class, 'peel'), 'Scaffolding\Rock must NOT have peel()');
    assert(method_exists(Scaffolding\Banana::class, 'peel'), 'Scaffolding\Banana must have peel()');
    assert(method_exists(Scaffolding\Banana::class, 'weigh'), 'Scaffolding\Banana must have weigh()');
    assert(!method_exists(Scaffolding\Banana::class, 'crush'), 'Scaffolding\Banana must NOT have crush()');

    // ── Array functions preserve types ───────────────────────────────────
    $penArray = [new Scaffolding\Pen('red'), new Scaffolding\Pen('blue'), new Scaffolding\Pen('green')];
    $filtered = array_filter($penArray, fn(Scaffolding\Pen $p) => $p->color() === 'blue');
    assert(count($filtered) === 1, 'array_filter must filter correctly');
    assert(reset($filtered) instanceof Scaffolding\Pen, 'array_filter must preserve Scaffolding\Pen type');

    $vals = array_values($penArray);
    assert($vals[0] instanceof Scaffolding\Pen, 'array_values must preserve Scaffolding\Pen type');

    $popped = array_pop($penArray);
    assert($popped instanceof Scaffolding\Pen, 'array_pop must return Scaffolding\Pen');

    $penArray2 = [new Scaffolding\Pen('a'), new Scaffolding\Pen('b')];
    $cur = current($penArray2);
    assert($cur instanceof Scaffolding\Pen, 'current() must return Scaffolding\Pen');

    $last = end($penArray2);
    assert($last instanceof Scaffolding\Pen, 'end() must return Scaffolding\Pen');

    $reduced = array_reduce($penArray2, function(Scaffolding\Pen $carry, Scaffolding\Pen $item): Scaffolding\Pen {
        return $carry;
    }, new Scaffolding\Pen('merged'));
    assert($reduced instanceof Scaffolding\Pen, 'array_reduce must return type of initial value');

    $sum = array_sum([10, 20, 30]);
    assert(is_int($sum) || is_float($sum), 'array_sum must return int or float');

    $product = array_product([2, 3, 4]);
    assert(is_int($product) || is_float($product), 'array_product must return int or float');

    // Inline, without an intermediate variable — the same element type.
    $penArray3 = [new Scaffolding\Pen('x'), new Scaffolding\Pen('y')];
    assert(array_values($penArray3)[0] instanceof Scaffolding\Pen, 'array_values must preserve Scaffolding\Pen inline');
    assert(array_map(fn(Scaffolding\Pen $p): Scaffolding\Pen => $p, $penArray3)[0] instanceof Scaffolding\Pen,
        'array_map must preserve Scaffolding\Pen inline');

    $penIterator = new \ArrayIterator([new Scaffolding\Pen('i'), new Scaffolding\Pen('j')]);
    assert(iterator_to_array($penIterator)[0] instanceof Scaffolding\Pen,
        'iterator_to_array must preserve Scaffolding\Pen inline');
    assert(array_values(iterator_to_array($penIterator))[0] instanceof Scaffolding\Pen,
        'a nested array function call must preserve Scaffolding\Pen');

    // ── Match expression types ──────────────────────────────────────────
    $matchResult = match (0) {
        0 => new Scaffolding\ElasticProductReviewIndexService(),
        1 => new Scaffolding\ElasticBrandIndexService(),
    };
    assert($matchResult instanceof Scaffolding\ElasticProductReviewIndexService
        || $matchResult instanceof Scaffolding\ElasticBrandIndexService,
        'Match expression must return one of the branch types');
    assert(method_exists($matchResult, 'index'), 'Match result must have shared index() method');

    // ── Ternary expression types ────────────────────────────────────────
    $ternaryResult = true
        ? new Scaffolding\ElasticProductReviewIndexService()
        : new Scaffolding\ElasticBrandIndexService();
    assert(method_exists($ternaryResult, 'index'), 'Ternary result must have shared index() method');

    // ── Intersection types ──────────────────────────────────────────────
    // Can't instantiate an intersection directly, but we can verify interfaces
    assert(method_exists(Scaffolding\Envelope::class, 'seal'), 'Scaffolding\Envelope must have seal()');
    assert(interface_exists(Scaffolding\Printable::class), 'Scaffolding\Printable must be an interface');

    // A parenthesized DNF return type `(Scaffolding\Envelope&Scaffolding\Printable)|null` really
    // yields an object that satisfies both, so both members are callable.
    $sealed = (new IntersectionDemo())->sealed();
    assert($sealed instanceof Scaffolding\Envelope, 'sealed() must return an Scaffolding\Envelope');
    assert($sealed instanceof Scaffolding\Printable, 'sealed() must return a Scaffolding\Printable');

    // ── First-class callable syntax ─────────────────────────────────────
    $fun = Scaffolding\makePen(...);
    assert($fun instanceof \Closure, 'Scaffolding\makePen(...) must be a Closure');
    $funResult = $fun();
    assert($funResult instanceof Scaffolding\Pen, 'Scaffolding\makePen(...)() must return Scaffolding\Pen');

    $staticCallable = Scaffolding\Pen::make(...);
    assert($staticCallable instanceof \Closure, 'Scaffolding\Pen::make(...) must be a Closure');
    $staticResult = $staticCallable();
    assert($staticResult instanceof Scaffolding\Pen, 'Scaffolding\Pen::make(...)() must return Scaffolding\Pen');

    $src2 = new Scaffolding\ScaffoldingFirstClassCallable();
    $methodCallable = $src2->dispatch(...);
    assert($methodCallable instanceof \Closure, '$obj->method(...) must be a Closure');
    $methodResult = $methodCallable();
    assert($methodResult instanceof Scaffolding\Pen, 'dispatch(...)() must return Scaffolding\Pen');

    // Immediate invocation: method(...)() returns the method's return type
    $immediateFunc = Scaffolding\makePen(...)();
    assert($immediateFunc instanceof Scaffolding\Pen, 'Scaffolding\makePen(...)() immediate must return Scaffolding\Pen');
    $immediateStatic = Scaffolding\Pen::make(...)();
    assert($immediateStatic instanceof Scaffolding\Pen, 'Scaffolding\Pen::make(...)() immediate must return Scaffolding\Pen');
    $immediateMethod = $src2->dispatch(...)();
    assert($immediateMethod instanceof Scaffolding\Pen, '$obj->dispatch(...)() immediate must return Scaffolding\Pen');

    // ── Class alias (use ... as) ────────────────────────────────────────
    $aliasProfile = new Profile($userForProfile);
    assert($aliasProfile instanceof Scaffolding\UserProfile, 'Profile alias must be Scaffolding\UserProfile');
    assert($aliasProfile instanceof Profile, 'Profile alias instanceof must work');

    // ── HoverOriginsDemo extends Scaffolding\Model implements Scaffolding\Renderable ────────────
    $hod = new HoverOriginsDemo();
    assert($hod instanceof Scaffolding\Model, 'HoverOriginsDemo must extend Scaffolding\Model');
    assert($hod instanceof Scaffolding\Renderable, 'HoverOriginsDemo must implement Scaffolding\Renderable');
    assert(method_exists($hod, 'format'), 'HoverOriginsDemo must have format()');
    assert(method_exists($hod, 'toArray'), 'HoverOriginsDemo must have toArray()');
    assert(method_exists($hod, 'getName'), 'HoverOriginsDemo must inherit getName()');

    // ── Switch statement type tracking ──────────────────────────────────
    $switchType = 'reviews';
    switch ($switchType) {
        case 'reviews':
            $switchService = new Scaffolding\ElasticProductReviewIndexService();
            break;
        default:
            $switchService = new Scaffolding\ElasticBrandIndexService();
            break;
    }
    assert(method_exists($switchService, 'index'), 'Switch-assigned variable must have index()');

    // ── Spread operator ─────────────────────────────────────────────────
    $spreadSource = [new Scaffolding\Pen('a'), new Scaffolding\Pen('b')];
    $spread = [...$spreadSource];
    assert($spread[0] instanceof Scaffolding\Pen, 'Spread must preserve Scaffolding\Pen type');
    assert(count($spread) === 2, 'Spread must preserve array length');

    $pencilSource = [new Scaffolding\Pencil()];
    $mixed = [...$spreadSource, ...$pencilSource];
    assert($mixed[0] instanceof Scaffolding\Pen || $mixed[0] instanceof Scaffolding\Pencil, 'Multi-spread must contain Scaffolding\Pen|Scaffolding\Pencil');

    // ── Array destructuring ─────────────────────────────────────────────
    $destructSource = [new Scaffolding\Pen('x'), new Scaffolding\Pen('y')];
    [$dFirst, $dSecond] = $destructSource;
    assert($dFirst instanceof Scaffolding\Pen, 'Destructured element must be Scaffolding\Pen');
    assert($dSecond instanceof Scaffolding\Pen, 'Second destructured element must be Scaffolding\Pen');

    // ── Named key destructuring from shape ──────────────────────────────
    $shapeSource = ['pen' => new Scaffolding\Pen(), 'pencil' => new Scaffolding\Pencil()];
    ['pen' => $dPen, 'pencil' => $dPencil] = $shapeSource;
    assert($dPen instanceof Scaffolding\Pen, 'Named destructured pen must be Scaffolding\Pen');
    assert($dPencil instanceof Scaffolding\Pencil, 'Named destructured pencil must be Scaffolding\Pencil');

    // ── Nested destructuring ────────────────────────────────────────────
    /** @var array{string, array{Scaffolding\Pen, Scaffolding\Pencil}} $nestedDestr */
    $nestedDestr = ['label', [new Scaffolding\Pen(), new Scaffolding\Pencil()]];
    [$nLabel, [$nPen, $nPencil]] = $nestedDestr;
    assert(is_string($nLabel), 'Nested destructured label must be string');
    assert($nPen instanceof Scaffolding\Pen, 'Nested destructured pen must be Scaffolding\Pen');
    assert($nPencil instanceof Scaffolding\Pencil, 'Nested destructured pencil must be Scaffolding\Pencil');

    // ── Skipped destructuring positions ─────────────────────────────────
    /** @var array{string, Scaffolding\Pen, Scaffolding\Pencil} $skipDestr */
    $skipDestr = ['label', new Scaffolding\Pen(), new Scaffolding\Pencil()];
    [, $sPen, ] = $skipDestr;
    assert($sPen instanceof Scaffolding\Pen, 'A hole must not shift position 1 back to position 0');
    [, , $sPencil] = $skipDestr;
    assert($sPencil instanceof Scaffolding\Pencil, 'Two holes must land on position 2');

    /** @var array<int, array{string, Scaffolding\Pen}> $skipRows */
    $skipRows = [['blue', new Scaffolding\Pen()]];
    foreach ($skipRows as [, $sRowPen]) {
        assert($sRowPen instanceof Scaffolding\Pen, 'Foreach hole must not shift position 1 back to position 0');
    }

    // ── Foreach destructuring ───────────────────────────────────────────
    /** @var array<int, array{tool: Scaffolding\Pen, count: int}> $foreachDestrInv */
    $foreachDestrInv = [['tool' => new Scaffolding\Pen(), 'count' => 5]];
    foreach ($foreachDestrInv as ['tool' => $fTool, 'count' => $fCount]) {
        assert($fTool instanceof Scaffolding\Pen, 'Foreach destructured tool must be Scaffolding\Pen');
        assert(is_int($fCount), 'Foreach destructured count must be int');
    }

    // ── Foreach key types ───────────────────────────────────────────────
    /** @var list<Scaffolding\Pen> $keyedPens */
    $keyedPens = [new Scaffolding\Pen(), new Scaffolding\Pen()];
    foreach ($keyedPens as $listKey => $listPen) {
        assert(is_int($listKey), 'A list foreach key must be int');
        assert($listPen instanceof Scaffolding\Pen, 'A list foreach value must be Scaffolding\Pen');
    }
    /** @var array{tool: Scaffolding\Pen, spare: Scaffolding\Pen} $keyedKit */
    $keyedKit = ['tool' => new Scaffolding\Pen(), 'spare' => new Scaffolding\Pen()];
    foreach ($keyedKit as $shapeKey => $shapePen) {
        assert(is_string($shapeKey), 'A string-keyed shape foreach key must be string');
        assert($shapePen instanceof Scaffolding\Pen, 'A shape foreach value must be Scaffolding\Pen');
    }
    $keyedOpen = (new Scaffolding\ScaffoldingIteration())->openKeyed();
    foreach ($keyedOpen as $openKey => $openPen) {
        assert(is_int($openKey) || is_string($openKey),
            'A `Pen[]` names no key type, so its foreach key is the whole array-key domain');
        assert($openPen instanceof Scaffolding\Pen, 'An open-keyed foreach value must be Scaffolding\Pen');
        if (is_int($openKey)) {
            continue;
        }
        assert(is_string($openKey), 'Past an `is_int` guard the surviving key must be string');
    }
    $collectedKeys = [];
    foreach ($keyedOpen as $collectedKey => $collectedPen) {
        assert($collectedPen instanceof Scaffolding\Pen, 'An open-keyed foreach value must be Scaffolding\Pen');
        $collectedKeys[] = $collectedKey;
    }
    foreach ($collectedKeys as $collected) {
        assert(is_int($collected) || is_string($collected),
            'Keys collected out of a `Pen[]` stay the whole array-key domain');
    }

    // ── Literal values surviving an array read ──────────────────────────
    $litNumbers = [1, 1.5, '123'];
    assert(is_numeric($litNumbers[array_rand($litNumbers)]),
        'Every entry of [1, 1.5, \'123\'] is numeric, whichever key is drawn');
    assert($litNumbers[2] === '123', 'A literal-key read yields the value written at that position');
    foreach (['a', 'b', 'c'] as $litGrade) {
        assert(in_array($litGrade, ['a', 'b', 'c'], true),
            'Iterating a literal array yields the values it names');
    }

    // ── A strict in_array gate leaves one of the list's values ──────────
    $gateDemo = new InArrayNarrowingDemo();
    assert($gateDemo->gate('b') === 'b',
        'a value the constant list names passes the gate and comes back unchanged');
    $gateRejected = false;
    try {
        $gateDemo->gate(null);
    } catch (\RuntimeException) {
        $gateRejected = true;
    }
    assert($gateRejected, 'null is not one of Scaffolding\GRADES, so the gate throws');

    // ── Dynamic (non-literal) key on a shape ────────────────────────────
    $shapeDemo = new ShapeMethodDemo();
    $dynKit = $shapeDemo->getToolKit();
    $dynWhich = 'pen';
    $dynTool = $dynKit[$dynWhich] ?? null;
    assert($dynTool instanceof Scaffolding\Pen, 'Dynamic key on shape must yield a shape value');

    $dynSums = [];
    foreach ([1, 2] as $dynId) {
        $dynSums[$dynId] = $shapeDemo->getToolKit();
        assert($dynSums[$dynId]['pen'] instanceof Scaffolding\Pen,
            'Dynamic-key write must read back the shape value');
    }

    $dynReport = [];
    $dynCount = 0;
    foreach ([new Scaffolding\Pen(), new Scaffolding\Pen()] as $dynPen) {
        $dynReport['data'][$dynCount]['tool'] = $dynPen;
        $dynEntry = $dynReport['data'][$dynCount]['tool'];
        assert($dynEntry instanceof Scaffolding\Pen, 'Nested dynamic write must read back Scaffolding\Pen');
        $dynCount++;
    }

    // ── Sibling branches writing the same key ───────────────────────────
    $branchedRows = $shapeDemo->branchedKeyWrites([new Scaffolding\Pen(), new Scaffolding\Pencil()]);
    assert($branchedRows[0]['pen'] instanceof Scaffolding\Pen,
        'The Scaffolding\Pen branch must write the pen shape at its slot');
    assert($branchedRows[1]['pencil'] instanceof Scaffolding\Pencil,
        'The Scaffolding\Pencil branch must write the pencil shape at its slot');

    // ── Append below a key ──────────────────────────────────────────────
    $appended = $shapeDemo->appendsBelowAKey([new Scaffolding\Pen(), new Scaffolding\Pen()]);
    assert($appended[0][0] instanceof Scaffolding\Pen,
        'An append below a dynamic key must read back the pushed value');
    assert(count($appended) === 2, 'Each iterated slot gets its own list');

    // ── Array union with += ─────────────────────────────────────────────
    $unioned = $shapeDemo->unionsTwoShapes();
    assert($unioned['tool'] instanceof Scaffolding\Pen,
        '+= keeps the key the left side already holds');
    assert($unioned['spare'] instanceof Scaffolding\Pencil,
        '+= adds the key only the right side contributes');

    // ── Writes refine the tracked array ─────────────────────────────────
    $refined = $shapeDemo->writesRefineTheTrackedArray(['first' => new Scaffolding\Pen()], 'spare');
    assert($refined['tool'] instanceof Scaffolding\Pen,
        'An append leaves the keys the array already holds alone');
    assert($refined[0] instanceof Scaffolding\Pencil,
        'An append takes the next free integer key');

    // ── Ambiguous variables ─────────────────────────────────────────────
    if (rand(0, 1)) {
        $ambiguous = new Scaffolding\Lamp();
    } else {
        $ambiguous = new Scaffolding\Faucet();
    }
    assert($ambiguous instanceof Scaffolding\Lamp || $ambiguous instanceof Scaffolding\Faucet,
        'Ambiguous var must be Scaffolding\Lamp|Scaffolding\Faucet');
    assert(method_exists($ambiguous, 'turnOff'), 'Both Scaffolding\Lamp and Scaffolding\Faucet have turnOff()');

    // ── Guard clause narrowing ──────────────────────────────────────────
    $guardSubject = Scaffolding\pickRockOrBanana();
    if (!$guardSubject instanceof Scaffolding\Banana) {
        // would return in real code; just verify type
        assert($guardSubject instanceof Scaffolding\Rock, 'Guard: not Scaffolding\Banana must be Scaffolding\Rock');
    } else {
        assert($guardSubject instanceof Scaffolding\Banana, 'Guard: else must be Scaffolding\Banana');
        }

    // ── Guard clause exiting through a never-returning method ───────────
    $neverGuardSubject = Scaffolding\pickRockOrBanana();
    if (!$neverGuardSubject instanceof Scaffolding\Banana) {
        // Scaffolding\demoAborter()->fail() would throw in real code
        assert($neverGuardSubject instanceof Scaffolding\Rock, 'Never guard: not Scaffolding\Banana must be Scaffolding\Rock');
    } else {
        assert(is_string($neverGuardSubject->peel()), 'Never guard: Scaffolding\Banana survives the guard');
    }

        // ── Guard clause: positive instanceof + early return on mixed ────
        // After `if ($x instanceof Y) { return; }`, $x is NOT Y.
        $mixedGuardVal = rand(0, 1) ? new Scaffolding\Rock() : 'scalar';
        if ($mixedGuardVal instanceof Scaffolding\Banana) {
            // would return in real code
            assert(false, 'Guard: should not reach here (Scaffolding\Banana branch)');
        }
        // $mixedGuardVal is NOT Scaffolding\Banana after the guard
        if ($mixedGuardVal instanceof Scaffolding\Rock) {
            assert(is_string($mixedGuardVal->crush()), 'Guard: mixed narrowed to Scaffolding\Rock');
        }

    // ── Null coalesce refinement ────────────────────────────────────────
    $ncA = new Scaffolding\Pen() ?? new Scaffolding\Marker();
    assert($ncA instanceof Scaffolding\Pen, 'Null coalesce: non-nullable LHS must be Scaffolding\Pen');

    $ncNullable = rand(0, 1) ? new Scaffolding\Pen() : null;
    $ncB = $ncNullable ?? new Scaffolding\Marker();
    assert($ncB instanceof Scaffolding\Pen || $ncB instanceof Scaffolding\Marker,
        'Null coalesce: nullable LHS must be Scaffolding\Pen or Scaffolding\Marker');

    $ncClone = clone new Scaffolding\Pen() ?? new Scaffolding\Marker();
    assert($ncClone instanceof Scaffolding\Pen, 'Null coalesce: clone LHS must be Scaffolding\Pen');

    // ── Ternary narrowing ───────────────────────────────────────────────
    $ternaryThing = Scaffolding\pickRockOrBanana();
    $ternaryResult2 = $ternaryThing instanceof Scaffolding\Rock ? $ternaryThing->crush() : $ternaryThing->peel();
    assert(is_string($ternaryResult2), 'Ternary narrowed call must return string');

    // ── Scaffolding\User::toArray() ─────────────────────────────────────────────────
    $userArr = (new Scaffolding\User('Test', 'test@example.com'))->toArray();
    assert(is_array($userArr), 'Scaffolding\User::toArray() must return array');

    // ── Scaffolding\AstNode (template bounds) ───────────────────────────────────────
    $astNode = new Scaffolding\AstNode();
    assert($astNode->getType() === '' || is_string($astNode->getType()), 'Scaffolding\AstNode::getType() must return string');
    $children = $astNode->getChildren();
    assert(is_array($children), 'Scaffolding\AstNode::getChildren() must return array');

    // ── Pass-by-reference parameter type ────────────────────────────────
    $refPen = null;
    Scaffolding\initPen($refPen);
    assert($refPen instanceof Scaffolding\Pen, 'Scaffolding\initPen(&$pen) must give $pen type Scaffolding\Pen');

    $staticPen = null;
    Scaffolding\PenFactory::create($staticPen);
    assert($staticPen instanceof Scaffolding\Pen, 'Scaffolding\PenFactory::create(&$pen) must give $pen type Scaffolding\Pen');

    $ctorPen = null;
    new Scaffolding\PenBuilder($ctorPen);
    assert($ctorPen instanceof Scaffolding\Pen, 'new Scaffolding\PenBuilder(&$pen) must give $pen type Scaffolding\Pen');

    // A callee that assigns on every path leaves nothing of the declared
    // null behind; one that assigns on a single branch leaves it in place.
    $writtenPen = null;
    Scaffolding\initPen($writtenPen);
    assert(is_string(Scaffolding\describePen($writtenPen)), 'Scaffolding\initPen(&$pen) writes on every path, so $pen is never null');

    $maybePen = null;
    Scaffolding\initPenWhen(false, $maybePen);
    assert($maybePen === null, 'Scaffolding\initPenWhen(false, &$pen) leaves $pen null');

    // The shape an out-parameter already holds is overwritten, not read.
    $offsetMatches = null;
    foreach (['a 1', 'b 2'] as $line) {
        preg_match_all('/(\w+)/', $line, $offsetMatches, PREG_OFFSET_CAPTURE);
    }
    assert(is_int($offsetMatches[1][0][1]), 'preg_match_all() with PREG_OFFSET_CAPTURE pairs each match with its offset');

    // ── Interface template inheritance (class-string<T>) ────────────────
    $locator = new Scaffolding\ScaffoldingEntityLocator();
    $locatorResult = $locator->find(Scaffolding\Pen::class);
    assert($locatorResult instanceof Scaffolding\Pen, 'Scaffolding\ScaffoldingEntityLocator::find(Scaffolding\Pen::class) must return Scaffolding\Pen');

    // ── Function-level @template (Scaffolding\collect) ──────────────────────────────
    $collectPens = [new Scaffolding\Pen()];
    $collected = Scaffolding\collect($collectPens);
    assert($collected instanceof Scaffolding\FluentCollection, 'Scaffolding\collect() must return Scaffolding\FluentCollection');
    $firstPen = $collected->first();
    assert($firstPen instanceof Scaffolding\Pen, 'Scaffolding\collect(Scaffolding\Pen[])->first() must return Scaffolding\Pen');

    // ── Multi-line @method / @property tags ──────────────────────────────
    $multiLine = new Scaffolding\ScaffoldingMultiLineTags();
    assert(
        $multiLine->fetchAll('black')->first() instanceof Scaffolding\Pen,
        'multi-line @method tag must yield Scaffolding\FluentCollection<int, Scaffolding\Pen>',
    );
    assert(
        $multiLine->penHolder->first() instanceof Scaffolding\Pen,
        'multi-line @property tag must yield Scaffolding\FluentCollection<int, Scaffolding\Pen>',
    );

    // ── Generic @phpstan-assert narrowing ────────────────────────────────
    $assertObj = new Scaffolding\Pen();
    Scaffolding\ScaffoldingAssert::assertInstanceOf(Scaffolding\Pen::class, $assertObj);
    assert($assertObj instanceof Scaffolding\Pen, 'Scaffolding\ScaffoldingAssert::assertInstanceOf(Scaffolding\Pen::class, $obj) must narrow to Scaffolding\Pen');

    // The union asserted type: `!=null|''` on the true branch means a
    // filled value is neither, so a `?string` really is a `string` there.
    assert(Scaffolding\demoFilled('needle'), 'demoFilled() must accept a non-empty string');
    assert(!Scaffolding\demoFilled(null), 'demoFilled() must reject null');
    assert(!Scaffolding\demoFilled(''), 'demoFilled() must reject the empty string');
    $filledSearch = 'needle';
    if (Scaffolding\demoFilled($filledSearch)) {
        assert(is_string($filledSearch), 'a filled ?string is a string');
    }

    // A variable class argument still guarantees the subject is an object.
    $assertCls = Scaffolding\Pen::class;
    $assertNode = new Scaffolding\Pen();
    Scaffolding\ScaffoldingAssert::assertInstanceOf($assertCls, $assertNode);
    assert($assertNode instanceof Scaffolding\Pen, 'Scaffolding\ScaffoldingAssert::assertInstanceOf($cls, $node) keeps the prior Scaffolding\Pen type');

    // ── @param-closure-this scaffolding ──────────────────────────────────
    $ctRoute = new Scaffolding\ScaffoldingClosureThisRoute();
    $ctMw = $ctRoute->middleware('auth');
    assert($ctMw instanceof Scaffolding\ScaffoldingClosureThisRoute, 'Route::middleware() must return self');
    $ctPfx = $ctRoute->prefix('/api');
    assert($ctPfx instanceof Scaffolding\ScaffoldingClosureThisRoute, 'Route::prefix() must return self');

    $ctRouter = new Scaffolding\ScaffoldingClosureThisRouter();
    assert(is_string($ctRouter->getDefaultDriver()), 'Router::getDefaultDriver() must return string');
    $ctExt = $ctRouter->extend('redis', function () {});
    assert($ctExt instanceof Scaffolding\ScaffoldingClosureThisRouter, 'Router::extend() must return self');

    // Nested @param-closure-this: the innermost binding is the one in
    // effect, and the inner call's receiver is the outer binding.
    $ctInner = null;
    $ctRouter->group(function () use (&$ctInner) {
        $this->resource('posts', function () use (&$ctInner) {
            $ctInner = $this->only('index');
        });
    });
    assert($ctInner instanceof Scaffolding\ScaffoldingClosureThisResource, 'nested @param-closure-this must bind $this to the innermost declared type');

    // A tag that declares the base class may bind a subclass, which is why
    // asserting inside the closure body is worth anything.
    $ctApi = null;
    $ctRouter->apiGroup(function () use (&$ctApi) {
        assert($this instanceof Scaffolding\ScaffoldingClosureThisApiRoute);
        $ctApi = $this->version('v2');
    });
    assert($ctApi instanceof Scaffolding\ScaffoldingClosureThisApiRoute, 'apiGroup() must bind $this to the subclass its tag cannot name');

    // Macro-style scope binding: self::/static:: inside a registered
    // closure refer to the macro target class at runtime.
    Scaffolding\ScaffoldingMacroTarget::macro('renderTwice', function (): string {
        return self::make()->render() . static::make()->render();
    });
    $macroTarget = new Scaffolding\ScaffoldingMacroTarget();
    assert($macroTarget->renderTwice() === 'renderedrendered', 'self::/static:: inside a macro closure must bind to Scaffolding\ScaffoldingMacroTarget');

    // ── Inherited docblock restricted by the override's own hint ────────
    $arrayPayload = (new Scaffolding\ScaffoldingArrayPayload())->payload();
    assert(is_array($arrayPayload), 'Scaffolding\ScaffoldingArrayPayload::payload() must return an array, never the string half');

    // ── Boolean literals and truthiness ─────────────────────────────────
    $booleanDemo = new BooleanLiteralDemo();
    assert($booleanDemo->demo('', 'Y') === '', 'an unparsed timestamp must take the falsy branch');
    assert($booleanDemo->demo('1970-01-02 UTC', 'Y') === '1970', 'a parsed timestamp must reach date()');

    // ── Closure literal signatures ──────────────────────────────────────
    $describePen = fn (Scaffolding\Pen $pen) => $pen->color();
    assert(
        (new Scaffolding\ScaffoldingPenDescriber())->describeWith($describePen) === 'black',
        'an arrow function must satisfy a declared Closure(Scaffolding\Pen): string'
    );

    // ── @mixin generic substitution scaffolding ─────────────────────────
    $mixinBuilder = new Scaffolding\ScaffoldingMixinBuilder();
    assert($mixinBuilder->firstOrFail() === null, 'Scaffolding\ScaffoldingMixinBuilder::firstOrFail() must return mixed');
    $mixinRelation = new Scaffolding\ScaffoldingMixinRelation();
    assert($mixinRelation instanceof Scaffolding\ScaffoldingMixinRelation, 'Scaffolding\ScaffoldingMixinRelation instantiates');
    $mixinBelongsTo = new Scaffolding\ScaffoldingMixinBelongsTo();
    assert($mixinBelongsTo instanceof Scaffolding\ScaffoldingMixinRelation, 'Scaffolding\ScaffoldingMixinBelongsTo extends Scaffolding\ScaffoldingMixinRelation');
    $orderLine = new Scaffolding\ScaffoldingOrderLine();
    $productRel = $orderLine->product();
    assert($productRel instanceof Scaffolding\ScaffoldingMixinBelongsTo, 'OrderLine::product() must return Scaffolding\ScaffoldingMixinBelongsTo');

    // ── @mixin with template parameter ──────────────────────────────────
    $tplMixinNode = new Scaffolding\ScaffoldingConcreteAstNode();
    $col = $tplMixinNode->getStartColumn();
    assert(is_int($col), 'ConcreteAstNode (via @mixin TNode bound) getStartColumn() must return int');
    // The tighter member is only on the subclass's narrowed bound, resolved
    // through the @mixin declared on the base class.
    $tplCallableNode = new Scaffolding\ScaffoldingConcreteCallableAstNode();
    $count = $tplCallableNode->getParameterCount();
    assert(is_int($count), 'ConcreteCallableAstNode (via tightest @mixin TNode bound) getParameterCount() must return int');

    // ── new $var() with class-string<T> ─────────────────────────────────
    $penFromClassString = Scaffolding\ScaffoldingClassStringFactory::create(Scaffolding\Pen::class);
    assert($penFromClassString instanceof Scaffolding\Pen, 'ClassStringFactory::create(Scaffolding\Pen::class) must return Scaffolding\Pen');

    // ── Inherited docblock type propagation ─────────────────────────────
    $iHolder = new Scaffolding\ScaffoldingConcreteHolder();
    $iHolderPens = $iHolder->getPens();
    assert(is_array($iHolderPens), 'Scaffolding\ScaffoldingConcreteHolder::getPens() must return array');
    assert($iHolderPens[0] instanceof Scaffolding\Pen, 'Scaffolding\ScaffoldingConcreteHolder::getPens()[0] must be Scaffolding\Pen');

    $iChild = new Scaffolding\ScaffoldingChildHolder();
    $iChildPens = $iChild->getPens();
    assert(is_array($iChildPens), 'Scaffolding\ScaffoldingChildHolder::getPens() must return array');
    assert($iChildPens[0] instanceof Scaffolding\Pen, 'Scaffolding\ScaffoldingChildHolder::getPens()[0] must be Scaffolding\Pen');

    $iDeep = new Scaffolding\ScaffoldingDeepChild();
    $iDeepPens = $iDeep->getPens();
    assert(is_array($iDeepPens), 'Scaffolding\ScaffoldingDeepChild::getPens() must return array');
    assert($iDeepPens[0] instanceof Scaffolding\Pen, 'Scaffolding\ScaffoldingDeepChild::getPens()[0] must be Scaffolding\Pen');

    $iCat = new Scaffolding\ScaffoldingCatStore();
    $iCatAnimals = $iCat->getAnimals();
    assert(is_array($iCatAnimals), 'Scaffolding\ScaffoldingCatStore::getAnimals() must return array');
    assert($iCatAnimals[0] instanceof Scaffolding\Pencil, 'Scaffolding\ScaffoldingCatStore::getAnimals()[0] must be Scaffolding\Pencil');

    $iBox = new Scaffolding\ScaffoldingPenBox();
    $iBoxPens = $iBox->getPens();
    assert(is_array($iBoxPens), 'Scaffolding\ScaffoldingPenBox::getPens() must return array');
    assert($iBoxPens[0] instanceof Scaffolding\Pen, 'Scaffolding\ScaffoldingPenBox::getPens()[0] must be Scaffolding\Pen');

    // ── Loop-carried assignment ─────────────────────────────────────────
    $lcPens = [new Scaffolding\Pen('a'), new Scaffolding\Pen('b')];
    $lcPrev = null;
    foreach ($lcPens as $lcPen) {
        if ($lcPrev !== null) {
            assert($lcPrev instanceof Scaffolding\Pen, 'Loop-carried $lcPrev must be Scaffolding\Pen on second iteration');
        }
        $lcPrev = $lcPen;
    }
    assert($lcPrev instanceof Scaffolding\Pen, '$lcPrev must be Scaffolding\Pen after foreach');

    $lcLast = null;
    $lcIter = 0;
    while ($lcIter < 2) {
        if ($lcLast !== null) {
            assert($lcLast instanceof Scaffolding\Response, 'Loop-carried $lcLast must be Scaffolding\Response');
        }
        $lcLast = new Scaffolding\Response(200, 'ok');
        $lcIter++;
    }
    assert($lcLast instanceof Scaffolding\Response, '$lcLast must be Scaffolding\Response after while');

    // ── Constant type inference ─────────────────────────────────────────
    assert(ConstantTypeDemo::TIMEOUT === 30, 'ConstantTypeDemo::TIMEOUT must be 30');
    assert(ConstantTypeDemo::NAME === 'app', 'ConstantTypeDemo::NAME must be "app"');
    assert(ConstantTypeDemo::RATE === 3.14, 'ConstantTypeDemo::RATE must be 3.14');
    assert(ConstantTypeDemo::ENABLED === true, 'ConstantTypeDemo::ENABLED must be true');
    assert(CT_ALLOWED_HOSTS === ['localhost', '127.0.0.1'], 'CT_ALLOWED_HOSTS must match');
    assert(Scaffolding\CT_APP_VERSION === '2.0.0', 'Scaffolding\CT_APP_VERSION must be "2.0.0"');

    // ── Variadic foreach ────────────────────────────────────────────────
    $vfDemo = new VariadicForeachDemo();
    $vfPens = [new Scaffolding\Pen('a'), new Scaffolding\Pen('b')];
    // demo() accepts Scaffolding\Pen ...$pens — foreach inside should see Scaffolding\Pen elements
    $vfDemo->demo(...$vfPens);
    foreach ($vfPens as $vfPen) {
        assert($vfPen instanceof Scaffolding\Pen, 'Variadic Scaffolding\Pen element must be Scaffolding\Pen');
    }
    $vfTools = [new Scaffolding\Pen('x'), new Scaffolding\Pencil()];
    foreach ($vfTools as $vfTool) {
        assert(
            $vfTool instanceof Scaffolding\Pen || $vfTool instanceof Scaffolding\Pencil,
            'Variadic union element must be Scaffolding\Pen or Scaffolding\Pencil'
        );
    }

    // ── Type guard narrowing ────────────────────────────────────────────
    /** @var list<Scaffolding\Pen> $tgPens */
    $tgPens = [new Scaffolding\Pen('a'), new Scaffolding\Pen('b')];
    /** @var null|list<Scaffolding\Pen>|Scaffolding\Pen $tgInput */
    $tgInput = $tgPens;
    if (is_array($tgInput)) {
        foreach ($tgInput as $tgPen) {
            assert($tgPen instanceof Scaffolding\Pen, 'is_array() narrowed foreach element must be Scaffolding\Pen');
        }
    }
    $tgSingle = new Scaffolding\Pen('solo');
    /** @var list<Scaffolding\Pen>|Scaffolding\Pen $tgMixed */
    $tgMixed = $tgSingle;
    if (!is_array($tgMixed)) {
        assert($tgMixed instanceof Scaffolding\Pen, 'Else branch of is_array() must be Scaffolding\Pen');
    }

    // ── Foreach array shape elements ────────────────────────────────────
    /** @var array<int, array{tool: Scaffolding\Pen, count: int}> $fasInventory */
    $fasInventory = [['tool' => new Scaffolding\Pen('red'), 'count' => 3]];
    foreach ($fasInventory as $fasEntry) {
        assert($fasEntry['tool'] instanceof Scaffolding\Pen, 'Foreach over array shape must resolve key type');
    }

    // ── Loop array build (variable-key assignment) ──────────────────────
    $labPens = [new Scaffolding\Pen('red'), new Scaffolding\Pen('blue')];
    $labIndexed = [];
    foreach ($labPens as $labPen) {
        $labKey = $labPen->color();
        $labIndexed[$labKey] = $labPen;
    }
    assert($labIndexed['red'] instanceof Scaffolding\Pen, 'Variable-key array element must be Scaffolding\Pen');
    foreach ($labIndexed as $labItem) {
        assert($labItem instanceof Scaffolding\Pen, 'Foreach over variable-key array must yield Scaffolding\Pen');
    }
    $labFound = $labIndexed['blue'] ?? null;
    assert($labFound instanceof Scaffolding\Pen, 'Null-coalesce on variable-key array must resolve to Scaffolding\Pen');

    // ── Conditional shape key addition ──────────────────────────────────
    $cskOptions = ['name' => 'default'];
    $cskPen = new Scaffolding\Pen('blue');
    $cskOptions['tool'] = $cskPen;
    assert($cskOptions['tool'] instanceof Scaffolding\Pen, 'Conditional shape key must resolve to Scaffolding\Pen');

    // ── Conditional loop shape (keyed assignment in if/else) ────────────
    $shapePens = [new Scaffolding\Pen('red'), new Scaffolding\Pen('blue'), new Scaffolding\Pen('red')];
    $shapeGrouped = [];
    foreach ($shapePens as $shapePen) {
        $shapeKey = $shapePen->color();
        if (array_key_exists($shapeKey, $shapeGrouped)) {
            $shapeGrouped[$shapeKey]['count']++;
        } else {
            $shapeGrouped[$shapeKey] = [
                'tool'  => $shapePen,
                'count' => 1,
            ];
        }
    }
    foreach ($shapeGrouped as $shapeEntry) {
        assert($shapeEntry['tool'] instanceof Scaffolding\Pen, 'Shape key from conditional loop must resolve to Scaffolding\Pen');
    }

    // ── Read loop that reassigns the checked variable ────────────────────
    assert(
        (new LoopCarriedAssignmentDemo())->readLoop(['alpha', 'beta', 'gamma']) === 'ALPHABETAGAMMA',
        'a read loop reads every line the stream hands out before the false'
    );
    assert(
        (new LoopCarriedAssignmentDemo())->compactReadLoop(['alpha', 'beta']) === 'ALPHABETAomega',
        'the compact read loop reads the same lines through its condition assignment'
    );
    assert(
        (new ConditionAssignmentDemo())->firstPen() instanceof Scaffolding\Pen,
        'a sentinel check beside an assignment leaves the success type behind'
    );

    // ── Folding a variable number of values into one accumulator ─────────
    $foldDemo = new LoopCarriedAssignmentDemo();
    $foldSteps = [new Scaffolding\DrawingStep(2), new Scaffolding\DrawingStep(3), new Scaffolding\DrawingStep(4)];
    assert(
        $foldDemo->foldAccumulator($foldSteps) === 9,
        'the fold seeds on the first pass and merges on the rest, so every step counts once'
    );
    assert(
        $foldDemo->foldAccumulator([]) === 0,
        'a loop that never runs leaves the accumulator null, which is what the `?->` reads'
    );
    assert(
        $foldDemo->foldAccumulatorTernary($foldSteps) === 18,
        'the if/else and ternary spellings of the fold add up to the same total each'
    );

    // ── Pre-validation loops ─────────────────────────────────────────────
    $preValidation = new PreValidationLoopDemo();
    $labelled = new Scaffolding\SketchGroup([
        new Scaffolding\LabelledSketchNode('alpha'),
        new Scaffolding\LabelledSketchNode('beta'),
    ]);
    $mixed = new Scaffolding\SketchGroup([new Scaffolding\LabelledSketchNode('alpha'), new Scaffolding\SketchNode()]);
    assert(
        $preValidation->captions([$labelled]) === 'alphabeta',
        'every node passed the pre-validation loop, so the second loop reads all of them'
    );
    assert(
        $preValidation->captions([$mixed]) === '',
        'one failing node makes the `break 2` skip the second loop entirely'
    );
    assert(
        $preValidation->firstCaption($labelled) === 'alpha',
        'a return guard rejects the whole group, so reaching the second loop proves every node'
    );
    assert(
        $preValidation->firstCaption($mixed) === '',
        'the return guard fires on the unlabelled node before the second loop runs'
    );
    assert(
        $preValidation->unproven($mixed) === 'labellednode',
        'a plain break leaves the unchecked nodes in the list the second loop walks'
    );

    // ── By-reference captures written on the way out ─────────────────────
    $gather = new ByRefCaptureGatherDemo();
    assert(
        $gather->captions($labelled) === 'alphabeta',
        'the callback pushes and returns in the same branch, and both pushes stick'
    );
    assert(
        $gather->captions($mixed) === 'alpha',
        'the unlabelled node fails the instanceof check, so nothing is pushed for it'
    );
    assert(
        $gather->firstCaption($labelled) === 'alpha',
        'the guard clause writes the capture once and returns, keeping the first match'
    );
    assert(
        $gather->firstCaption(new Scaffolding\SketchGroup()) === '',
        'an empty group never runs the callback, so the capture is still null'
    );

    // ── Loops over an array that cannot be empty ─────────────────────────
    $nonEmptyLoop = new NonEmptyLoopDemo();
    $shortest = new Scaffolding\Pen();
    assert(
        $nonEmptyLoop->smallest([new Scaffolding\Pen(), $shortest]) instanceof Scaffolding\Pen,
        'a guarded non-empty list leaves a pen behind, never the pre-loop null'
    );
    assert(
        is_string($nonEmptyLoop->labels([new Scaffolding\Pen()])),
        'a non-empty-list annotation runs the body, so both loop-written names are set'
    );
    assert(
        $nonEmptyLoop->maybeLast([]) === null,
        'a list that may be empty really does leave the pre-loop null behind'
    );

    // ── Untyped property inference from constructor ─────────────────────
    $untypedDemo = new UntypedPropertyInferenceDemo();
    // The scaffolding repo's findById() returns Scaffolding\Pen, so we can verify
    // that the inferred type propagates through the property chain.
    $repoRef = new Scaffolding\ScaffoldingUntypedRepo();
    $found = $repoRef->findById(1);
    assert($found instanceof Scaffolding\Pen, 'Scaffolding\ScaffoldingUntypedRepo::findById() must return Scaffolding\Pen');

    // ── Deep variable chain ────────────────────────────────────────────
    $chainBrush = new Scaffolding\Brush();
    $chainCanvas = $chainBrush->getCanvas();
    assert($chainCanvas instanceof Scaffolding\Canvas, 'Scaffolding\Brush::getCanvas() must return Scaffolding\Canvas');
    $chainEasel = $chainCanvas->easel;
    assert($chainEasel instanceof Scaffolding\Easel, 'Scaffolding\Canvas::$easel must be Scaffolding\Easel');
    $chainMaterial = $chainEasel->material;
    assert(is_string($chainMaterial), 'Scaffolding\Easel::$material must be string');
    $chainBack = $chainCanvas->getBrush();
    assert($chainBack instanceof Scaffolding\Brush, 'Scaffolding\Canvas::getBrush() must return Scaffolding\Brush');

    // ── Closure scope inference ────────────────────────────────────────
    $scopePens = [new Scaffolding\Pen()];
    $scopeWorker = function () use ($scopePens): void {
        foreach ($scopePens as $sp) {
            assert($sp instanceof Scaffolding\Pen, 'Captured $pens element must be Scaffolding\Pen');
        }
    };
    $scopeWorker();

    // ── Global keyword ─────────────────────────────────────────────────
    global $globalPen;
    assert($globalPen instanceof Scaffolding\Pen, '$globalPen must be Scaffolding\Pen at top level');
    globalKeywordDemo();

    // ── Built-in generic collections ────────────────────────────────────
    $demo = new BuiltinGenericCollectionDemo();
    $pen = $demo->getPens()->current();
    assert($pen instanceof Scaffolding\Pen, 'ArrayIterator<int, Scaffolding\Pen>::current() must return Scaffolding\Pen');

    // ── SimpleXMLElement iteration (Iterator without generics) ──────────
    $xmlDemo = new SimpleXmlIterationDemo();
    $xmlChild = $xmlDemo->firstChild();
    assert($xmlChild instanceof \SimpleXMLElement, 'SimpleXMLElement::children() foreach element must be SimpleXMLElement');

    // ── SPL wrapper iterators ───────────────────────────────────────────
    $filter = new Scaffolding\PhpFileFilter(new \ArrayIterator([new \SplFileInfo(__FILE__)]));
    foreach ($filter as $splFile) {
        assert($splFile instanceof \SplFileInfo, 'FilterIterator<_, SplFileInfo, _> foreach element must be SplFileInfo');
    }
    foreach (new \DirectoryIterator(__DIR__) as $dirEntry) {
        assert($dirEntry instanceof \DirectoryIterator, 'DirectoryIterator foreach element must be DirectoryIterator');
        break;
    }

    // ── Lazy initialisation inside a guarded `if` ───────────────────────
    $lazy = new LazyInitNarrowingDemo();
    assert($lazy->marker() instanceof Scaffolding\Marker, 'a property assigned inside a guard keeps that type after the block');

    // ── Builtins whose failure branch is never checked ──────────────────
    $benevolent = new BenevolentBuiltinDemo();
    $reportPath = $benevolent->writeReport('body');
    assert(is_string($reportPath), 'tempnam() yields a usable path, so the |false branch never arrives');
    assert(file_get_contents($reportPath) === 'body', 'the report was written to the temp file');
    assert($benevolent->writeCheckedReport('checked') !== null, 'checking === false narrows to the string branch');
    unlink($reportPath);

    // ── Scalar guard clauses on a property ──────────────────────────────
    $guarded = new PropertyGuardDemo();
    assert($guarded->earlyReturn() === 'unlabelled', 'the guard fires while the property is false');
    assert($guarded->skipIteration() === 0, 'an empty label skips every iteration');
    assert($guarded->chainedPath() === 'anonymous', 'a null handle takes the guarded branch');
    $guarded->label = 'ink';
    $guarded->handle = new Scaffolding\ScaffoldingHandle('nib');
    assert($guarded->earlyReturn() === 'INK', 'past the guard the label really is a string');
    assert($guarded->earlyThrow() === 'INK', 'a `!` guard proves the same thing');
    assert($guarded->skipIteration() === 9, 'three iterations of a 3-character label');
    assert($guarded->chainedPath() === 'NIB', 'a two-hop path narrows the same way');
    assert($guarded->rewritten('new') === 'NEW', 'the second guard proves the written value');
    assert($guarded->rewritten(false) === 'unlabelled', 'a write after the guard restores the union');

    // ── interface-string names an interface, not a class ────────────────
    assert(interface_exists(Scaffolding\Printable::class), 'Scaffolding\Printable::class is an interface-string');
    assert(!interface_exists(Scaffolding\SealedEnvelope::class), 'Scaffolding\SealedEnvelope::class is not, however it is implemented');

    // ── Magic members highlighted from their docblock tags ──────────────
    $magic = new SemanticMagicMemberDemo();
    assert($magic->displayName === 'Ada Lovelace', '@property $displayName is served by __get');
    assert($magic->shout('ada') === 'ADA', '@method shout() is served by __call');
    assert(SemanticMagicMemberDemo::brand() === 'PHPantom', '@method static brand() is served by __callStatic');
    assert($magic->demo() === 'Ada LovelaceADA', 'both instance magic members dispatch at runtime');
    assert(SemanticMagicMemberDemo::slogan() === 'built with PHPantom', 'static::brand() from a static method reaches __callStatic');

    // ── Magic constants ─────────────────────────────────────────────────
    $magicConst = new MagicConstantDemo();
    assert(is_int($magicConst->lineOffset()), '__LINE__ + 3 really is an int');
    assert(str_contains($magicConst->describe(), 'completion.php'), '__FILE__ is the demo file');
    assert(str_starts_with($magicConst->describe(), MagicConstantDemo::class . '::describe'), '__METHOD__ names the class and the method');
    assert(class_exists($magicConst->ownName()), '__CLASS__ is a class-string');

    // ── Property hook bodies ────────────────────────────────────────────
    $hooked = new HookedGtdDemo();
    assert($hooked->formatted === 'gtd', 'an expression-bodied get hook reads through $this');
    assert($hooked->labelled === 'gtd', 'a block-bodied get hook keeps its local across statements');
    $hooked->declaredWrite = new GtdTarget();
    assert($hooked->lastSeen === 'gtd', 'a declared set($value) receives the assigned object');
    $hooked->implicitWrite = new GtdTarget();
    assert($hooked->lastSeen === 'gtd', 'a set hook with no parameter list still receives $value');

    $promotedHook = new PromotedHookGtdDemo(new GtdTarget(), new GtdTarget());
    assert($promotedHook->seen === 'gtd', 'promotion runs the promoted property\'s set hook');
    assert($promotedHook->promoted instanceof GtdTarget, 'the promoted get hook reads through $this');
    assert($promotedHook->formatted === 'gtd', 'parent::$formatted::get() calls the overridden hook');

    // ── The user-comparison sorts, and a callback narrowed by its body ──
    $sorted = (new Scaffolding\ScaffoldingArrayFunc())->byName();
    uasort($sorted, fn($a, $b) => strcmp($a->color(), $b->color()));
    assert(array_keys($sorted) === ['blue', 'red'], 'uasort() hands its callback the values and keeps the keys with them');
    uksort($sorted, fn($a, $b) => strcmp($b, $a));
    assert(array_keys($sorted) === ['red', 'blue'], 'uksort() hands its callback the keys');

    $markers = (new Scaffolding\ScaffoldingArrayFunc())->markers();
    $renamedMarkers = array_map(fn(Scaffolding\Marker $m): Scaffolding\Pen => $m->rename('wide'), $markers);
    assert($renamedMarkers[0] instanceof Scaffolding\Marker, 'rename() returns static, so a callback declaring Scaffolding\Pen still hands back a Scaffolding\Marker');

    assert((new ClosureParamInferenceDemo())->docblockedClosure() === 'blue', 'a closure typed by the docblock above its assignment runs on the values that docblock describes');

    // ── Arrays the code proved have entries ─────────────────────────────
    $proven = new ProvenNonEmptyDemo();
    $penList = [new Scaffolding\Pen('red'), new Scaffolding\Pen('blue')];
    assert($proven->afterCountGuard($penList)->color() === 'blue', 'a loop guarded by count() > 0 runs, so the sentinel is gone');
    assert($proven->afterCountGuard([])->color() === 'black', 'the empty case never enters the guarded branch');
    assert($proven->afterEmptyGuard($penList)->color() === 'blue', 'the fall-through of a count() === 0 guard iterates at least once');
    assert($proven->afterElementWrite(new Scaffolding\Pen('green'))->color() === 'green', 'an array an element was written to has that element to iterate');
    assert($proven->afterConditionalWrite(new Scaffolding\Pen('gold'), true)?->color() === 'gold', 'the branch that wrote the element leaves it there');
    assert($proven->afterConditionalWrite(new Scaffolding\Pen('gold'), false) === null, 'the branch that wrote nothing leaves the loop with no iterations');

    // ── Proofs the condition never states outright ──────────────────────
    $reconstructed = new ReconstructedProofDemo();
    $loopNode = new Scaffolding\ScaffoldingLoopNode();
    $loopNode->valueVar->name = 'value';
    assert($reconstructed->keyName($loopNode) === null, 'the null leg of the guard is the one that held');
    $loopNode->keyVar = new Scaffolding\ScaffoldingNameNode();
    $loopNode->keyVar->name = 'key';
    assert($reconstructed->keyName($loopNode) === 'key', 'the surviving leg proves the key name is a string');

    assert($reconstructed->describeArguments(['a', 'b']) === 'a, b', 'the re-tested count guard proves the acceptor was filled');
    assert($reconstructed->describeArguments([]) === '', 'neither branch runs when there are no arguments');

    $labelled = new Scaffolding\ScaffoldingOptionalLabel();
    assert(($reconstructed->labelPrinter($labelled))() === '', 'a null label never reaches the capturing closure');
    $labelled->label = 'gtd';
    assert(($reconstructed->labelPrinter($labelled))() === 'GTD', 'the closure runs on the label the guard proved');

    assert($reconstructed->describeWhenFlagged(['a', 'b'], true) === 'a, b', 'the re-tested flag proves the acceptor was filled');
    assert($reconstructed->describeWhenFlagged(['a', 'b'], false) === '', 'a flag that skipped the branch is false below it');

    $plainName = new Scaffolding\ScaffoldingNameNode();
    $qualifiedName = new Scaffolding\ScaffoldingQualifiedName();
    $qualifiedName->namespacePrefix = 'App\\';
    assert($reconstructed->qualifiedLabel($qualifiedName) === 'q', 'the re-tested instanceof proves the acceptor was filled');
    assert($reconstructed->qualifiedLabel($plainName) === '', 'the path that failed the check filled nothing');
    assert($reconstructed->eitherPrefix($plainName, $qualifiedName) === 'App\\', 'ruling the first flag out leaves what the second proved');
    assert($reconstructed->eitherPrefix($qualifiedName, $plainName) === 'App\\', 'the first flag holding proves its own subject');
    assert($reconstructed->eitherPrefix($plainName, $plainName) === '', 'neither flag holding leaves the guard');

    // ── Member visibility ───────────────────────────────────────────────
    // The diagnostics demo claims which members of ScaffoldingVault are out
    // of reach from outside; reflection is what makes that claim checkable
    // without writing code PHP would fatal on.
    $vault = new \ReflectionClass(Scaffolding\ScaffoldingVault::class);
    assert($vault->getProperty('pin')->isPrivate(), '$pin is private, so an outside read is fatal');
    assert($vault->getProperty('branch')->isProtected(), '$branch is protected, so a subclass may read it');
    assert($vault->getProperty('label')->isPublic(), '$label is public and always reachable');
    assert($vault->getProperty('openCount')->isPrivate(), '$openCount is a private static property');
    assert($vault->getMethod('rotate')->isPrivate(), 'rotate() is private');
    assert($vault->getMethod('open')->isPublic(), 'open() is the public way in');
    assert($vault->getReflectionConstant('MASTER_KEY')->isPrivate(), 'MASTER_KEY is a private constant');
    assert($vault->getReflectionConstant('REGION')->isPublic(), 'REGION is a public constant');

    $branchVault = new \ReflectionClass(Scaffolding\ScaffoldingBranchVault::class);
    assert($branchVault->getProperty('ledger')->isPrivate(), '$ledger is private to the subclass that declares it');
    assert($branchVault->isSubclassOf(Scaffolding\ScaffoldingVault::class), 'the branch vault descends from the vault');
    assert(!$branchVault->hasProperty('pin') || $branchVault->getProperty('pin')->getDeclaringClass()->getName() === Scaffolding\ScaffoldingVault::class, 'a private parent property is not inherited into the subclass scope');

    echo "All assertions passed.\n";
}

runDemoAssertions();
