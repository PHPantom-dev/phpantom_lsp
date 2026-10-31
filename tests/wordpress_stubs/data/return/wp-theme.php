<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function PHPStan\Testing\assertType;

$theme = Faker::wpTheme();

// WP_Theme::__get()
assertType('string', $theme->name);
assertType('string', $theme->title);
assertType('string', $theme->version);
assertType('string', $theme->parent_theme);
assertType('string', $theme->template_dir);
assertType('string', $theme->stylesheet_dir);
assertType('string', $theme->template);
assertType('string', $theme->stylesheet);
assertType('string', $theme->screenshot);
assertType('string', $theme->description);
assertType('string', $theme->author);
assertType('list<string>', $theme->tags);
assertType('string', $theme->theme_root);
assertType('string', $theme->theme_root_uri);
assertType('*ERROR*', $theme->unknown_property);

// WP_Theme::offsetExists()
assertType('false', $theme->offsetExists('NoThemeKey'));
assertType('true', $theme->offsetExists('Name'));
assertType('true', $theme->offsetExists('Version'));
assertType('true', $theme->offsetExists('Status'));
assertType('true', $theme->offsetExists('Title'));
assertType('true', $theme->offsetExists('Author'));
assertType('true', $theme->offsetExists('Author Name'));
assertType('true', $theme->offsetExists('Author URI'));
assertType('true', $theme->offsetExists('Description'));
assertType('true', $theme->offsetExists('Template'));
assertType('true', $theme->offsetExists('Stylesheet'));
assertType('true', $theme->offsetExists('Template Files'));
assertType('true', $theme->offsetExists('Stylesheet Files'));
assertType('true', $theme->offsetExists('Template Dir'));
assertType('true', $theme->offsetExists('Stylesheet Dir'));
assertType('true', $theme->offsetExists('Screenshot'));
assertType('true', $theme->offsetExists('Tags'));
assertType('true', $theme->offsetExists('Theme Root'));
assertType('true', $theme->offsetExists('Theme Root URI'));
assertType('true', $theme->offsetExists('Parent Theme'));
assertType('bool', $theme->offsetExists(Faker::string()));

// WP_Theme::offsetGet()
assertType('null', $theme->offsetGet('NoThemeKey'));
assertType('mixed', $theme->offsetGet('Name'));
assertType('mixed', $theme->offsetGet('Version'));
assertType('mixed', $theme->offsetGet('Status'));
assertType('mixed', $theme->offsetGet('Title'));
assertType('mixed', $theme->offsetGet('Author'));
assertType('mixed', $theme->offsetGet('Author Name'));
assertType('mixed', $theme->offsetGet('Author URI'));
assertType('mixed', $theme->offsetGet('Description'));
assertType('mixed', $theme->offsetGet('Template'));
assertType('mixed', $theme->offsetGet('Stylesheet'));
assertType('mixed', $theme->offsetGet('Template Files'));
assertType('mixed', $theme->offsetGet('Stylesheet Files'));
assertType('mixed', $theme->offsetGet('Template Dir'));
assertType('mixed', $theme->offsetGet('Stylesheet Dir'));
assertType('mixed', $theme->offsetGet('Screenshot'));
assertType('mixed', $theme->offsetGet('Tags'));
assertType('mixed', $theme->offsetGet('Theme Root'));
assertType('mixed', $theme->offsetGet('Theme Root URI'));
assertType('mixed', $theme->offsetGet('Parent Theme'));
assertType('mixed', $theme->offsetGet(Faker::string()));
