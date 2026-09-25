<?php declare(strict_types = 1);

namespace Bug12834;

use function PHPStan\Testing\assertType;

/**
 * @method int test()
 */
class HelloWorld extends \SoapClient
{

}

$x = new HelloWorld("file.wsdl");
assertType('int', $x->test());
