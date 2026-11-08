# Diagnostics Reference

Every diagnostic PHPantom reports carries an identifier. It is shown
with the message in your editor and under the message in `analyze`
output, and it is what these take:

- `[[diagnostics.ignore]] identifier = "…"` in `.phpantom.toml` (see
  [Configuration](configuration.md#diagnosticsignore)),
- an `// @phpantom-ignore …` comment (see
  [Inline suppression](configuration.md#inline-suppression)),
- `analyze --code` and `--exclude-code` (see [CLI](cli.md#analyze)).

The severity is the default one. A few checks are off until you turn
them on in [`[diagnostics]`](configuration.md#diagnostics); their rows
name the key. Framework checks run only in a project PHPantom detects as
using that framework, and need no setting.

An identifier starts with the kind of problem (`unknown_`, `invalid_`,
`missing_`, `unused_`, `type_mismatch_`, and so on). One that belongs to
a framework or template language names it next, as in
`invalid_laravel_route` or `unclosed_blade_directive`. A name a framework
expects to be declared somewhere, but is not, is always `invalid_`.

## PHP code

| Identifier | Severity | Reports |
| --- | --- | --- |
| `syntax_error` | Error | Code that does not parse. |
| `unknown_class` | Warning | A class, interface, trait, or enum that cannot be found, including one named by the type of a value whose member is used. |
| `unknown_function` | Error | A function call that cannot be resolved. |
| `unknown_member` | Warning | A method, property, or constant the resolved class does not have. `magic-properties` widens it to classes with `__get`. |
| `partial_union_member` | Warning | A member only some classes of a union declare. Off unless `partial-union-members` is on. |
| `unresolved_member_access` | Hint | `->`, `?->`, or `::` on a value whose type is `mixed` or could not be worked out. Off unless `unresolved-member-access` is on. |
| `scalar_member_access` | Error | `->` or `::` on a scalar, `null`, or an array. |
| `inaccessible_member` | Error | A `private` or `protected` member reached from where it is not visible. |
| `invalid_member_name` | Error | A dynamic member name (`$object->{$name}`) whose value has the wrong type. |
| `unknown_variable` | Error | A variable read before anything defines it. |
| `unknown_offset` | Warning | A destructuring offset the array's type rules out. |
| `argument_count_mismatch` | Error | Too few arguments for the signature, or too many with `extra-arguments` on. |
| `unknown_named_argument` | Error | A named argument that matches no parameter. |
| `type_mismatch_argument` | Error | An argument whose type the parameter does not accept. |
| `type_mismatch_return` | Error | A returned value whose type the return type does not accept. |
| `type_mismatch_property` | Error | A value assigned to a property whose type does not accept it. |
| `invalid_string_conversion` | Error or Warning | An object without `__toString()` used as a string. Warning when it only might be one. |
| `invalid_readonly_write` | Error | A write to a `readonly` property from outside its class, or a second write. |
| `uninitialized_property` | Warning | A typed property with no default that nothing writes. |
| `uninitialized_property_read` | Warning | A typed property read on a new object before anything writes it. |
| `missing_return` | Error | A function that can end without returning the value it promises. |
| `used_void_result` | Warning | The result of a `void` call used as a value. |
| `discarded_return_value` | Warning | A `#[\NoDiscard]` call whose result is thrown away. |
| `unhandled_match` | Error | A `match` without `default` that leaves some values of its subject unhandled. |
| `unreachable_match_arm` | Warning | A `match` arm that can never be identical to the subject. |
| `unknown_enum_value` | Error or Warning | A literal passed to an enum's `from()` (Error) or `tryFrom()` (Warning) that no case backs. |
| `invalid_enum_case` | Error | A backed case without a value, a pure case with one, or two cases with the same value. |
| `invalid_enum_backing_type` | Error | An enum backed by something other than `int` or `string`. |
| `invalid_class_kind` | Error or Warning | A class-like used where its kind is not allowed, such as `new` on an interface or `implements` a trait. |
| `missing_implementation` | Error | A concrete class that does not implement every interface or abstract method. |
| `incompatible_override` | Error | An override that does not return `static` where the parent method does. |
| `incompatible_trait_property` | Error | The same property from two traits, or a trait and the class, declared incompatibly. |
| `invalid_override_attribute` | Error | `#[\Override]` on a method or property that overrides nothing. |
| `unhandled_exception` | Hint | An exception thrown or passed on that is neither caught nor declared in `@throws`. Off unless `unhandled-exceptions` is on. |
| `dead_catch` | Warning | A `catch` for an exception nothing in its `try` can throw. Shown dimmed. |
| `deprecated_usage` | Hint | A reference to a symbol marked `@deprecated`. Shown struck through. |

## Unused and unreachable code

These are shown dimmed rather than underlined, except `unwritten_property`.

| Identifier | Severity | Reports |
| --- | --- | --- |
| `unused_import` | Hint | A `use` import nothing in the file refers to. |
| `unused_variable` | Hint | A variable that is assigned or bound but never read. |
| `unused_parameter` | Hint | A trailing parameter the body never reads. Off unless `unused-parameters` is on. |
| `unused_method` | Hint | A private method nothing calls. |
| `unused_property` | Hint | A private property that is never read. |
| `unused_constant` | Hint | A private constant nothing reads. |
| `unwritten_property` | Information | A private property that is read but never written. |
| `unreachable_code` | Hint | Statements after a `return`, `throw`, `exit`, `break`, or similar. |

## Docblocks and project layout

| Identifier | Severity | Reports |
| --- | --- | --- |
| `docblock_native_mismatch` | Warning | A docblock type that contradicts the native type. |
| `docblock_missing_null` | Hint | A docblock type that leaves out the `null` its nullable native type allows. |
| `class_case_mismatch` | Warning | A class name spelled with different case than its declaration. |
| `class_name_mismatch` | Warning | A class whose name does not match its PSR-4 file name. |
| `namespace_mismatch` | Warning | A class whose namespace does not match its PSR-4 path. |

## PHPUnit and Doctrine

| Identifier | Severity | Reports |
| --- | --- | --- |
| `invalid_phpunit_data_provider` | Error or Warning | A data provider that is not `public`, or not `static` where the PHPUnit version requires it. |
| `missing_phpunit_parent_call` | Warning | A `setUp()` or `tearDown()` override that skips a parent that does work. |
| `invalid_doctrine_field` | Warning | A field in a repository call that the entity does not map. |
| `invalid_dql_syntax` | Error | A DQL string that does not parse. |
| `invalid_dql_alias` | Error | A DQL alias nothing in the query declares. |

## Laravel and Blade

| Identifier | Severity | Reports |
| --- | --- | --- |
| `invalid_laravel_route` | Warning | A route name no route declares. |
| `invalid_laravel_view` | Warning | A view name with no template. |
| `invalid_laravel_config` | Warning | A `config()` key no config file declares. |
| `invalid_laravel_translation` | Warning | A translation key no language file declares. |
| `invalid_laravel_middleware` | Warning | A middleware alias or group that is not registered. |
| `invalid_laravel_ability` | Warning | A gate or policy ability that is not defined, or not for the given model. |
| `invalid_laravel_command` | Warning | An Artisan command name that is not registered. |
| `invalid_laravel_command_parameter` | Warning | An `argument()` or `option()` name the command's signature does not declare. |
| `invalid_laravel_morph_alias` | Warning | A morph type alias not in the enforced morph map. |
| `invalid_laravel_asset` | Warning | An `asset()` path that does not exist. |
| `invalid_laravel_mix_asset` | Warning | A `mix()` asset not in the Mix manifest. |
| `invalid_laravel_vite_asset` | Warning | A Vite asset that does not exist. |
| `invalid_inertia_page` | Warning | An Inertia page with no page file. |
| `invalid_laravel_auth_guard` | Warning | An auth guard not in the config. |
| `invalid_laravel_broadcast_connection` | Warning | A broadcast connection not in the config. |
| `invalid_laravel_cache_store` | Warning | A cache store not in the config. |
| `invalid_laravel_database_connection` | Warning | A database connection not in the config. |
| `invalid_laravel_log_channel` | Warning | A log channel not in the config. |
| `invalid_laravel_mailer` | Warning | A mailer not in the config. |
| `invalid_laravel_queue_connection` | Warning | A queue connection not in the config. |
| `invalid_laravel_storage_disk` | Warning | A storage disk not in the config. |
| `invalid_laravel_validation_rule` | Warning | A validation rule name Laravel does not have. |
| `invalid_laravel_validation_message` | Warning | A form request `messages()` or `attributes()` key naming an unknown field or rule. |
| `missing_blade_variable` | Error | A variable a Blade template declares that the call site does not pass. |
| `unused_blade_variable` | Warning | A variable passed to a view that the template does not use. |
| `type_mismatch_blade_variable` | Error | A variable passed to a view whose type the template's declaration does not accept. |
| `invalid_blade_component` | Warning | An `<x-…>` tag with no component class or template. |
| `invalid_livewire_component` | Warning | A `<livewire:…>` tag with no component. |
| `unclosed_blade_component_tag` | Error | An `<x-…>` tag that is never closed. |
| `unexpected_blade_component_tag` | Error | A closing component tag with nothing open. |
| `mismatched_blade_component_tag` | Error | A closing component tag that closes a different tag. |
| `unclosed_blade_directive` | Error | A block directive that is never closed. |
| `unexpected_blade_directive` | Error | A closing directive with nothing open. |
| `mismatched_blade_directive` | Error | A closing directive that closes a different block. |
| `unrendered_blade_section` | Warning | A `@section` no layout yields. |
| `unrendered_blade_stack` | Warning | A `@push` to a stack no layout renders. |
| `duplicate_blade_signature` | Error | A second `@bladestan-signature` block in one template. |
| `incompatible_blade_signature` | Error | A `@bladestan-signature` the parent layout does not accept. |

## Symfony and Twig

| Identifier | Severity | Reports |
| --- | --- | --- |
| `invalid_symfony_route` | Warning | A route name no route declares. |
| `invalid_symfony_service` | Warning | A service ID that is not declared. |
| `invalid_symfony_parameter` | Warning | A container parameter that is not declared. |
| `invalid_symfony_translation` | Warning | A translation key missing from its domain. |
| `invalid_symfony_messenger_bus` | Warning | A Messenger bus that is not configured. |
| `invalid_twig_template` | Warning | A Twig template name with no template. |
| `invalid_twig_filter` | Warning | A Twig filter that is not declared. |
| `invalid_twig_function` | Warning | A Twig function that is not declared. |
| `invalid_symfony_form_option` | Error | An option the form type does not accept. |
| `invalid_symfony_constraint_option` | Error | An option the validation constraint does not take. |

## Yii

| Identifier | Severity | Reports |
| --- | --- | --- |
| `invalid_yii_route` | Warning | A route name no route declares. |
| `invalid_yii_route_argument` | Warning | A route argument the route's path does not have. |
| `invalid_yii_view` | Warning | A view or layout with no view file. |
| `invalid_yii_view_path` | Warning | A view path directory that does not exist. |
| `invalid_yii_view_parameter` | Warning | A parameter passed to a view whose template neither declares it in its `@var` block nor reads it. |
| `missing_yii_view_parameter` | Warning | A parameter a template declares in its `@var` block that the render does not pass and no view is given otherwise. |
| `type_mismatch_yii_view_parameter` | Error | A parameter passed to a view whose type the template's `@var` declaration does not accept. |
| `invalid_yii_asset` | Warning | An asset bundle's `$css` or `$js` file that is not in the bundle's directory. |
| `invalid_yii_definition_argument` | Warning | A DI definition argument that names no parameter of the method. |
| `invalid_yii_definition_key` | Error | A DI definition key that is not `class`, `__construct()`, a method call (`'name()'`), or a property (`'$name'`). |
| `invalid_yii_event_listener` | Error | An event listener with a required parameter that has no class type, which neither the event nor the container can fill. |
| `invalid_yii_param` | Warning | A `$params` key no params file declares. |
| `invalid_yii_translation` | Warning | A message ID, spelled like a key (`menu.blog`), that no message file of its category declares. |
| `invalid_yii_config_file` | Warning | A file a `config-plugin` in `composer.json` lists that does not exist. |
| `invalid_yii_attribute` | Warning | An Active Record attribute the record declares no property for. On a record that declares no properties, whose attributes are its table's columns, a column no migration creates. |
| `invalid_yii_relation` | Warning | An Active Record relation the record does not declare. |
| `invalid_yii_table` | Warning | A table a `yiisoft/db` query reads from or joins that no migration creates. |
| `invalid_yii_column` | Warning | A column a `yiisoft/db` query names that none of the tables it may belong to has, as the migrations create them. |

## Drupal

| Identifier | Severity | Reports |
| --- | --- | --- |
| `invalid_drupal_route` | Warning | A route name nothing declares. |
| `invalid_drupal_route_parameter` | Warning | A controller parameter named after an entity type but typed with an unrelated class, so it receives the raw ID. |
| `invalid_drupal_library` | Warning | A library name nothing declares. |
| `invalid_drupal_service` | Warning | A service ID no services file declares and no PHP code may define. |
| `invalid_drupal_plugin` | Warning | A plugin ID no plugin of its type declares and no PHP code may add. |
| `invalid_drupal_render_element` | Warning | A render element `#type` no element class declares. |

## External tools

Diagnostics from [PHPStan, PHPCS, PHPMD, and Mago](configuration.md#phpstan)
keep the tool's own identifier: PHPStan's error identifier (such as
`argument.type`), the PHPCS sniff (such as
`Generic.Files.LineLength.TooLong`), the PHPMD rule (such as
`UnusedLocalVariable`), and the Mago issue code (such as
`invalid-return-statement`). Both `[[diagnostics.ignore]]` and
`@phpantom-ignore` match them the same way.

## Renamed identifiers

These identifiers were renamed. The old spelling still works in
`[[diagnostics.ignore]]`, `@phpantom-ignore`, and `analyze --code` and
`--exclude-code`; the config and the command line warn when they meet
one.

| Old | New |
| --- | --- |
| `invalid_member_access` | `inaccessible_member` |
| `invalid_laravel_trans` | `invalid_laravel_translation` |
| `invalid_command_parameter` | `invalid_laravel_command_parameter` |
| `missing_view_variable` | `missing_blade_variable` |
| `unused_view_variable` | `unused_blade_variable` |
| `type_mismatch_view_variable` | `type_mismatch_blade_variable` |
| `blade_signature_widens_layout` | `incompatible_blade_signature` |
