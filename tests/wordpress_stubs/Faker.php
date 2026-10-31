<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

/**
 * Class that provides fake types via docBlocks for type testing.
 *
 * @method static bool bool()
 * @method static true true()
 * @method static false false()
 * @method static int int()
 * @method static int<1, max> positiveInt()
 * @method static int<min, -1> negativeInt()
 * @method static int<min, 0> nonPositiveInt()
 * @method static int<0, max> nonNegativeInt()
 * @method static int<min, -1>|int<1, max> nonZeroInt()
 * @method static float float()
 * @method static string string()
 * @method static non-empty-string nonEmptyString()
 * @method static lowercase-string lowercaseString()
 * @method static numeric-string numericString()
 * @method static non-falsy-string nonFalsyString()
 * @method static scalar scalar()
 * @method static resource resource()
 * @method static object object()
 * @method static mixed mixed()
 * @method static callable callable()
 * @method static \stdClass stdClass()
 * @method static \WP_Block wpBlock()
 * @method static \WP_Comment wpComment()
 * @method static \WP_Dependencies wpDependencies()
 * @method static \WP_Error wpError()
 * @method static \WP_Locale wpLocale()
 * @method static \WP_Object_Cache wpObjectCache()
 * @method static \WP_Post wpPost()
 * @method static \WP_Query wpQuery()
 * @method static \WP_REST_Request wpRestRequest()
 * @method static \WP_REST_Response wpRestResponse()
 * @method static \WP_Screen wpScreen()
 * @method static \WP_Scripts wpScripts()
 * @method static \WP_Styles wpStyles()
 * @method static \WP_Term wpTerm()
 * @method static \WP_Theme wpTheme()
 * @method static \WP_Translations wpTranslations()
 * @method static \WP_User wpUser()
 * @method static \WP_Widget wpWidget()
 * @method static \WP_Widget_Factory wpWidgetFactory()
 * @method static \wpdb wpdb()
 */
class Faker
{
    /**
     * Fakes `array<Type>`. If `$type` is `null`, fakes `array<mixed>`.
     *
     * @template T
     * @param T $type
     * @return ($type is null ? array<array-key, mixed> : array<array-key, T>)
     */
    public static function array($type = null): array
    {
        return [$type];
    }

    /**
     * Fakes `array<int, Type>`. If `$type` is `null`, fakes `array<int, mixed>`.
     *
     * @template T
     * @param T|null $type
     * @return ($type is null ? array<int, mixed> : array<int, T>)
     */
    public static function intArray($type = null): array
    {
        return [$type];
    }

    /**
     * Fakes `array<string, Type>`. If `$type` is `null`, fakes `array<string, mixed>`.
     *
     * @template T
     * @param T|null $type
     * @return ($type is null ? array<string, mixed>: array<string, T>)
     */
    public static function strArray($type = null): array
    {
        return [self::string() => $type];
    }

    /**
     * Fakes `list<Type>`. If `$type` is `null`, fakes `list<mixed>`.
     *
     * @template T
     * @param T|null $type
     * @return ($type is null ? list<mixed> : list<T>)
     */
    public static function list($type = null): array
    {
        return [$type];
    }

    /**
     * Fakes `non-empty-array<Type>`, `non-empty-array<KeyType, ValueType>`,
     * and `non-empty-array<mixed>` if no type is specified.
     *
     * @template TKeyOrValue
     * @template TValue
     * @param TKeyOrValue $keyOrValueType
     * @param TValue $valueType
     * @return (TKeyOrValue is null ? non-empty-array<mixed> : (TValue is null ? non-empty-array<TKeyOrValue> : non-empty-array<TKeyOrValue, TValue>))
     *
     * @phpcs:disable Generic.CodeAnalysis.UnusedFunctionParameter.FoundAfterLastUsed
     */
    public static function nonEmptyArray($keyOrValueType = null, $valueType = null): array
    {
        return ['non-empty'];
    }

    /**
     * @template T
     * @param T ...$types
     * @return T
     *
     * @phpcs:disable NeutronStandard.Functions.TypeHint
     */
    public static function union(...$types)
    {
        return $types[0];
    }

    /**
     * @template TType1
     * @template TType2
     * @param TType1 $type1
     * @param TType2 $type2
     * @return (TType1&TType2)
     *
     * @phpcs:disable NeutronStandard.Functions.TypeHint
     * @phpstan-ignore return.missing
     */
    public static function intersection($type1, $type2)
    {
    }

    /**
     * Fakes a class string.
     *
     * @template T
     * @param T $theClass
     * @return (T is null ? class-string : class-string<T>)
     *
     * @phpcs:disable Generic.CodeAnalysis.UnusedFunctionParameter.Found
     */
    public static function classString($theClass = null): string
    {
        return self::class;
    }
}
