<?php declare(strict_types = 1);

namespace ComplexGenericsExample;

use function PHPStan\Testing\assertType;

/**
 * @template TVariant of VariantInterface
 */
interface ExperimentInterface
{
}

interface VariantInterface
{
}

interface VariantRetrieverInterface
{
	/**
	 * @template TVariant of VariantInterface
	 * @param ExperimentInterface<TVariant> $experiment
	 * @return TVariant
	 */
	public function getVariant(ExperimentInterface $experiment): VariantInterface;
}

/**
 * @implements ExperimentInterface<SomeVariant>
 */
class SomeExperiment implements ExperimentInterface
{
}

class SomeVariant implements VariantInterface
{
}

class SomeClass
{
	private $variantRetriever;

	public function __construct(VariantRetrieverInterface $variantRetriever)
	{
		$this->variantRetriever = $variantRetriever;
	}

	public function someFunction(): void
	{
		assertType('ComplexGenericsExample\SomeVariant', $this->variantRetriever->getVariant(new SomeExperiment()));
	}
}
