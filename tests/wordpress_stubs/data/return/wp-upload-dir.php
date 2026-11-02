<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function _wp_upload_dir;
use function wp_get_upload_dir;
use function wp_upload_dir;
use function PHPStan\Testing\assertType;

assertType('array{path: non-falsy-string, url: non-falsy-string, subdir: string, basedir: non-falsy-string, baseurl: non-falsy-string, error: false}', _wp_upload_dir());
assertType('array{path: string, url: string, subdir: string, basedir: string, baseurl: string, error: string|false}', wp_get_upload_dir());
assertType('array{path: string, url: string, subdir: string, basedir: string, baseurl: string, error: string|false}', wp_upload_dir());
