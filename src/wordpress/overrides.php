<?php

/**
 * Return types for functions of php-stubs/wordpress-stubs, where the type
 * an argument decides is narrower than the one the stubs declare.
 *
 * Only the return type is read from here. It replaces the stubs' own on
 * their declaration of the same function, which keeps its parameters,
 * docblock and location. Each type follows what WordPress core returns,
 * leaving out what a filter may substitute, as the stubs do.
 */

/**
 * @phpstan-return ($meta_key is '' ? ($single is true ? array<string, list<string>>|''|false : array<string, list<string>>|false) : ($single is true ? mixed : list<mixed>|false))
 */
function get_metadata($meta_type, $object_id, $meta_key = '', $single = false) {}

/**
 * @phpstan-return ($key is '' ? ($single is true ? array<string, list<string>>|''|false : array<string, list<string>>|false) : ($single is true ? mixed : list<mixed>|false))
 */
function get_post_meta($post_id, $key = '', $single = false) {}

/**
 * @phpstan-return ($key is '' ? ($single is true ? array<string, list<string>>|''|false : array<string, list<string>>|false) : ($single is true ? mixed : list<mixed>|false))
 */
function get_user_meta($user_id, $key = '', $single = false) {}

/**
 * @phpstan-return ($key is '' ? ($single is true ? array<string, list<string>>|''|false : array<string, list<string>>|false) : ($single is true ? mixed : list<mixed>|false))
 */
function get_term_meta($term_id, $key = '', $single = false) {}

/**
 * @phpstan-return ($key is '' ? ($single is true ? array<string, list<string>>|''|false : array<string, list<string>>|false) : ($single is true ? mixed : list<mixed>|false))
 */
function get_comment_meta($comment_id, $key = '', $single = false) {}

/**
 * @phpstan-return ($key is '' ? ($single is true ? array<string, list<string>>|''|false : array<string, list<string>>|false) : ($single is true ? mixed : list<mixed>|false))
 */
function get_site_meta($site_id, $key = '', $single = false) {}

/**
 * @phpstan-return ($format is 'G'|'U' ? int|false : string|false)
 */
function get_post_time($format = 'U', $gmt = false, $post = null, $translate = false) {}

/**
 * @phpstan-return ($format is 'G'|'U' ? int|false : string|false)
 */
function get_post_modified_time($format = 'U', $gmt = false, $post = null, $translate = false) {}

/**
 * An empty format reads the format from the `time_format` option.
 *
 * @phpstan-return ($format is 'G'|'U' ? int|false : ($format is '' ? string|int|false : string|false))
 */
function get_the_time($format = '', $post = null) {}

/**
 * @phpstan-return ($format is 'G'|'U' ? int|false : ($format is '' ? string|int|false : string|false))
 */
function get_the_modified_time($format = '', $post = null) {}

/**
 * An empty format reads the format from the `date_format` option.
 *
 * @phpstan-return ($format is 'G'|'U' ? int|false : ($format is '' ? string|int|false : string|false))
 */
function get_the_date($format = '', $post = null) {}

/**
 * @phpstan-return ($format is 'G'|'U' ? int|false : ($format is '' ? string|int|false : string|false))
 */
function get_the_modified_date($format = '', $post = null) {}

/**
 * Returns nothing when there are no comments to list, whatever `echo` says.
 *
 * @phpstan-return ($args is array{echo: false, ...} ? string|void : void)
 */
function wp_list_comments($args = array(), $comments = null) {}

/**
 * A URL `parse_url()` cannot parse gives `false` whichever component is
 * asked for, and a component no `PHP_URL_*` constant names gives `null`.
 *
 * @phpstan-return (
 *     $component is -1
 *         ? false|array{
 *               scheme?: string,
 *               host?: string,
 *               port?: int<0, 65535>,
 *               user?: string,
 *               pass?: string,
 *               path?: string,
 *               query?: string,
 *               fragment?: string,
 *           }
 *         : (
 *             $component is int<0, 7>
 *                 ? ($component is 2 ? int<0, 65535>|false|null : string|false|null)
 *                 : false|null
 *         )
 * )
 */
function wp_parse_url($url, $component = -1) {}

/**
 * Slashing adds backslashes and nothing else, so a lowercase string stays
 * one.
 *
 * @phpstan-return (T is lowercase-string ? lowercase-string : (T is string ? string : (T is array ? array<key-of<T>, (value-of<T> is string ? string : value-of<T>)> : T)))
 */
function wp_slash($value) {}

/**
 * Escapes each value of an array, nested arrays included, and gives `''`
 * for anything that is not a scalar.
 *
 * @phpstan-return ($data is array ? array : ($data is string|int|float|bool ? string : ''))
 */
function esc_sql($data) {}

/**
 * @phpstan-return non-falsy-string
 */
function wp_hash_password($password) {}

/**
 * @phpstan-return ($value is lowercase-string ? lowercase-string&non-falsy-string : non-falsy-string)
 */
function trailingslashit($value) {}

/**
 * @phpstan-return ($value is lowercase-string ? lowercase-string : string)
 */
function untrailingslashit($value) {}

/**
 * A leading digit gains a backslash, so no non-empty string comes back `'0'`.
 *
 * @phpstan-return ($value is '' ? '' : ($value is lowercase-string&non-empty-string ? lowercase-string&non-falsy-string : ($value is non-empty-string ? non-falsy-string : string)))
 */
function backslashit($value) {}

/**
 * Collapses runs of whitespace and trims the ends, so a non-empty string
 * may come back empty.
 *
 * @phpstan-return ($str is literal-string ? ($str is lowercase-string ? lowercase-string&literal-string : literal-string) : ($str is lowercase-string ? lowercase-string : string))
 */
function normalize_whitespace($str) {}
