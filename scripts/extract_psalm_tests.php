<?php

/**
 * Psalm Test Extractor for PHPantom
 *
 * Parses Psalm's PHP test classes and extracts test cases with type assertions
 * into standalone .php files compatible with PHPantom's assert_type_runner.rs.
 *
 * Psalm format:
 *   'testName' => [
 *       'code' => '<?php ... $var = expr; ',
 *       'assertions' => ['$var' => 'ExpectedType'],
 *   ],
 *
 * Output format (matching PHPStan assertType):
 *   <?php
 *   // Test: testName
 *   // Source: Psalm/Tests/TypeReconciliation/ConditionalTest.php
 *   ... code ...
 *   assertType('ExpectedType', $var);
 *
 * Usage:
 *   php scripts/extract_psalm_tests.php [psalm_test_file.php ...] [--output-dir DIR]
 *   php scripts/extract_psalm_tests.php --all [--output-dir DIR]
 *
 * With --all, processes all 3.5A priority files from the test-porting plan.
 *
 * With --mark-widened, every assertion Psalm checks in its widened spelling
 * (a key without `===`, where literal ints and strings print as `int` and
 * `string`) gets a trailing `// psalm-widened` comment, so curation can tell
 * a literal PHPantom keeps from a real mismatch.
 */

declare(strict_types=1);

$outputDir = null;
$files = [];
$processAll = false;
$markWidened = false;

// Parse arguments
$args = array_slice($argv, 1);
for ($i = 0; $i < count($args); $i++) {
    if ($args[$i] === '--output-dir' && isset($args[$i + 1])) {
        $outputDir = $args[++$i];
    } elseif ($args[$i] === '--all') {
        $processAll = true;
    } elseif ($args[$i] === '--mark-widened') {
        $markWidened = true;
    } else {
        $files[] = $args[$i];
    }
}

$outputDir = $outputDir ?? __DIR__ . '/../tests/psalm_assertions';

$psalmTestBase = realpath(__DIR__ . '/../references/psalm/tests');

// 3.5A priority files (clearly LSP-relevant)
$allFiles = [
    'TypeReconciliation/ConditionalTest.php',
    'TypeReconciliation/IssetTest.php',
    'TypeReconciliation/ArrayKeyExistsTest.php',
    'TypeReconciliation/TypeTest.php',
    'TypeReconciliation/InArrayTest.php',
    'TypeReconciliation/ScopeTest.php',
    'ArrayAssignmentTest.php',
    'ArrayAccessTest.php',
    'ClosureTest.php',
    'EnumTest.php',
    'GeneratorTest.php',
    'MixinAnnotationTest.php',
    'MagicMethodAnnotationTest.php',
    'MagicPropertyTest.php',
    'MethodCallTest.php',
    'PropertyTypeTest.php',
    'ReturnTypeTest.php',
    'Loop/ForeachTest.php',
    'Loop/DoTest.php',
    'Loop/WhileTest.php',
    'Loop/ForTest.php',
    'AnnotationTest.php',
    'DocblockInheritanceTest.php',
    'MatchTest.php',
    'SwitchTypeTest.php',
    'IntersectionTypeTest.php',
    'NativeIntersectionsTest.php',
    'ClassLikeStringTest.php',
    'TraitTest.php',
    'AssertAnnotationTest.php',
    'TypeAnnotationTest.php',
    'TryCatchTest.php',
    'CastTest.php',
    'Template/ClassTemplateTest.php',
    'Template/ClassTemplateExtendsTest.php',
    'Template/FunctionTemplateTest.php',
    'Template/ConditionalReturnTypeTest.php',
    'Template/FunctionClassStringTemplateTest.php',
    'Template/FunctionTemplateAssertTest.php',
    'IfThisIsTest.php',
    'ThisOutTest.php',
    // 3.5B: Partially relevant files
    'ArrayFunctionCallTest.php',
    'FunctionCallTest.php',
    'BinaryOperationTest.php',
    'ConstantTest.php',
    'CallableTest.php',
    'TypeReconciliation/EmptyTest.php',
    'TypeReconciliation/RedundantConditionTest.php',
    'TypeReconciliation/TypeAlgebraTest.php',
];

if ($processAll) {
    foreach ($allFiles as $rel) {
        $full = $psalmTestBase . '/' . $rel;
        if (file_exists($full)) {
            $files[] = $full;
        } else {
            fprintf(STDERR, "WARNING: File not found: %s\n", $full);
        }
    }
}

if (empty($files)) {
    fprintf(STDERR, "Usage: php %s [--all | file1.php file2.php ...] [--output-dir DIR]\n", $argv[0]);
    exit(1);
}

if (!is_dir($outputDir)) {
    mkdir($outputDir, 0755, true);
}

$totalExtracted = 0;
$totalSkipped = 0;
$totalFiles = 0;

foreach ($files as $file) {
    if (!file_exists($file)) {
        fprintf(STDERR, "ERROR: File not found: %s\n", $file);
        continue;
    }

    $source = file_get_contents($file);
    $relativePath = str_replace($psalmTestBase . '/', '', realpath($file));

    $testCases = extractTestCases($source);

    if (empty($testCases)) {
        fprintf(STDERR, "  No test cases with assertions found in %s\n", $relativePath);
        continue;
    }

    // Generate output filename from the Psalm test path
    // e.g. TypeReconciliation/ConditionalTest.php -> type_reconciliation_conditional.php
    $outBasename = pathToOutputName($relativePath);

    $extracted = 0;
    $skipped = 0;
    $outputParts = [];

    foreach ($testCases as $testName => $testCase) {
        $code = $testCase['code'];
        $assertions = $testCase['assertions'];
        $phpVersion = $testCase['php_version'] ?? null;

        // Psalm's own harness skips a case whose name starts with `SKIPPED-`.
        if (empty($assertions) || str_starts_with($testName, 'SKIPPED-')) {
            continue;
        }

        // A trailing `===` asks Psalm for its exact spelling (literal values
        // kept), which is what PHPantom prints, so the key is the bare variable.
        $normalized = [];
        $widened = [];
        foreach ($assertions as $key => $type) {
            $var = preg_replace('/===$/', '', $key);
            if (str_starts_with($var, '$') && !str_starts_with($var, '$this')) {
                $normalized[$var] = $type;
                $widened[$var] = $var === $key;
            }
        }

        // Filter out assertions with Psalm-specific types we don't support
        $filteredAssertions = filterAssertions($normalized);
        if (empty($filteredAssertions)) {
            $skipped++;
            continue;
        }

        // Normalize the code: strip leading indentation from Psalm's heredoc style
        $code = normalizeCode($code);

        // Build assertType calls
        $assertCalls = [];
        foreach ($filteredAssertions as $var => $type) {
            $type = normalizeType($type);
            $assertCalls[] = sprintf("assertType('%s', %s);", addcslashes($type, "'\\"), $var)
                . ($markWidened && $widened[$var] ? ' // psalm-widened' : '');
        }

        $outputParts[] = [
            'name' => $testName,
            'code' => $code,
            'asserts' => $assertCalls,
            'php_version' => $phpVersion,
        ];

        $extracted++;
    }

    if (empty($outputParts)) {
        fprintf(STDERR, "  No usable assertions in %s (skipped %d)\n", $relativePath, $skipped);
        continue;
    }

    // Write one file per Psalm test class, with all test cases concatenated.
    // Each test case is wrapped in a namespace to avoid name collisions.
    $output = "<?php\n";
    $output .= sprintf("// Source: Psalm %s\n", $relativePath);
    $output .= "// Auto-extracted by scripts/extract_psalm_tests.php\n";
    $output .= "// Do not edit manually — re-run the extraction script instead.\n\n";

    $caseIndex = 0;
    foreach ($outputParts as $part) {
        $caseIndex++;
        $namespaceName = sprintf("PsalmTest_%s_%d", preg_replace('/[^a-zA-Z0-9]/', '_', $outBasename), $caseIndex);

        $output .= sprintf("// Test: %s\n", $part['name']);
        if ($part['php_version']) {
            $output .= sprintf("// Requires PHP %s\n", $part['php_version']);
        }

        // If the code starts with <?php, strip it since we already have one
        $code = preg_replace('/^\s*<\?php\s*/', '', $part['code']);
        $output .= wrapCase($code, $namespaceName, $part['asserts']);
    }

    $outPath = $outputDir . '/' . $outBasename . '.php';
    file_put_contents($outPath, $output);

    $totalExtracted += $extracted;
    $totalSkipped += $skipped;
    $totalFiles++;

    fprintf(STDERR, "  %s: %d test cases extracted, %d skipped -> %s\n",
        $relativePath, $extracted, $skipped, basename($outPath));
}

fprintf(STDERR, "\nTotal: %d files processed, %d test cases extracted, %d skipped\n",
    $totalFiles, $totalExtracted, $totalSkipped);
fprintf(STDERR, "Output directory: %s\n", realpath($outputDir) ?: $outputDir);


// ─── Output ─────────────────────────────────────────────────────────────────

/**
 * Emit one test case as bracketed namespace blocks, so cases sharing a file
 * cannot see each other's declarations. A case with no namespace of its own
 * gets a unique one; a case that declares namespaces keeps them (unbracketed
 * ones are rewritten into the bracketed form a multi-case file needs) and its
 * assertions go in the last block, where Psalm reads them.
 *
 * @param list<string> $asserts
 */
function wrapCase(string $code, string $namespaceName, array $asserts): string
{
    $code = rtrim($code);
    $assertLines = implode('', array_map(static fn($a) => "    $a\n", $asserts));

    if (preg_match('/^\s*namespace\b[^;{]*\{/m', $code)) {
        return $code . "\n\nnamespace {\n" . $assertLines . "}\n\n";
    }

    if (preg_match('/^\s*namespace\b[^;{]*;/m', $code)) {
        $parts = preg_split('/^\s*(namespace\b[^;{]*);/m', $code, -1, PREG_SPLIT_DELIM_CAPTURE);
        $out = trim($parts[0]) === '' ? '' : "namespace {\n" . indent($parts[0]) . "}\n";
        for ($i = 1; $i < count($parts); $i += 2) {
            $out .= $parts[$i] . " {\n" . indent($parts[$i + 1]);
            if ($i + 2 >= count($parts)) {
                $out .= "\n" . $assertLines;
            }
            $out .= "}\n";
        }
        return $out . "\n";
    }

    return sprintf("namespace %s {\n", $namespaceName) . indent($code) . "\n" . $assertLines . "}\n\n";
}

function indent(string $code): string
{
    $out = '';
    foreach (explode("\n", trim($code, "\n")) as $line) {
        $out .= trim($line) === '' ? "\n" : "    " . $line . "\n";
    }
    return $out;
}

// ─── Extraction functions ───────────────────────────────────────────────────

/**
 * Extract test cases from a Psalm test PHP file.
 *
 * Looks for providerValidCodeParse() method which returns an array of test
 * cases. Each test case has 'code', optional 'assertions', optional
 * 'php_version', etc.
 *
 * @return array<string, array{code: string, assertions: array<string, string>, php_version: ?string}>
 */
function extractTestCases(string $source): array
{
    // The provider is read with PHP's tokenizer and a small literal parser
    // rather than eval(), so nothing in the test file is ever executed.
    $tokens = array_values(array_filter(
        token_get_all($source),
        static fn($t) => !is_array($t) || !in_array($t[0], [T_WHITESPACE, T_COMMENT, T_DOC_COMMENT], true),
    ));

    $body = providerBody($tokens, 'providerValidCodeParse');
    if ($body === null) {
        return [];
    }
    [$pos, $end] = $body;

    $cases = [];
    // A test case is `'name' => [...]`, either an element of the returned
    // array or the operand of `yield`. Any array value whose `code` entry is
    // a string counts; everything else in the method body is skipped over.
    while ($pos < $end) {
        $t = $tokens[$pos];
        if (is_array($t) && $t[0] === T_CONSTANT_ENCAPSED_STRING
            && isOp($tokens[$pos + 1] ?? null, T_DOUBLE_ARROW)
            && isArrayStart($tokens[$pos + 2] ?? null)
        ) {
            $next = $pos + 2;
            $value = parseValue($tokens, $next);
            if (is_array($value) && isset($value['code']) && is_string($value['code'])) {
                $name = decodeString($t[1]);
                $assertions = [];
                foreach (is_array($value['assertions'] ?? null) ? $value['assertions'] : [] as $k => $v) {
                    if (is_string($k) && is_string($v)) {
                        $assertions[$k] = $v;
                    }
                }
                $cases[$name] = [
                    'code' => $value['code'],
                    'assertions' => $assertions,
                    'php_version' => is_string($value['php_version'] ?? null) ? $value['php_version'] : null,
                ];
                $pos = $next;
                continue;
            }
        }
        $pos++;
    }

    return $cases;
}

/**
 * Token range [start, end) of the named method's body.
 *
 * @return array{int, int}|null
 */
function providerBody(array $tokens, string $method): ?array
{
    $count = count($tokens);
    for ($i = 0; $i < $count; $i++) {
        if (isOp($tokens[$i], T_FUNCTION) && is_array($tokens[$i + 1] ?? null) && $tokens[$i + 1][1] === $method) {
            for ($j = $i; $j < $count && $tokens[$j] !== '{'; $j++);
            $depth = 0;
            for ($k = $j; $k < $count; $k++) {
                $tok = $tokens[$k];
                if ($tok === '{' || isOp($tok, T_CURLY_OPEN) || isOp($tok, T_DOLLAR_OPEN_CURLY_BRACES)) {
                    $depth++;
                } elseif ($tok === '}') {
                    if (--$depth === 0) {
                        return [$j + 1, $k];
                    }
                }
            }
            return null;
        }
    }
    return null;
}

function isOp(mixed $token, int $id): bool
{
    return is_array($token) && $token[0] === $id;
}

function isArrayStart(mixed $token): bool
{
    return $token === '[' || isOp($token, T_ARRAY);
}

/**
 * Parse one literal value starting at $pos, leaving $pos just past it.
 * Strings (including concatenations, heredocs and nowdocs) and arrays are
 * decoded; anything else (constants, class constants, numbers) is skipped
 * and returned as null.
 */
function parseValue(array $tokens, int &$pos): mixed
{
    $value = parseAtom($tokens, $pos);
    while (($tokens[$pos] ?? null) === '.') {
        $pos++;
        $rhs = parseAtom($tokens, $pos);
        $value = is_string($value) && is_string($rhs) ? $value . $rhs : null;
    }
    return $value;
}

function parseAtom(array $tokens, int &$pos): mixed
{
    $t = $tokens[$pos] ?? null;

    if (isOp($t, T_CONSTANT_ENCAPSED_STRING)) {
        $pos++;
        return decodeString($t[1]);
    }

    if (isOp($t, T_START_HEREDOC)) {
        return parseHeredoc($tokens, $pos);
    }

    if (isArrayStart($t)) {
        $close = $t === '[' ? ']' : ')';
        $pos += $t === '[' ? 1 : 2;
        $result = [];
        $index = 0;
        while (($tokens[$pos] ?? $close) !== $close) {
            $item = parseValue($tokens, $pos);
            if (isOp($tokens[$pos] ?? null, T_DOUBLE_ARROW)) {
                $pos++;
                $val = parseValue($tokens, $pos);
                if (is_string($item) || is_int($item)) {
                    $result[$item] = $val;
                }
            } else {
                $result[$index++] = $item;
            }
            if (($tokens[$pos] ?? null) === ',') {
                $pos++;
            }
        }
        $pos++;
        return $result;
    }

    // Unknown expression: skip to the next `,`, `=>`, `.` or closer at this depth.
    $depth = 0;
    while (isset($tokens[$pos])) {
        $tok = $tokens[$pos];
        if ($tok === '(' || $tok === '[') {
            $depth++;
        } elseif ($tok === ')' || $tok === ']') {
            if ($depth === 0) {
                break;
            }
            $depth--;
        } elseif ($depth === 0 && ($tok === ',' || $tok === '.' || $tok === ';' || isOp($tok, T_DOUBLE_ARROW))) {
            break;
        }
        $pos++;
    }
    return null;
}

function parseHeredoc(array $tokens, int &$pos): ?string
{
    $isNowdoc = str_contains($tokens[$pos][1], "'");
    $pos++;
    $raw = '';
    $plain = true;
    while (isset($tokens[$pos]) && !isOp($tokens[$pos], T_END_HEREDOC)) {
        if (isOp($tokens[$pos], T_ENCAPSED_AND_WHITESPACE)) {
            $raw .= $tokens[$pos][1];
        } else {
            $plain = false;
        }
        $pos++;
    }
    $closing = $tokens[$pos][1] ?? '';
    $pos++;
    if (!$plain) {
        return null;
    }

    // PHP 7.3 flexible heredoc: the closing marker's indentation is removed
    // from every line, and the newline before the marker is not content.
    $indent = strlen($closing) - strlen(ltrim($closing));
    $lines = explode("\n", preg_replace('/\n$/', '', $raw));
    $lines = array_map(static fn($l) => substr($l, min($indent, strlen($l) - strlen(ltrim($l)))), $lines);
    $text = implode("\n", $lines);

    return $isNowdoc ? $text : decodeDoubleQuoted($text);
}

function decodeString(string $literal): string
{
    $quote = $literal[0];
    $inner = substr($literal, 1, -1);
    if ($quote === "'") {
        return preg_replace_callback("/\\\\([\\\\'])/", static fn($m) => $m[1], $inner);
    }
    return decodeDoubleQuoted($inner);
}

function decodeDoubleQuoted(string $inner): string
{
    $map = ['n' => "\n", 't' => "\t", 'r' => "\r", '\\' => '\\', '"' => '"', '$' => '$', 'e' => "\e", 'v' => "\v", 'f' => "\f", '0' => "\0"];
    return preg_replace_callback('/\\\\(.)/s', static fn($m) => $map[$m[1]] ?? $m[0], $inner);
}

/**
 * Filter out assertions that use Psalm-specific types PHPantom doesn't support.
 *
 * @param array<string, string> $assertions
 * @return array<string, string>
 */
function filterAssertions(array $assertions): array
{
    $filtered = [];

    foreach ($assertions as $var => $type) {
        // Skip Psalm-specific types
        if (preg_match('/\b(non-empty-|non-falsy-|lowercase-|uppercase-|numeric-string|positive-int|negative-int|int<|literal-|class-string-map|closed-resource|pure-|no-return)/', $type)) {
            continue;
        }

        // Skip literal bool values (PHPantom has no LiteralValue bool variant)
        if ($type === 'true' || $type === 'false') {
            continue;
        }

        $filtered[$var] = $type;
    }

    return $filtered;
}

/**
 * Normalize Psalm type strings to PHPantom equivalents.
 */
function normalizeType(string $type): string
{
    // Psalm uses 'list<T>' which PHPantom treats as 'array<int, T>'
    // Keep it as-is for now; the runner's normalize function handles this.

    // Psalm uses 'array-key' — keep as-is, runner normalizes.

    // Remove 'Psalm\Tests\...' namespace prefixes if any leaked in
    $type = preg_replace('/Psalm\\\\Tests\\\\[A-Za-z\\\\]*\\\\/', '', $type);

    return $type;
}

/**
 * Normalize code: strip common leading indentation from Psalm's indented heredoc style.
 */
function normalizeCode(string $code): string
{
    $lines = explode("\n", $code);

    // Find minimum indentation (ignoring empty lines and <?php line)
    $minIndent = PHP_INT_MAX;
    foreach ($lines as $line) {
        if (trim($line) === '' || trim($line) === '<?php') {
            continue;
        }
        $stripped = ltrim($line);
        if ($stripped === '') {
            continue;
        }
        $indent = strlen($line) - strlen($stripped);
        $minIndent = min($minIndent, $indent);
    }

    if ($minIndent === PHP_INT_MAX || $minIndent === 0) {
        return $code;
    }

    // Strip common indentation
    $result = [];
    foreach ($lines as $line) {
        if (trim($line) === '') {
            $result[] = '';
        } else {
            $result[] = substr($line, min($minIndent, strlen($line) - strlen(ltrim($line))));
        }
    }

    return implode("\n", $result);
}

/**
 * Convert a Psalm test path to an output filename.
 * e.g. "TypeReconciliation/ConditionalTest.php" -> "type_reconciliation_conditional"
 */
function pathToOutputName(string $path): string
{
    // Remove .php extension
    $name = preg_replace('/\.php$/', '', $path);

    // Remove "Test" suffix
    $name = preg_replace('/Test$/', '', $name);

    // Convert PascalCase to snake_case
    $name = preg_replace('/([a-z])([A-Z])/', '$1_$2', $name);

    // Convert path separators to underscores
    $name = str_replace('/', '_', $name);

    return strtolower($name);
}