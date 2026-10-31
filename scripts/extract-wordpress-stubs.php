<?php

/**
 * Copy the slice of `php-stubs/wordpress-stubs` a set of fixtures touches.
 *
 * The WordPress stubs are one file of some six megabytes, too large to keep
 * whole in the repository for the fixture project `tests/wordpress_stubs/`.
 * This keeps every declaration a fixture names: the functions and classes
 * its code and its expected types mention, the classes those functions
 * mention, and the parents, interfaces, and traits of every class kept.
 * Declarations are copied verbatim, docblocks included, in the order and
 * namespace blocks of the original.
 *
 * The stubs file is read as tokens, never loaded.
 *
 * Usage:
 *   php scripts/extract-wordpress-stubs.php <wordpress-stubs.php> <out-file> <fixture-path>...
 *
 * Each fixture path is a PHP file or a directory searched for PHP files.
 */

if ($argc < 4) {
    fwrite(STDERR, "usage: php {$argv[0]} <wordpress-stubs.php> <out-file> <fixture-path>...\n");
    exit(1);
}

[, $stubsFile, $outFile] = $argv;
$fixturePaths = array_slice($argv, 3);

$blocks = parse_stubs((string) file_get_contents($stubsFile));

/** @var array<string, array{block: int, item: int}> $functions lowercased FQN => position */
$functions = [];
/** @var array<string, array{block: int, item: int}> $classes lowercased FQN => position */
$classes = [];
foreach ($blocks as $b => $block) {
    foreach ($block['items'] as $i => $item) {
        if ($item['kind'] === 'function') {
            $functions[strtolower($item['name'])] = ['block' => $b, 'item' => $i];
        } elseif ($item['kind'] === 'class') {
            $classes[strtolower($item['name'])] = ['block' => $b, 'item' => $i];
        }
    }
}

$seeds = [];
foreach ($fixturePaths as $path) {
    foreach (php_files($path) as $file) {
        foreach (mentioned_names((string) file_get_contents($file)) as $name) {
            $seeds[$name] = true;
        }
    }
}

$keep = [];
$classQueue = [];
foreach (array_keys($seeds) as $name) {
    $key = strtolower(ltrim((string) $name, '\\'));
    if (isset($functions[$key])) {
        $pos = $functions[$key];
        $keep[$pos['block']][$pos['item']] = true;
        // The classes a kept function's signature and docblock name.
        foreach (mentioned_names($blocks[$pos['block']]['items'][$pos['item']]['text']) as $mentioned) {
            $classQueue[] = $mentioned;
        }
    }
    $classQueue[] = $key;
}

$seenClasses = [];
while ($classQueue !== []) {
    $key = strtolower(ltrim((string) array_shift($classQueue), '\\'));
    if (isset($seenClasses[$key]) || !isset($classes[$key])) {
        continue;
    }
    $seenClasses[$key] = true;
    $pos = $classes[$key];
    $keep[$pos['block']][$pos['item']] = true;
    foreach ($blocks[$pos['block']]['items'][$pos['item']]['ancestors'] as $ancestor) {
        $classQueue[] = $ancestor;
    }
}

$out = "<?php\n/**\n * The declarations of php-stubs/wordpress-stubs the fixtures touch, copied\n"
    . " * by scripts/extract-wordpress-stubs.php.\n *\n * @see https://github.com/php-stubs/wordpress-stubs\n */\n";
$kept = 0;
foreach ($blocks as $b => $block) {
    if (!isset($keep[$b])) {
        continue;
    }
    $out .= "\n" . $block['header'] . "\n";
    foreach ($block['items'] as $i => $item) {
        if (isset($keep[$b][$i])) {
            $out .= '    ' . $item['text'] . "\n";
            $kept++;
        }
    }
    $out .= "}\n";
}
file_put_contents($outFile, $out);
fwrite(STDERR, "kept $kept declarations\n");

/**
 * The stubs as namespace blocks, each a list of its top-level declarations.
 *
 * @return list<array{header: string, items: list<array{kind: string, name: string, text: string, ancestors: list<string>}>}>
 */
function parse_stubs(string $source): array
{
    $tokens = token_get_all($source);
    $count = count($tokens);
    $blocks = [];
    for ($i = 0; $i < $count; $i++) {
        if (!is_array($tokens[$i]) || $tokens[$i][0] !== T_NAMESPACE) {
            continue;
        }
        $namespace = '';
        for ($i++; $i < $count && $tokens[$i] !== '{'; $i++) {
            if (is_array($tokens[$i]) && in_array($tokens[$i][0], [T_STRING, T_NAME_QUALIFIED], true)) {
                $namespace = $tokens[$i][1];
            }
        }
        $header = $namespace === '' ? 'namespace {' : "namespace $namespace {";
        $items = [];
        $start = null;
        for ($i++; $i < $count && $tokens[$i] !== '}'; $i++) {
            $token = $tokens[$i];
            if (is_array($token) && $token[0] === T_WHITESPACE) {
                continue;
            }
            $start ??= $i;
            if (is_array($token) && $token[0] === T_FUNCTION) {
                $name = next_name($tokens, $i);
                $end = skip_braces($tokens, $i);
                $items[] = item('function', $namespace, $name, $tokens, $start, $end, []);
                $i = $end;
                $start = null;
            } elseif (is_array($token) && in_array($token[0], [T_CLASS, T_INTERFACE, T_TRAIT, T_ENUM], true)) {
                $name = next_name($tokens, $i);
                $ancestors = [];
                for ($j = $i + 1; $tokens[$j] !== '{'; $j++) {
                    if (is_array($tokens[$j]) && in_array($tokens[$j][0], [T_STRING, T_NAME_QUALIFIED, T_NAME_FULLY_QUALIFIED], true) && $tokens[$j][1] !== $name) {
                        $ancestors[] = qualify($tokens[$j][1], $namespace);
                    }
                }
                $end = skip_braces($tokens, $i);
                // `use Trait;` directly inside the class body.
                $depth = 0;
                for ($k = $j; $k < $end; $k++) {
                    $text = is_array($tokens[$k]) ? $tokens[$k][1] : $tokens[$k];
                    if ($text === '{' || (is_array($tokens[$k]) && in_array($tokens[$k][0], [T_CURLY_OPEN, T_DOLLAR_OPEN_CURLY_BRACES], true))) {
                        $depth++;
                    } elseif ($text === '}') {
                        $depth--;
                    } elseif ($depth === 1 && is_array($tokens[$k]) && $tokens[$k][0] === T_USE) {
                        for ($k++; $tokens[$k] !== ';' && $tokens[$k] !== '{'; $k++) {
                            if (is_array($tokens[$k]) && in_array($tokens[$k][0], [T_STRING, T_NAME_QUALIFIED, T_NAME_FULLY_QUALIFIED], true)) {
                                $ancestors[] = qualify($tokens[$k][1], $namespace);
                            }
                        }
                    }
                }
                $items[] = item('class', $namespace, $name, $tokens, $start, $end, $ancestors);
                $i = $end;
                $start = null;
            } elseif ($token === ';') {
                // A top-level `const` or other statement.
                $items[] = item('statement', $namespace, '', $tokens, $start, $i, []);
                $start = null;
            } elseif ($token === '{') {
                $end = skip_braces($tokens, $i - 1);
                $items[] = item('statement', $namespace, '', $tokens, $start, $end, []);
                $i = $end;
                $start = null;
            }
        }
        $blocks[] = ['header' => $header, 'items' => $items];
    }

    return $blocks;
}

/** @return array{kind: string, name: string, text: string, ancestors: list<string>} */
function item(string $kind, string $namespace, string $name, array $tokens, int $start, int $end, array $ancestors): array
{
    $text = '';
    for ($i = $start; $i <= $end; $i++) {
        $text .= is_array($tokens[$i]) ? $tokens[$i][1] : $tokens[$i];
    }

    return [
        'kind' => $kind,
        'name' => $namespace === '' ? $name : "$namespace\\$name",
        'text' => $text,
        'ancestors' => $ancestors,
    ];
}

function next_name(array $tokens, int $i): string
{
    for ($i++; ; $i++) {
        if (is_array($tokens[$i]) && $tokens[$i][0] === T_STRING) {
            return $tokens[$i][1];
        }
    }
}

/** The index of the `}` closing the first `{` after `$i`. */
function skip_braces(array $tokens, int $i): int
{
    $depth = 0;
    $parens = 0;
    for ($i++; ; $i++) {
        $token = $tokens[$i];
        $text = is_array($token) ? $token[1] : $token;
        if ($text === '(') {
            $parens++;
        } elseif ($text === ')') {
            $parens--;
        } elseif ($text === '{' || (is_array($token) && in_array($token[0], [T_CURLY_OPEN, T_DOLLAR_OPEN_CURLY_BRACES], true))) {
            $depth++;
        } elseif ($text === '}') {
            $depth--;
            if ($depth === 0) {
                return $i;
            }
        } elseif ($text === ';' && $depth === 0 && $parens === 0) {
            return $i;
        }
    }
}

function qualify(string $name, string $namespace): string
{
    if ($name[0] === '\\') {
        return substr($name, 1);
    }

    return $namespace === '' ? $name : "$namespace\\$name";
}

/**
 * Every name the source mentions, in code, in docblocks, or in strings (an
 * `assertType()` expectation names classes inside a string).
 *
 * @return list<string>
 */
function mentioned_names(string $source): array
{
    preg_match_all('/\\\\?[A-Za-z_][A-Za-z0-9_]*(?:\\\\[A-Za-z_][A-Za-z0-9_]*)*/', $source, $matches);

    return array_values(array_unique($matches[0]));
}

/** @return list<string> */
function php_files(string $path): array
{
    if (is_file($path)) {
        return [$path];
    }
    $files = [];
    $iterator = new RecursiveIteratorIterator(new RecursiveDirectoryIterator($path, FilesystemIterator::SKIP_DOTS));
    foreach ($iterator as $file) {
        if ($file->getExtension() === 'php') {
            $files[] = $file->getPathname();
        }
    }
    sort($files);

    return $files;
}
