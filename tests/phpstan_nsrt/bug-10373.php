<?php

namespace Bug10373;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param string $name Name of template file in the `templates/element/` folder,
	 *   or `MyPlugin.template` to use the template element from MyPlugin. If the element
	 *   is not found in the plugin, the normal view path cascade will be searched.
	 * @param array $data Array of data to be made available to the rendered view (i.e. the Element)
	 * @param array<string, mixed> $options Array of options. Possible keys are:
	 *
	 * - `cache` - Can either be `true`, to enable caching using the config in View::$elementCache. Or an array
	 *   If an array, the following keys can be used:
	 *
	 *   - `config` - Used to store the cached element in a custom cache configuration.
	 *   - `key` - Used to define the key used in the Cache::write(). It will be prefixed with `element_`
	 *
	 * - `callbacks` - Set to true to fire beforeRender and afterRender helper callbacks for this element.
	 *   Defaults to false.
	 * - `ignoreMissing` - Used to allow missing elements. Set to true to not throw exceptions.
	 * - `plugin` - setting to false will force to use the application's element from plugin templates, when the
	 *   plugin has element with same name. Defaults to true
	 * @return string Rendered Element
	 * @psalm-param array{cache?:array|true, callbacks?:bool, plugin?:string|false, ignoreMissing?:bool} $options
	 */
	public function element(string $name, array $data = [], array $options = []): string
	{
		$options += ['callbacks' => false, 'cache' => null, 'plugin' => null, 'ignoreMissing' => false];
		assertType('array|true|null', $options['cache']);
		if (isset($options['cache'])) {
			$options['cache'] = $this->_elementCache(
				$name,
				$data,
				array_diff_key($options, ['callbacks' => false, 'plugin' => null, 'ignoreMissing' => null])
			);
			assertType('array{key: string, config: string}', $options['cache']);
		} else {
			assertType('null', $options['cache']);
		}
		assertType('array{key: string, config: string}|null', $options['cache']);

		$pluginCheck = $options['plugin'] !== false;
		$file = $this->_getElementFileName($name, $pluginCheck);
		if ($file && $options['cache']) {
			assertType('array{key: string, config: string}', $options['cache']);
			return $this->cache(function (): void {
				echo '';
			}, $options['cache']);
		}

		return $file;
	}

	/**
	 * @param string $name
	 * @param bool $pluginCheck
	 */
	protected function _getElementFileName(string $name, bool $pluginCheck): string
	{
		return $name;
	}

	/**
	 * @param callable $block The block of code that you want to cache the output of.
	 * @param array<string, mixed> $options The options defining the cache key etc.
	 * @return string The rendered content.
	 * @throws \InvalidArgumentException When $options is lacking a 'key' option.
	 */
	public function cache(callable $block, array $options = []): string
	{
		$options += ['key' => '', 'config' => []];
		if (empty($options['key'])) {
			throw new \InvalidArgumentException('Cannot cache content with an empty key');
		}
		/** @var string $result */
		$result = $options['key'];
		if ($result) {
			return $result;
		}

		$bufferLevel = ob_get_level();
		ob_start();

		try {
			$block();
		} catch (\Throwable $exception) {
			while (ob_get_level() > $bufferLevel) {
				ob_end_clean();
			}

			throw $exception;
		}

		$result = (string)ob_get_clean();

		return $result;
	}

	/**
	 * Generate the cache configuration options for an element.
	 *
	 * @param string $name Element name
	 * @param array<mixed> $data Data
	 * @param array<string, mixed> $options Element options
	 * @return array Element Cache configuration.
	 * @psalm-return array{key:string, config:string}
	 */
	protected function _elementCache(string $name, array $data, array $options): array
	{
		[$plugin, $name] = explode(':', $name, 2);

		$pluginKey = null;
		if ($plugin) {
			$pluginKey = str_replace('/', '_', $plugin);
		}
		$elementKey = str_replace(['\\', '/'], '_', $name);

		$cache = $options['cache'];
		unset($options['cache']);
		$keys = array_merge(
			[$pluginKey, $elementKey],
			array_keys($options),
			array_keys($data)
		);
		$config = [
			'config' => [],
			'key' => implode('_', array_keys($keys)),
		];
		if (is_array($cache)) {
			$config = $cache + $config;
		}
		$config['key'] = 'element_' . $config['key'];

		/** @var array{config: string, key: string} */
		return $config;
	}
}
