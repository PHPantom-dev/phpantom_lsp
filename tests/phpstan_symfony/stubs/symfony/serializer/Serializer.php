<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\Serializer;

use Symfony\Component\Serializer\Encoder\ChainDecoder;
use Symfony\Component\Serializer\Encoder\ChainEncoder;
use Symfony\Component\Serializer\Encoder\ContextAwareDecoderInterface;
use Symfony\Component\Serializer\Encoder\ContextAwareEncoderInterface;
use Symfony\Component\Serializer\Encoder\DecoderInterface;
use Symfony\Component\Serializer\Encoder\EncoderInterface;
use Symfony\Component\Serializer\Exception\InvalidArgumentException;
use Symfony\Component\Serializer\Exception\LogicException;
use Symfony\Component\Serializer\Exception\NotNormalizableValueException;
use Symfony\Component\Serializer\Exception\PartialDenormalizationException;
use Symfony\Component\Serializer\Exception\UnsupportedFormatException;
use Symfony\Component\Serializer\Normalizer\AbstractObjectNormalizer;
use Symfony\Component\Serializer\Normalizer\DenormalizerAwareInterface;
use Symfony\Component\Serializer\Normalizer\DenormalizerInterface;
use Symfony\Component\Serializer\Normalizer\NormalizerAwareInterface;
use Symfony\Component\Serializer\Normalizer\NormalizerInterface;

/**
 * Serializer serializes and deserializes data.
 *
 * objects are turned into arrays by normalizers.
 * arrays are turned into various output formats by encoders.
 *
 *     $serializer->serialize($obj, 'xml')
 *     $serializer->decode($data, 'xml')
 *     $serializer->denormalize($data, 'Class', 'xml')
 *
 * @author Jordi Boggiano <j.boggiano@seld.be>
 * @author Johannes M. Schmitt <schmittjoh@gmail.com>
 * @author Lukas Kahwe Smith <smith@pooteeweet.org>
 * @author Kévin Dunglas <dunglas@gmail.com>
 */
class Serializer implements SerializerInterface, NormalizerInterface, DenormalizerInterface, ContextAwareEncoderInterface, ContextAwareDecoderInterface
{
    /**
     * Flag to control whether an empty array should be transformed to an
     * object (in JSON: {}) or to a list (in JSON: []).
     */
    public const EMPTY_ARRAY_AS_OBJECT = 'empty_array_as_object';

    private const SCALAR_TYPES = [
        'int' => true,
        'bool' => true,
        'float' => true,
        'string' => true,
    ];

    protected ChainEncoder $encoder;

    protected ChainDecoder $decoder;

    /**
     * @var array<string, array<string, array<bool>>>
     */
    private array $denormalizerCache = [];

    /**
     * @var array<string, array<string, array<bool>>>
     */
    private array $normalizerCache = [];

    /**
     * @param array<NormalizerInterface|DenormalizerInterface> $normalizers
     * @param array<EncoderInterface|DecoderInterface>         $encoders
     * @param array<string, mixed>                             $defaultContext
     */
    public function __construct(
        private array $normalizers = [],
        array $encoders = [],
        private array $defaultContext = [],
    ) {}

    final public function serialize(mixed $data, string $format, array $context = []): string
    {}

    final public function deserialize(mixed $data, string $type, string $format, array $context = []): mixed
    {}

    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {}

    /**
     * @throws NotNormalizableValueException
     * @throws PartialDenormalizationException Occurs when one or more properties of $type fails to denormalize
     */
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {}

    public function getSupportedTypes(?string $format): array
    {}

    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {}

    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {}

    /**
     * Returns a matching normalizer.
     *
     * @param mixed       $data    Data to get the serializer for
     * @param string|null $format  Format name, present to give the option to normalizers to act differently based on formats
     * @param array       $context Options available to the normalizer
     */
    private function getNormalizer(mixed $data, ?string $format, array $context): ?NormalizerInterface
    {}

    /**
     * Returns a matching denormalizer.
     *
     * @param mixed       $data    Data to restore
     * @param string      $class   The expected class to instantiate
     * @param string|null $format  Format name, present to give the option to normalizers to act differently based on formats
     * @param array       $context Options available to the denormalizer
     */
    private function getDenormalizer(mixed $data, string $class, ?string $format, array $context): ?DenormalizerInterface
    {}

    final public function encode(mixed $data, string $format, array $context = []): string
    {}

    final public function decode(string $data, string $format, array $context = []): mixed
    {}

    public function supportsEncoding(string $format, array $context = []): bool
    {}

    public function supportsDecoding(string $format, array $context = []): bool
    {}
}
