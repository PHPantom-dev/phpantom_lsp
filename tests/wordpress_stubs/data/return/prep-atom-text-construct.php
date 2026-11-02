<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function prep_atom_text_construct;
use function PHPStan\Testing\assertType;

assertType("array{'html'|'text'|'xhtml', string}", prep_atom_text_construct(''));
assertType("array{'html'|'text'|'xhtml', string}", prep_atom_text_construct('data'));
assertType("array{'html'|'text'|'xhtml', string}", prep_atom_text_construct(Faker::string()));
