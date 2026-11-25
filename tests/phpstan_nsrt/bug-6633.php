<?php declare(strict_types = 1);

namespace Bug6633;

use function PHPStan\Testing\assertType;

class CreateServiceSolrData
{
  public ?string $name;
  public ?string $version;
}

class CreateServiceRedisData
{
  public ?string $name;
  public ?string $version;
  public ?bool $persistent;

}

class ServiceSolr
{
	public function __construct(
		private string $name,
		private string $version,
	) {}

	public function touchAll() : string{
		return $this->name . $this->version;
	}
}

class ServiceRedis
{
	public function __construct(
		private string $name,
		private string $version,
		private bool $persistent,
	) {}

	public function touchAll() : string{
		return $this->persistent ? $this->name : $this->version;
	}
}

function test(?string $type = NULL) : void {
	$types = [
      'solr' => [
        'label' => 'SOLR Search',
        'data_class' => CreateServiceSolrData::class,
        'to_entity' => function (CreateServiceSolrData $data) {
          assert($data->name !== NULL && $data->version !== NULL, "Incorrect form validation");
          return new ServiceSolr($data->name, $data->version);
        },
      ],
	  'redis' => [
        'label' => 'Redis',
        'data_class' => CreateServiceRedisData::class,
        'to_entity' => function (CreateServiceRedisData $data) {
          assert($data->name !== NULL && $data->version !== NULL && $data->persistent !== NULL, "Incorrect form validation");
          return new ServiceRedis($data->name, $data->version, $data->persistent);
        },
      ],
    ];

	if ($type === NULL || !isset($types[$type])) {
		throw new \RuntimeException("404 or choice form here");
	}

    $data = new $types[$type]['data_class']();

    $service = $types[$type]['to_entity']($data);

	assertType('Bug6633\ServiceRedis|Bug6633\ServiceSolr', $service);
}
