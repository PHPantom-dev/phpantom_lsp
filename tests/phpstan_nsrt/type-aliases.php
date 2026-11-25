<?php

namespace TypeAliasesDataset\SubScope {
	class Foo
	{
	}

	/**
	 * @phpstan-type ScopedAlias Foo
	 */
	class Bar
	{
	}
}

namespace TypeAliasesDataset {

	use function PHPStan\Testing\assertType;

	/**
	 * @phpstan-type ExportedTypeAlias \Countable&\Traversable
	 */
	class Bar
	{
	}

	/**
	 * @phpstan-import-type ExportedTypeAlias from Bar as ReexportedTypeAlias
	 * @phpstan-import-type CircularTypeAliasImport2 from Qux
	 * @phpstan-type CircularTypeAliasImport1 CircularTypeAliasImport2
	 * @property CircularTypeAliasImport1 $baz
	 * @property CircularTypeAliasImport2 $qux
	 */
	class Baz
	{

		public function circularAlias()
		{
			assertType('*ERROR*', $this->baz);
			assertType('*ERROR*', $this->qux);
		}

	}

	/**
	 * @phpstan-import-type CircularTypeAliasImport1 from Baz
	 * @phpstan-type CircularTypeAliasImport2 CircularTypeAliasImport1
	 */
	class Qux
	{
	}

	/**
	 * @phpstan-type LocalTypeAlias callable(string $value): (string|false)
	 * @phpstan-type NestedLocalTypeAlias LocalTypeAlias[]
	 * @phpstan-import-type ExportedTypeAlias from Bar as ImportedTypeAlias
	 * @phpstan-import-type ReexportedTypeAlias from Baz
	 * @phpstan-type NestedImportedTypeAlias iterable<ImportedTypeAlias>
	 * @phpstan-import-type ScopedAlias from SubScope\Bar
	 * @phpstan-import-type ImportedAliasFromNonClass from int
	 * @phpstan-import-type ImportedAliasFromUnknownClass from UnknownClass
	 * @phpstan-import-type ImportedUknownAlias from SubScope\Bar
	 * @phpstan-type Baz never
	 * @phpstan-type GlobalTypeAlias never
	 * @phpstan-type RecursiveTypeAlias RecursiveTypeAlias[]
	 * @phpstan-type CircularTypeAlias1 CircularTypeAlias2
	 * @phpstan-type CircularTypeAlias2 CircularTypeAlias1
	 * @phpstan-type int ShouldNotHappen
	 * @property GlobalTypeAlias $globalAliasProperty
	 * @property LocalTypeAlias $localAliasProperty
	 * @property ImportedTypeAlias $importedAliasProperty
	 * @property ReexportedTypeAlias $reexportedAliasProperty
	 * @property ScopedAlias $scopedAliasProperty
	 * @property RecursiveTypeAlias $recursiveAliasProperty
	 * @property CircularTypeAlias1 $circularAliasProperty
	 */
	class Foo
	{

		/**
		 * @param GlobalTypeAlias $parameter
		 */
		public function globalAlias($parameter)
		{
			assertType('int|string', $parameter);  // SKIP (GlobalTypeAlias comes from PHPStan's typeAliases setting, not the source)
		}

		/**
		 * @param LocalTypeAlias $parameter
		 */
		public function localAlias($parameter)
		{
			assertType('callable(string): (string|false)', $parameter);
		}

		/**
		 * @param NestedLocalTypeAlias $parameter
		 */
		public function nestedLocalAlias($parameter)
		{
			assertType('array<callable(string): (string|false)>', $parameter);
		}

		/**
		 * @param ImportedTypeAlias $parameter
		 */
		public function importedAlias($parameter)
		{
			assertType('Countable&Traversable', $parameter);
		}

		/**
		 * @param NestedImportedTypeAlias $parameter
		 */
		public function nestedImportedAlias($parameter)
		{
			assertType('iterable<Countable&Traversable>', $parameter);
		}

		/**
		 * @param ImportedAliasFromNonClass $parameter1
		 * @param ImportedAliasFromUnknownClass $parameter2
		 * @param ImportedUknownAlias $parameter3
		 */
		public function invalidImports($parameter1, $parameter2, $parameter3)
		{
			assertType('TypeAliasesDataset\ImportedAliasFromNonClass', $parameter1);
			assertType('TypeAliasesDataset\ImportedAliasFromUnknownClass', $parameter2);
			assertType('TypeAliasesDataset\ImportedUknownAlias', $parameter3);
		}

		/**
		 * @param Baz $parameter
		 */
		public function conflictingAlias($parameter)
		{
			assertType('never', $parameter);
		}

		public function __get(string $name)
		{
			return null;
		}

		/** @param int $int */
		public function testIntAlias($int)
		{
			assertType('int', $int);
		}

	}

	assertType('int|string', (new Foo)->globalAliasProperty);  // SKIP (GlobalTypeAlias comes from PHPStan's typeAliases setting, not the source)
	assertType('callable(string): (string|false)', (new Foo)->localAliasProperty);
	assertType('Countable&Traversable', (new Foo)->importedAliasProperty);
	assertType('Countable&Traversable', (new Foo)->reexportedAliasProperty);
	assertType('TypeAliasesDataset\SubScope\Foo', (new Foo)->scopedAliasProperty);
	assertType('*ERROR*', (new Foo)->recursiveAliasProperty);
	assertType('*ERROR*', (new Foo)->circularAliasProperty);

	trait FooTrait
	{

		/**
		 * @param Test $a
		 * @return Test
		 */
		public function doFoo($a)
		{
			assertType(Test::class, $a);
		}

	}

	/** @phpstan-type Test array{string, int} */
	class UsesTrait1
	{

		use FooTrait;

		/** @param Test $a */
		public function doBar($a)
		{
			assertType('array{string, int}', $a);
			assertType(Test::class, $this->doFoo());
		}

	}

	/** @phpstan-type Test \stdClass */
	class UsesTrait2
	{

		use FooTrait;

		/** @param Test $a */
		public function doBar($a)
		{
			assertType('stdClass', $a);
			assertType(Test::class, $this->doFoo());
		}

	}

	class UsesTrait3
	{

		use FooTrait;

		/** @param Test $a */
		public function doBar($a)
		{
			assertType(Test::class, $a);
			assertType(Test::class, $this->doFoo());
		}

	}

}
