<?php // lint >= 8.1

namespace TemplateDefaultReferringOther;

use function PHPStan\Testing\assertType;

class MoneyValue
{

    public function __construct(
        public readonly string $currency,
        public readonly int $cents,
    )
    {
    }

}

/**
 * @template-contravariant DI
 * @template-contravariant EI
 * @template-covariant DO of EI = EI
 * @template-covariant EO of DI = DI
 */
interface Codec
{

    /**
     * @param DI $data
     * @return DO
     */
    public function decode(mixed $data): mixed;

    /**
     * @param EI $data
     * @return EO
     */
    public function encode(mixed $data): mixed;

}

/**
 * @implements Codec<
 *   array{currency: string, cents: int},
 *   MoneyValue,
 * >
 */
class MoneyCodec implements Codec
{

    public function decode(mixed $data): MoneyValue
    {
        return new MoneyValue($data['currency'], $data['cents']);
    }

    public function encode(mixed $data): array
    {
        return [
            'currency' => $data->currency,
            'cents' => $data->cents,
        ];
    }

}

/**
 * @implements Codec<
 *   string,
 *   \DateTimeInterface,
 *   \DateTimeImmutable,
 *   string,
 * >
 */
class DateTimeInterfaceCodec implements Codec
{

    public function decode(mixed $data): \DateTimeImmutable
    {
       return new \DateTimeImmutable($data);
    }

    public function encode(mixed $data): string
    {
        return $data->format('c');
    }

}

/**
 * @param Codec<array{currency: string, cents: int}, MoneyValue> $moneyCodec
 * @param Codec<string, \DateTimeInterface, \DateTimeImmutable, string> $dtCodec
 */
function test(
    Codec $moneyCodec,
    Codec $dtCodec,
    string $dtString,
    \DateTimeInterface $dtInterface,
): void
{
    assertType('TemplateDefaultReferringOther\MoneyValue', $moneyCodec->decode(['currency' => 'CZK', 'cents' => 123]));
    assertType('array{currency: string, cents: int}', $moneyCodec->encode(new MoneyValue('CZK', 100)));

    assertType('DateTimeImmutable', $dtCodec->decode($dtString));
    assertType('string', $dtCodec->encode($dtInterface));
}

function testMoneyCodecDirect(MoneyCodec $codec): void
{
    assertType('TemplateDefaultReferringOther\MoneyValue', $codec->decode(['currency' => 'CZK', 'cents' => 123]));
    assertType('array{currency: string, cents: int}', $codec->encode(new MoneyValue('CZK', 100)));
}

function testDateTimeCodecDirect(DateTimeInterfaceCodec $codec): void
{
    assertType('DateTimeImmutable', $codec->decode('2024-01-01'));
    assertType('string', $codec->encode(new \DateTimeImmutable()));
}
