<?php declare(strict_types = 1);

// The entity manager the ORM rule fixtures run against, as the source that
// builds it, with the custom DBAL types the rule tests register. PHPStan's
// Doctrine extension boots it to read the mapping; PHPantom reads the same
// mapping from the entities' attributes. Upstream maps most entities with
// annotations, which ORM 3 no longer reads.

use Carbon\Doctrine\CarbonImmutableType;
use Carbon\Doctrine\CarbonType;
use Doctrine\DBAL\DriverManager;
use Doctrine\DBAL\Types\DateTimeImmutableType;
use Doctrine\DBAL\Types\Type;
use Doctrine\ORM\Configuration;
use Doctrine\ORM\EntityManager;
use Doctrine\ORM\Mapping\Driver\AttributeDriver;
use Doctrine\Persistence\Mapping\Driver\MappingDriverChain;
use PHPStan\Rules\Doctrine\ORM\CustomNumericType;
use PHPStan\Rules\Doctrine\ORM\CustomType;
use PHPStan\Rules\Doctrine\ORM\FakeTestingRamseyUuidType;

$config = new Configuration();
$config->setProxyDir(__DIR__);
$config->setProxyNamespace('PHPstan\Doctrine\OrmProxies');

$metadataDriver = new MappingDriverChain();
$metadataDriver->addDriver(new AttributeDriver([__DIR__ . '/data']), 'PHPStan\\Rules\\Doctrine\\ORM\\');
$metadataDriver->addDriver(new AttributeDriver([__DIR__ . '/data-attributes']), 'PHPStan\\Rules\\Doctrine\\ORMAttributes\\');

$config->setMetadataDriverImpl($metadataDriver);

Type::overrideType('date', DateTimeImmutableType::class);
Type::addType(CustomType::NAME, CustomType::class);
Type::addType(CustomNumericType::NAME, CustomNumericType::class);
Type::addType(FakeTestingRamseyUuidType::NAME, FakeTestingRamseyUuidType::class);
Type::addType('carbon', CarbonType::class);
Type::addType('carbon_immutable', CarbonImmutableType::class);

return new EntityManager(
	DriverManager::getConnection([
		'driver' => 'pdo_sqlite',
		'memory' => true,
	]),
	$config,
);
