<?php // lint >= 8.1

declare(strict_types = 0);

namespace RememberNullablePropertyWhenStrictTypesDisabled;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

interface ObjectDataMapper
{
	/**
	 * @template OutType of object
	 *
	 * @param literal-string&class-string<OutType> $class
	 * @param mixed                                $data
	 *
	 * @return OutType
	 *
	 * @throws \Exception
	 */
	public function map(string $class, $data): object;
}

final class ApiProductController
{

	protected ?SearchProductsVM $searchProductsVM = null;

	protected static ?SearchProductsVM $searchProductsVMStatic = null;

	public function search(ObjectDataMapper $dataMapper): void
	{
		$this->searchProductsVM = $dataMapper->map(SearchProductsVM::class, $_REQUEST);
		assertType('RememberNullablePropertyWhenStrictTypesDisabled\SearchProductsVM', $this->searchProductsVM);
	}

	public function searchStatic(ObjectDataMapper $dataMapper): void
	{
		self::$searchProductsVMStatic = $dataMapper->map(SearchProductsVM::class, $_REQUEST);
		assertType('RememberNullablePropertyWhenStrictTypesDisabled\SearchProductsVM', self::$searchProductsVMStatic);
	}
}

class SearchProductsVM {}
