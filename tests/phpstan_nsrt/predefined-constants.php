<?php

use function PHPStan\Testing\assertType;

// core, https://www.php.net/manual/en/reserved.constants.php
assertType('non-falsy-string', PHP_VERSION);
// PHPantom reads the major and minor version, and the range of
// PHP_VERSION_ID, from the configured PHP version (8.5 here).
assertType('8', PHP_MAJOR_VERSION);
assertType('5', PHP_MINOR_VERSION);
assertType('int<0, max>', PHP_RELEASE_VERSION);
assertType('int<80500, 80599>', PHP_VERSION_ID);
assertType('string', PHP_EXTRA_VERSION);
assertType('0|1', PHP_ZTS);
assertType('0|1', PHP_DEBUG);
assertType('int<1, max>', PHP_MAXPATHLEN);
assertType('non-falsy-string', PHP_OS);
assertType('\'apache\'|\'apache2handler\'|\'cgi\'|\'cli\'|\'cli-server\'|\'embed\'|\'fpm-fcgi\'|\'litespeed\'|\'phpdbg\'|non-falsy-string', PHP_SAPI);
assertType('"\n"|"\r\n"', PHP_EOL);
// PHPantom assumes a 64-bit build.
assertType('8', PHP_INT_SIZE);
assertType('string', DEFAULT_INCLUDE_PATH);
assertType('string', PEAR_INSTALL_DIR);
assertType('string', PEAR_EXTENSION_DIR);
assertType('non-falsy-string', PHP_EXTENSION_DIR);
assertType('non-falsy-string', PHP_PREFIX);
assertType('non-falsy-string', PHP_BINDIR);
assertType('non-falsy-string', PHP_BINARY);
assertType('non-falsy-string', PHP_MANDIR);
assertType('non-falsy-string', PHP_LIBDIR);
assertType('non-falsy-string', PHP_DATADIR);
assertType('non-falsy-string', PHP_SYSCONFDIR);
assertType('non-falsy-string', PHP_LOCALSTATEDIR);
assertType('non-falsy-string', PHP_CONFIG_FILE_PATH);
assertType('string', PHP_CONFIG_FILE_SCAN_DIR);
assertType('\'dll\'|\'so\'', PHP_SHLIB_SUFFIX);
assertType('1', E_ERROR);
assertType('2', E_WARNING);
assertType('4', E_PARSE);
assertType('8', E_NOTICE);
assertType('16', E_CORE_ERROR);
assertType('32', E_CORE_WARNING);
assertType('64', E_COMPILE_ERROR);
assertType('128', E_COMPILE_WARNING);
assertType('256', E_USER_ERROR);
assertType('512', E_USER_WARNING);
assertType('1024', E_USER_NOTICE);
assertType('4096', E_RECOVERABLE_ERROR);
assertType('8192', E_DEPRECATED);
assertType('16384', E_USER_DEPRECATED);
// E_ALL follows from the configured PHP version: E_STRICT left it in 8.4.
assertType('30719', E_ALL);
assertType('2048', E_STRICT);
assertType('int<1, max>', __COMPILER_HALT_OFFSET__);
assertType('true', true);
assertType('false', false);
assertType('null', null);

// core other, https://www.php.net/manual/en/info.constants.php
assertType('int<4, max>', PHP_WINDOWS_VERSION_MAJOR);
assertType('int<0, max>', PHP_WINDOWS_VERSION_MINOR);
assertType('int<1, max>', PHP_WINDOWS_VERSION_BUILD);

// dir, https://www.php.net/manual/en/dir.constants.php
assertType('\'/\'|\'\\\\\'', DIRECTORY_SEPARATOR);
assertType('\':\'|\';\'', PATH_SEPARATOR);

// iconv, https://www.php.net/manual/en/iconv.constants.php
assertType('non-falsy-string', ICONV_IMPL);

// libxml, https://www.php.net/manual/en/libxml.constants.php
assertType('int<1, max>', LIBXML_VERSION);
assertType('non-falsy-string', LIBXML_DOTTED_VERSION);

// openssl, https://www.php.net/manual/en/openssl.constants.php
assertType('int<1, max>', OPENSSL_VERSION_NUMBER);

// pcre, https://www.php.net/manual/en/pcre.constants.php
assertType('non-falsy-string', PCRE_VERSION);

// other
assertType('bool', ZEND_DEBUG_BUILD);
assertType('bool', ZEND_THREAD_SAFE);
