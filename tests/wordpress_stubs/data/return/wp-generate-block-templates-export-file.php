<?php

declare(strict_types=1);

use function PHPStan\Testing\assertType;

assertType('non-falsy-string|WP_Error', wp_generate_block_templates_export_file());
