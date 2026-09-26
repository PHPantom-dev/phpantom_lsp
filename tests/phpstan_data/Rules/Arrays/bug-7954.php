<?php

namespace Bug7954;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-type ExtensionMap = array<string, array{
 *     global: bool,
 *     excludes: array<string, string>,
 *     admins: array<string, string>,
 *     implements: array<class-string, string>,
 *     extends: array<class-string, string>,
 *     instanceof: array<class-string, string>,
 *     uses: array<class-string, string>,
 *     priority: int,
 * }>
 * @phpstan-type SonataAdminConfigurationOptions = array{
 *     confirm_exit: bool,
 *     default_admin_route: string,
 *     default_group: string,
 *     default_icon: string,
 *     default_translation_domain: string,
 *     default_label_catalogue: string,
 *     dropdown_number_groups_per_colums: int,
 *     form_type: 'standard'|'horizontal',
 *     html5_validate: bool,
 *     js_debug: bool,
 *     list_action_button_content: 'text'|'icon'|'all',
 *     lock_protection: bool,
 *     logo_content: 'text'|'icon'|'all',
 *     mosaic_background: string,
 *     pager_links: int|null,
 *     skin: 'skin-black'|'skin-black-light'|'skin-blue'|'skin-blue-light'|'skin-green'|'skin-green-light'|'skin-purple'|'skin-purple-light'|'skin-red'|'skin-red-light'|'skin-yellow'|'skin-yellow-light',
 *     sort_admins: bool,
 *     use_bootlint: bool,
 *     use_icheck: bool,
 *     use_select2: bool,
 *     use_stickyforms: bool,
 * }
 * @phpstan-type SonataAdminConfiguration = array{
 *     assets: array{
 *         extra_javascripts: list<string>,
 *         extra_stylesheets: list<string>,
 *         javascripts: list<string>,
 *         remove_javascripts: list<string>,
 *         remove_stylesheets: list<string>,
 *         stylesheets: list<string>,
 *     },
 *     breadcrumbs: array{
 *         child_admin_route: string,
 *     },
 *     dashboard: array{
 *         blocks: array{
 *             class: string,
 *             position: string,
 *             roles: list<string>,
 *             settings: array<string, mixed>,
 *             type: string,
 *         },
 *         groups: array<string, array{
 *             label?: string,
 *             translation_domain?: string,
 *             icon?: string,
 *             items: array<Item>,
 *             keep_open: bool,
 *             on_top: bool,
 *             provider?: string,
 *             roles: list<string>
 *        }>,
 *     },
 *     default_admin_services: array{
 *         configuration_pool: string|null,
 *         datagrid_builder: string|null,
 *         data_source: string|null,
 *         field_description_factory: string|null,
 *         form_contractor: string|null,
 *         label_translator_strategy: string|null,
 *         list_builder: string|null,
 *         menu_factory: string|null,
 *         model_manager: string|null,
 *         pager_type: string|null,
 *         route_builder: string|null,
 *         route_generator: string|null,
 *         security_handler: string|null,
 *         show_builder: string|null,
 *         translator: string|null,
 *     },
 *     default_controller: string,
 *     extensions: array<string, ExtensionMap>,
 *     filter_persister: string,
 *     global_search: array{
 *         admin_route: string,
 *         empty_boxes: 'show'|'fade'|'hide',
 *     },
 *     options: SonataAdminConfigurationOptions,
 *     persist_filters: bool,
 *     security: array{
 *         acl_user_manager: string|null,
 *         admin_permissions: list<string>,
 *         information: array<string, list<string>>,
 *         object_permissions: list<string>,
 *         handler: string,
 *         role_admin: string,
 *         role_super_admin: string,
 *     },
 *     search: bool,
 *     show_mosaic_button: bool,
 *     templates: array{
 *         acl: string,
 *         action: string,
 *         action_create: string,
 *         add_block: string,
 *         ajax: string,
 *         base_list_field: string,
 *         batch: string,
 *         batch_confirmation: string,
 *         button_acl: string,
 *         button_create: string,
 *         button_edit: string,
 *         button_history: string,
 *         button_list: string,
 *         button_show: string,
 *         dashboard: string,
 *         delete: string,
 *         edit: string,
 *         filter: string,
 *         filter_theme: list<string>,
 *         form_theme: list<string>,
 *         history: string,
 *         history_revision_timestamp: string,
 *         inner_list_row: string,
 *         knp_menu_template: string,
 *         layout: string,
 *         list: string,
 *         list_block: string,
 *         outer_list_rows_list: string,
 *         outer_list_rows_mosaic: string,
 *         outer_list_rows_tree: string,
 *         pager_links: string,
 *         pager_results: string,
 *         preview: string,
 *         search: string,
 *         search_result_block: string,
 *         select: string,
 *         short_object_description: string,
 *         show: string,
 *         show_compare: string,
 *         tab_menu_template: string,
 *         user_block: string,
 *     },
 *     title: string,
 *     title_logo: string,
 * }
 **/
class HelloWorld
{
	/** @param SonataAdminConfiguration $config */
	public function sayHello(array $config): void
	{
		assertType('string', $config['security']['role_admin']);

		if (false === $config['options']['lock_protection']) {
			// things
		}

		assertType('string', $config['security']['role_admin']);

		switch ($config['security']['handler']) {
			case 'sonata.admin.security.handler.role':
				if (0 === \count($config['security']['information'])) {
					$config['security']['information'] = [
						'EDIT' => ['EDIT'],
						'LIST' => ['LIST'],
						'CREATE' => ['CREATE'],
						'VIEW' => ['VIEW'],
						'DELETE' => ['DELETE'],
						'EXPORT' => ['EXPORT'],
						'ALL' => ['ALL'],
					];
				}

				break;
			case 'sonata.admin.security.handler.acl':
				if (0 === \count($config['security']['information'])) {
					$config['security']['information'] = [
						'GUEST' => ['VIEW', 'LIST'],
						'STAFF' => ['EDIT', 'LIST', 'CREATE'],
						'EDITOR' => ['OPERATOR', 'EXPORT'],
						'ADMIN' => ['MASTER'],
					];
				}

				break;
		}

		assertType('string', $config['security']['role_admin']);
	}
}
