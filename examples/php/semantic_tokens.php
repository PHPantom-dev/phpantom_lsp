<?php

/**
 * PHP Showcase — Semantic Tokens
 *
 * The type-aware highlighting PHPantom layers on top of your editor's own
 * syntax grammar. A grammar colors text by its shape; semantic tokens color
 * it by what it resolves to, so identical-looking text ends up different
 * colors depending on what it means in this spot.
 *
 * Two things to know before reading on:
 *
 * 1. PHPantom emits `contextual` tokens by default: only the ones a grammar
 *    cannot work out for itself, leaving everything else to your editor's
 *    coloring. Topics marked "(full mode)" need
 *
 *        [semantic-tokens]
 *        mode = "full"
 *
 *    in `.phpantom.toml`. See docs/configuration.md for the other modes.
 * 2. What you actually see depends on the theme. Themes decide which token
 *    types and modifiers get their own color, and most map only a handful.
 *    A topic that looks no different from the code around it means the
 *    theme colors that token like everything else, not that the token is
 *    missing.
 *
 * One of the demo files listed in README.md. Supporting fixtures live in
 * scaffolding/scaffolding.php (namespace Demo\Scaffolding), and the runtime
 * assertions that verify the type claims in the comments below live in
 * scaffolding/assertions.php.
 */

namespace Demo;

use Demo\Scaffolding;

// ── Semantic Tokens: class vs interface vs enum vs trait ────────────────────
// Four references with the same shape, four token types. A grammar sees four
// capitalized words and colors them all alike; PHPantom resolves each name
// and reports what it found. (full mode)

class SemanticKindsDemo
{
    use Scaffolding\JsonSerializer;   // trait → `type`

    public function demo(
        Scaffolding\Pen $pen,                 // class → `class`
        Scaffolding\Renderable $renderable,   // interface → `interface`
        Scaffolding\Status $status,           // enum → `enum`
    ): void {
    }
}


// ── Semantic Tokens: members are verified before they are colored ───────────

class SemanticMemberVerificationDemo
{
    public function demo(): string
    {
        // 'write' is a real method on Scaffolding\Pen, so it gets the method
        // token even though it is written as a plain string. Misspell it and
        // the token disappears: the string drops back to ordinary string
        // coloring, which makes a broken callable visible at a glance.
        // (full mode)
        $callable = [Scaffolding\Pen::class, 'write'];

        // The same check runs on ordinary calls whose subject is a bare class
        // name or $this. `make` exists on Pen, so it is colored as a method,
        // and since it is a static one the token says so even in contextual
        // mode: a grammar cannot tell a static call from an instance call.
        $pen = Scaffolding\Pen::make('blue');

        // Subjects PHPantom would have to resolve an expression for (a
        // variable, a call chain) are too expensive to verify on every
        // keystroke, so their members are colored unconditionally.
        // (full mode)
        $pen->rename('Sky Blue');

        return $callable[1] . $pen->color();
    }
}


// ── Semantic Tokens: magic members ──────────────────────────────────────────
// Members that exist only as docblock tags are resolved like declared ones,
// so they are colored as a property and a method rather than left as unknown
// text. That holds on both sides: the name in the tag below is colored by the
// tag that declares it, and matches the color it gets everywhere it is used.
// (full mode)

/**
 * @property string $displayName    <- colored as a property
 * @method string shout(string $text)
 * @method static string brand()
 */
class SemanticMagicMemberDemo
{
    public function demo(): string
    {
        // Neither member is declared below. Both come from the tags above,
        // and PHPantom colors them exactly as it would color a declared
        // property and a declared method.
        $name = $this->displayName;   // property
        $loud = $this->shout('ada');  // method

        return $name . $loud;
    }

    /**
     * `brand` is tagged static, so its token says so even in contextual
     * mode: static is the part a grammar cannot infer. There is no $this
     * here either, which is what makes PHP route the call to __callStatic
     * rather than __call.
     */
    public static function slogan(): string
    {
        return 'built with ' . static::brand();   // method, static
    }

    public function __get(string $name): mixed
    {
        return $name === 'displayName' ? 'Ada Lovelace' : null;
    }

    public function __call(string $name, array $args): mixed
    {
        return strtoupper((string) ($args[0] ?? ''));
    }

    public static function __callStatic(string $name, array $args): mixed
    {
        return $name === 'brand' ? 'PHPantom' : null;
    }
}


// ── Semantic Tokens: @template parameters ───────────────────────────────────

/**
 * @template TPen of Scaffolding\Pen
 */
class SemanticTemplateDemo
{
    /**
     * `TPen` and `Scaffolding\Pen` are both just words in a comment as far as
     * a grammar is concerned. PHPantom knows the first one is a template
     * parameter declared on the class and colors it as one, while the second
     * stays a class. Template tokens show up in contextual mode too.
     *
     * @param TPen $pen
     * @return TPen
     */
    public function keep(Scaffolding\Pen $pen): Scaffolding\Pen
    {
        return $pen;
    }
}


// ── Semantic Tokens: constants, enum cases, and static properties ───────────

class SemanticConstantDemo
{
    public function demo(): void
    {
        // Four accesses spelled `Class::member`, four different results:
        Scaffolding\User::TYPE_ADMIN;           // constant → `enumMember`, readonly + static
        Scaffolding\Status::Active;             // enum case → `enumMember` as well
        Scaffolding\StaticPropHolder::$shared;  // the `$` makes it a `property`, static
        Scaffolding\User::findByEmail('ada@example.com');   // method, static

        // In contextual mode the two constants are left to the editor's
        // grammar and only the static property and the static call are
        // emitted, since `static` is the part a grammar cannot infer.
    }
}


// ── Semantic Tokens: language builtins ──────────────────────────────────────
// `$this`, `self`, `static`, and `parent` carry the defaultLibrary modifier,
// so a theme can set them apart from names you declared yourself. `$this` is
// marked readonly on top of that. (full mode)

class SemanticBuiltinsDemo extends Scaffolding\ScaffoldingBasePenHolder
{
    public const int LIMIT = 10;

    public function demo(): int
    {
        $mine = $this->getPens();   // $this → `variable`, readonly + defaultLibrary
        $all = parent::getPens();   // parent resolves to the parent's own kind
        $cap = self::LIMIT;         // self → `type` + defaultLibrary
        $late = static::LIMIT;      // static → `type` + defaultLibrary

        return min(count($mine) + count($all), $cap, $late);
    }
}


// ── Semantic Tokens: parameters stand out from locals ───────────────────────
// This is the one contextual mode leans on hardest: `$subject` and `$local`
// are the same shape, and only one of them is something the caller passed in.

class SemanticParameterDemo
{
    public function demo(string $subject): string
    {
        $local = strtoupper($subject);   // $subject → `parameter`, $local → plain variable

        return $local;
    }
}


// ── Semantic Tokens: attributes ─────────────────────────────────────────────
// An attribute name is a class reference, but it reads as an annotation, so
// it gets the `decorator` token instead of `class`. Contextual mode leaves
// attributes alone entirely. (full mode)

#[Scaffolding\ClassOnlyAttr]
class SemanticAttributeDemo
{
    #[Scaffolding\MethodOnlyAttr]
    public function annotated(): void {}

    #[Scaffolding\PropertyOnlyAttr]
    public string $tagged = '';
}


// ── Semantic Tokens: types inside docblocks ─────────────────────────────────
// A comment is normally one flat token. PHPantom splits it around the
// symbols it recognizes inside, so the types in a docblock are colored as
// types rather than disappearing into the comment color. (full mode)

class SemanticDocblockDemo
{
    /**
     * @param Scaffolding\Pen $pen        `Scaffolding\Pen` is colored as a class
     * @param Scaffolding\Status $status  `Scaffolding\Status` as an enum
     * @return Scaffolding\Renderable     and this one as an interface
     */
    public function describe(Scaffolding\Pen $pen, Scaffolding\Status $status): Scaffolding\Renderable
    {
        return Scaffolding\createUser('Ada', 'ada@example.com');
    }
}


// ── Semantic Tokens: deprecated members ─────────────────────────────────────
// Anything PHPantom resolves to a deprecated declaration carries the
// `deprecated` modifier, which most themes render struck through, and it is
// one of the few things contextual mode does emit. The members in
// DeprecationDemo (diagnostics.php) are the ones to look at: opening that
// file shows the strikethrough on the same lines that report the warning.
