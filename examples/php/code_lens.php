<?php

/**
 * PHP Showcase — Code Lens
 *
 * The annotations rendered above a declaration.
 *
 * One of the demo files listed in README.md. Supporting fixtures live in
 * scaffolding/scaffolding.php (namespace Demo\Scaffolding), and the runtime
 * assertions that verify the type claims in the comments below live in
 * scaffolding/assertions.php.
 */

namespace Demo;

use Demo\Scaffolding;

// ── Code Lens: prototype method annotations ─────────────────────────────────
// Open this class and look at the gutter above each method. PHPantom shows
// clickable annotations ("↑ ParentClass::method" or "◆ Interface::method")
// that navigate to the parent/interface declaration.
class CodeLensDemo extends Scaffolding\ScaffoldingAbstractShape implements Scaffolding\ScaffoldingDrawable
{
    // ↑ Scaffolding\ScaffoldingAbstractShape::area  — click to jump to abstract declaration
    public function area(): float { return 3.14; }

    // ↑ Scaffolding\ScaffoldingAbstractShape::perimeter
    protected function perimeter(): float { return 6.28; }

    // ◆ Scaffolding\ScaffoldingDrawable::draw  — interface implementations use ◆
    public function draw(string $color, float $opacity = 1.0): void {}
}


// ── Code Lens: reference counts on declarations ─────────────────────────────
// Above a declaration PHPantom shows how many places use it, for a class,
// method, property, constant, or a function declared outside any class, so a
// plain helpers file gets them as well. Click the count to list the usages.

function codeLensFormatLabel(string $text): string
{
    return ucfirst($text);
}

// codeLensFormatLabel above shows "2 references": the two calls below.
$codeLensFirstLabel = codeLensFormatLabel('first');
$codeLensSecondLabel = codeLensFormatLabel('second');

// A function nothing calls shows "0 references", which is the quickest way
// to spot dead code in a procedural file.
function codeLensUnusedHelper(): void {}
