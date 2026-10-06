<?php declare(strict_types = 1);

namespace Bug13392;

use RedisCluster;

use function PHPStan\Testing\assertType;

interface Client
{
	public function get(): RedisCluster;
}

function func(Client $client): void
{
	$redisCluster = $client->get();
	assertType('RedisCluster',$redisCluster);

	$transaction = $redisCluster->multi();
	assertType('(bool|RedisCluster)',$transaction);	
}
