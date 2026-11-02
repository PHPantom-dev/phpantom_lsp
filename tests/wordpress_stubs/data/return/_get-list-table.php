<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function _get_list_table;
use function PHPStan\Testing\assertType;

// A non-class-string
assertType('false', _get_list_table('string'));

// A class-string that is not class-string<WP_List_Table>
assertType('false', _get_list_table('WP_Post'));

// WP_List_Table itself
assertType('false', _get_list_table('WP_List_Table'));

// Core WP_List_Table classes
assertType('WP_Posts_List_Table', _get_list_table('WP_Posts_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_Media_List_Table', _get_list_table('WP_Media_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_Terms_List_Table', _get_list_table('WP_Terms_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_Users_List_Table', _get_list_table('WP_Users_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_Comments_List_Table', _get_list_table('WP_Comments_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_Post_Comments_List_Table', _get_list_table('WP_Post_Comments_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_Links_List_Table', _get_list_table('WP_Links_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_Plugin_Install_List_Table', _get_list_table('WP_Plugin_Install_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_Themes_List_Table', _get_list_table('WP_Themes_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_Plugins_List_Table', _get_list_table('WP_Plugins_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_Application_Passwords_List_Table', _get_list_table('WP_Application_Passwords_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_MS_Sites_List_Table', _get_list_table('WP_MS_Sites_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_MS_Users_List_Table', _get_list_table('WP_MS_Users_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_MS_Themes_List_Table', _get_list_table('WP_MS_Themes_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_Privacy_Data_Export_Requests_List_Table', _get_list_table('WP_Privacy_Data_Export_Requests_List_Table')); // SKIP: `new<T>` is not supported
assertType('WP_Privacy_Data_Removal_Requests_List_Table', _get_list_table('WP_Privacy_Data_Removal_Requests_List_Table')); // SKIP: `new<T>` is not supported

// Union of core WP_List_Table classes
assertType('WP_Media_List_Table|WP_Posts_List_Table', _get_list_table(Faker::union('WP_Posts_List_Table', 'WP_Media_List_Table'))); // SKIP: `new<T>` is not supported

// Union of core WP_List_Table class and class that is not a subclass of WP_List_Table
assertType('WP_Posts_List_Table|false', _get_list_table(Faker::union('WP_Posts_List_Table', 'WP_Post'))); // SKIP: `new<T>` is not supported

// WP_Post_Comments_List_Table is generalized WP_Comments_List_Table
assertType('WP_Comments_List_Table', _get_list_table(Faker::union('WP_Comments_List_Table', 'WP_Post_Comments_List_Table'))); // SKIP: `new<T>` is not supported

// WP_Theme_Install_List_Table is generalized to WP_Themes_List_Table
assertType('WP_Themes_List_Table', _get_list_table(Faker::union('WP_Themes_List_Table', 'WP_Theme_Install_List_Table'))); // SKIP: `new<T>` is not supported

// Unknown string
assertType('WP_Application_Passwords_List_Table|WP_Comments_List_Table|WP_Links_List_Table|WP_Media_List_Table|WP_MS_Sites_List_Table|WP_MS_Themes_List_Table|WP_MS_Users_List_Table|WP_Plugin_Install_List_Table|WP_Plugins_List_Table|WP_Posts_List_Table|WP_Privacy_Data_Export_Requests_List_Table|WP_Privacy_Data_Removal_Requests_List_Table|WP_Terms_List_Table|WP_Themes_List_Table|WP_Users_List_Table|false', _get_list_table(Faker::string())); // SKIP: `new<T>` is not supported
