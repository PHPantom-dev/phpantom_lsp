<?php

use function PHPStan\Testing\assertType;

$heredoc = <<<EOT
foo
EOT;

$nowdoc = <<<'EOD'
bar
EOD;

assertType('\'foo\'', $heredoc);
assertType('\'bar\'', $nowdoc);
