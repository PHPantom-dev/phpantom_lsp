<?php declare(strict_types = 1);

namespace Bug6845;

use function PHPStan\Testing\assertType;

class BaseActionLog { }

class RequestLog
{
	/**
	 * @template T of BaseActionLog
	 * @param class-string<T> $class
	 * @return T
	 */
	public function LogAction(string $class) : BaseActionLog
	{
		return new $class();
	}
}

interface CoreActionLog
{
	public function SetAdmin(bool $admin) : void;
}

class ActionLog extends BaseActionLog implements CoreActionLog
{
	public function SetAdmin(bool $admin) : void { }
}

class CoreApp
{
	/** @return class-string<CoreActionLog&BaseActionLog> */
	public static function getLogClass() : string
	{
		return ActionLog::class;
	}

	public function Run() : void
	{
		$requestlog = new RequestLog();
		$actionlog = $requestlog->LogAction(self::getLogClass());
		assertType('Bug6845\BaseActionLog&Bug6845\CoreActionLog', $actionlog);
	}
}
