<?php

/**
 * Copy framework classes out of a Composer install as stubs: every
 * declaration and docblock kept, every function body emptied.
 *
 * The PHPStan extension fixture projects (`tests/phpstan_symfony/`,
 * `tests/phpstan_doctrine/`) carry the slice of Symfony and Doctrine their
 * fixtures touch this way, so the fixtures measure what the framework
 * itself declares without a `composer install` in the test run.
 *
 * Usage:
 *   php scripts/extract-framework-stubs.php <vendor-dir> <out-dir> <class-list-file>
 *
 * The class list names one class, interface, or trait per line (`#` starts
 * a comment). Their parents, interfaces, and traits are copied as well,
 * except for a class that cannot be loaded because one of them is not
 * installed.
 * Each file lands at `<out-dir>/<package>/<path inside the package>`, and
 * each package's license file beside its copies.
 */

if ($argc !== 4) {
    fwrite(STDERR, "usage: php {$argv[0]} <vendor-dir> <out-dir> <class-list-file>\n");
    exit(1);
}

[, $vendorDir, $outDir, $listFile] = $argv;
$vendorDir = (string) realpath($vendorDir);
$loader = require $vendorDir . '/autoload.php';

$queue = [];
foreach (file($listFile, FILE_IGNORE_NEW_LINES) as $line) {
    $line = trim((string) preg_replace('/#.*/', '', $line));
    if ($line !== '') {
        $queue[] = ltrim($line, '\\');
    }
}

$files = [];
$seen = [];
while ($queue !== []) {
    $name = array_shift($queue);
    if (isset($seen[strtolower($name)])) {
        continue;
    }
    $seen[strtolower($name)] = true;
    try {
        if (!class_exists($name) && !interface_exists($name) && !trait_exists($name)) {
            fwrite(STDERR, "not found: $name\n");
            continue;
        }
    } catch (Throwable $e) {
        // A class whose parent is not installed (a test case extending
        // PHPUnit's) is copied on its own.
        $file = $loader->findFile($name);
        if ($file === false) {
            fwrite(STDERR, "cannot load $name: {$e->getMessage()}\n");
            continue;
        }
        $files[(string) realpath($file)] = true;
        fwrite(STDERR, "copied without its ancestors: $name\n");
        continue;
    }
    $class = new ReflectionClass($name);
    $file = $class->getFileName();
    if ($file === false || !str_starts_with($file, $vendorDir . '/')) {
        continue;
    }
    $files[$file] = true;
    if ($parent = $class->getParentClass()) {
        $queue[] = $parent->getName();
    }
    foreach ($class->getInterfaceNames() as $interface) {
        $queue[] = $interface;
    }
    foreach ($class->getTraitNames() as $trait) {
        $queue[] = $trait;
    }
}

ksort($files);
$packages = [];
foreach (array_keys($files) as $file) {
    // vendor/<org>/<package>/... becomes <org>/<package>/...
    $relative = substr($file, strlen($vendorDir) + 1);
    $target = $outDir . '/' . $relative;
    if (!is_dir(dirname($target))) {
        mkdir(dirname($target), 0777, true);
    }
    file_put_contents($target, strip_bodies((string) file_get_contents($file)));
    echo $relative, "\n";
    $packages[implode('/', array_slice(explode('/', $relative), 0, 2))] = true;
}

// The copies keep each package's license beside them.
foreach (array_keys($packages) as $package) {
    foreach (['LICENSE', 'LICENSE.md', 'LICENSE.txt'] as $name) {
        if (is_file("$vendorDir/$package/$name")) {
            copy("$vendorDir/$package/$name", "$outDir/$package/$name");
            break;
        }
    }
}

/** The source with every function and method body replaced by `{}`. */
function strip_bodies(string $source): string
{
    $tokens = token_get_all($source);
    $out = '';
    $count = count($tokens);
    for ($i = 0; $i < $count; $i++) {
        $token = $tokens[$i];
        $out .= is_array($token) ? $token[1] : $token;
        if (!is_array($token) || $token[0] !== T_FUNCTION) {
            continue;
        }
        // Copy the signature up to the body's `{`, or to the `;` of an
        // abstract or interface method.
        $parens = 0;
        for ($i++; $i < $count; $i++) {
            $token = $tokens[$i];
            $text = is_array($token) ? $token[1] : $token;
            if ($text === '(') {
                $parens++;
            } elseif ($text === ')') {
                $parens--;
            } elseif ($parens === 0 && ($text === ';' || $text === '{')) {
                break;
            }
            $out .= $text;
        }
        if ($i >= $count || $tokens[$i] === ';') {
            $out .= ';';
            continue;
        }
        $depth = 0;
        for (; $i < $count; $i++) {
            $text = is_array($tokens[$i]) ? $tokens[$i][1] : $tokens[$i];
            if ($text === '{' || $text === '${' || (is_array($tokens[$i]) && $tokens[$i][0] === T_CURLY_OPEN) || (is_array($tokens[$i]) && $tokens[$i][0] === T_DOLLAR_OPEN_CURLY_BRACES)) {
                $depth++;
            } elseif ($text === '}') {
                $depth--;
                if ($depth === 0) {
                    break;
                }
            }
        }
        $out .= '{}';
    }

    return $out;
}
