<?php

/**
 * @file
 * Hooks the acme module invokes.
 */

/**
 * Alter a greeting before the greeting page shows it.
 *
 * @param string $greeting
 *   The greeting, to alter in place.
 * @param string $name
 *   The name being greeted.
 */
function hook_acme_greeting_alter(string &$greeting, string $name): void
{
    $greeting .= ' Nice to meet you, ' . $name . '.';
}
