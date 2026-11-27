<?php

namespace Bug9575;

use SimpleXMLElement;
use function PHPStan\Testing\assertType;

$string = <<<XML
<a>
 <foo name="one" game="lonely">1</foo>
</a>
XML;

$xml = new SimpleXMLElement($string);
foreach($xml->foo[0]->attributes() as $a => $b) {
	echo $a,'="',$b,"\"\n";
}

assertType('(SimpleXMLElement|null)', $xml->foo);
assertType('(SimpleXMLElement|null)', $xml->foo[0]);
assertType('(SimpleXMLElement|null)', $xml->foobar);
assertType('(SimpleXMLElement|null)', $xml->foo->attributes());
