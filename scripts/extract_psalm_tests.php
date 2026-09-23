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
 */

declare(strict_types=1);

$outputDir = null;
$files = [];
$processAll = false;

// Parse arguments
$args = array_slice($argv, 1);
for ($i = 0; $i < count($args); $i++) {
    if ($args[$i] === '--output-dir' && isset($args[$i + 1])) {
        $outputDir = $args[++$i];
    } elseif ($args[$i] === '--all') {
        $processAll = true;
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

        if (empty($assertions)) {
            continue;
        }

        // Filter out assertions with Psalm-specific types we don't support
        $filteredAssertions = filterAssertions($assertions);
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
            $assertCalls[] = sprintf("assertType('%s', %s);", addcslashes($type, "'\\"), $var);
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

        $code = $part['code'];

        // If the code starts with <?php, strip it since we already have one
        $code = preg_replace('/^<\?php\s*/', '', $code);

        // Wrap in namespace to isolate each test case
        $output .= sprintf("namespace %s {\n", $namespaceName);

        // Indent the code
        $codeLines = explode("\n", rtrim($code));
        foreach ($codeLines as $line) {
            if (trim($line) === '') {
                $output .= "\n";
            } else {
                $output .= "    " . $line . "\n";
            }
        }

        // Add assertType calls
        $output .= "\n";
        foreach ($part['asserts'] as $assert) {
            $output .= "    " . $assert . "\n";
        }

        $output .= "}\n\n";
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
    $cases = [];

    // We parse this with a state machine rather than eval() for safety.
    // Strategy: find each test case block by matching the pattern:
    //   'testName' => [
    //       'code' => '...',
    //       'assertions' => ['$var' => 'Type', ...],
    //   ],

    // Find all test case blocks using a regex-based approach.
    // First, locate the providerValidCodeParse method.
    $validStart = strpos($source, 'providerValidCodeParse');
    if ($validStart === false) {
        return $cases;
    }

    // Work from providerValidCodeParse to the end of its return array.
    $workingSource = substr($source, $validStart);

    // Find test case names: lines like "'testCaseName' => ["
    preg_match_all("/^\\s*'([a-zA-Z0-9_]+)'\\s*=>\\s*\\[/m", $workingSource, $nameMatches, PREG_OFFSET_CAPTURE);

    for ($i = 0; $i < count($nameMatches[0]); $i++) {
        $testName = $nameMatches[1][$i][0];
        $blockStart = $nameMatches[0][$i][1];

        // Find the end of this test case block by matching brackets
        $blockEnd = findMatchingBracket($workingSource, $blockStart);
        if ($blockEnd === false) {
            continue;
        }

        $block = substr($workingSource, $blockStart, $blockEnd - $blockStart + 1);

        // Extract 'code' value
        $code = extractStringValue($block, 'code');
        if ($code === null) {
            continue;
        }

        // Extract 'assertions' array
        $assertions = extractAssertions($block);

        // Extract 'php_version' if present
        $phpVersion = extractStringValue($block, 'php_version');

        $cases[$testName] = [
            'code' => $code,
            'assertions' => $assertions,
            'php_version' => $phpVersion,
        ];
    }

    return $cases;
}

/**
 * Find the position of the closing bracket that matches the first '[' found
 * at or after $startPos in $source.
 */
function findMatchingBracket(string $source, int $startPos): int|false
{
    $pos = strpos($source, '[', $startPos);
    if ($pos === false) {
        return false;
    }

    $depth = 0;
    $inSingleQuote = false;
    $inDoubleQuote = false;
    $len = strlen($source);

    for ($i = $pos; $i < $len; $i++) {
        $ch = $source[$i];
        $prev = $i > 0 ? $source[$i - 1] : '';

        if ($inSingleQuote) {
            if ($ch === "'" && $prev !== '\\') {
                $inSingleQuote = false;
            }
            continue;
        }

        if ($inDoubleQuote) {
            if ($ch === '"' && $prev !== '\\') {
                $inDoubleQuote = false;
            }
            continue;
        }

        if ($ch === "'") {
            $inSingleQuote = true;
        } elseif ($ch === '"') {
            $inDoubleQuote = true;
        } elseif ($ch === '[') {
            $depth++;
        } elseif ($ch === ']') {
            $depth--;
            if ($depth === 0) {
                return $i;
            }
        }
    }

    return false;
}

/**
 * Extract a string value for a given key from a test case block.
 * Handles single-quoted strings with concatenation across lines.
 */
function extractStringValue(string $block, string $key): ?string
{
    // Match: 'key' => '...'
    $pattern = "/'" . preg_quote($key, '/') . "'\\s*=>\\s*'/";
    if (!preg_match($pattern, $block, $m, PREG_OFFSET_CAPTURE)) {
        return null;
    }

    $valueStart = $m[0][1] + strlen($m[0][0]) - 1; // position of opening quote

    // Collect the full string value, handling multi-line strings.
    // Psalm uses single-quoted strings. We need to handle escaped single quotes.
    $result = '';
    $i = $valueStart + 1; // skip opening quote
    $len = strlen($block);

    while ($i < $len) {
        $ch = $block[$i];

        if ($ch === '\\' && $i + 1 < $len) {
            $next = $block[$i + 1];
            if ($next === "'") {
                $result .= "'";
                $i += 2;
                continue;
            } elseif ($next === '\\') {
                $result .= '\\';
                $i += 2;
                continue;
            }
        }

        if ($ch === "'") {
            // End of this string segment. Check for concatenation.
            $rest = ltrim(substr($block, $i + 1));
            if (str_starts_with($rest, ". '") || str_starts_with($rest, ".'")) {
                // String concatenation — find the next quote and continue
                $nextQuote = strpos($block, "'", $i + 1);
                if ($nextQuote !== false) {
                    $nextQuote = strpos($block, "'", $nextQuote + 1);
                }
                // Actually, just skip to next single quote after the dot
                $dotPos = strpos($block, '.', $i + 1);
                if ($dotPos !== false) {
                    $nextQuoteStart = strpos($block, "'", $dotPos + 1);
                    if ($nextQuoteStart !== false) {
                        $i = $nextQuoteStart + 1;
                        continue;
                    }
                }
            }
            break;
        }

        $result .= $ch;
        $i++;
    }

    return $result;
}

/**
 * Extract assertions array from a test case block.
 * Format: 'assertions' => ['$var' => 'Type', '$var2' => 'Type2'],
 *
 * @return array<string, string>
 */
function extractAssertions(string $block): array
{
    $assertions = [];

    // Find 'assertions' => [
    $pos = strpos($block, "'assertions'");
    if ($pos === false) {
        return $assertions;
    }

    $bracketStart = strpos($block, '[', $pos);
    if ($bracketStart === false) {
        return $assertions;
    }

    $bracketEnd = findMatchingBracket($block, $pos);
    if ($bracketEnd === false) {
        return $assertions;
    }

    $assertBlock = substr($block, $bracketStart, $bracketEnd - $bracketStart + 1);

    // Match each '$var' => 'Type' pair
    preg_match_all("/'(\\$[a-zA-Z_][a-zA-Z0-9_]*)'\\s*=>\\s*'([^']*)'/", $assertBlock, $matches);

    for ($i = 0; $i < count($matches[0]); $i++) {
        $assertions[$matches[1][$i]] = $matches[2][$i];
    }

    return $assertions;
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