<?php

namespace Bug4985;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo()
	{
		$m = new CollectionMockWithApp();
		$m->setApp(new class() {
			/** @var string */
			public $name = 'app';
			/** @var int */
			public $max_name_length = 40;
		});

		/** @var FieldMockCustom $surnameField */
		$surnameField = $m->addField('surname', [FieldMockCustom::class]);

		assertType(FieldMockCustom::class, $surnameField);
	}

	public function doBar()
	{
		$m = new CollectionMockWithApp();
		$m->setApp();

		/** @var FieldMockCustom $surnameField */
		$surnameField = $m->addField('surname', [FieldMockCustom::class]);

		assertType(FieldMockCustom::class, $surnameField);
	}

}
