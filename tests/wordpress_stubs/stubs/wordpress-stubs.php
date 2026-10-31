<?php
/**
 * The declarations of php-stubs/wordpress-stubs the fixtures touch, copied
 * by scripts/extract-wordpress-stubs.php.
 *
 * @see https://github.com/php-stubs/wordpress-stubs
 */

namespace {
    /**
     * PemFTP base class
     *
     */
    class ftp_base
    {
        var $LocalEcho;
        var $Verbose;
        var $OS_local;
        var $OS_remote;
        var $_lastaction;
        var $_errors;
        var $_type;
        var $_umask;
        var $_timeout;
        var $_passive;
        var $_host;
        var $_fullhost;
        var $_port;
        var $_datahost;
        var $_dataport;
        var $_ftp_control_sock;
        var $_ftp_data_sock;
        var $_ftp_temp_sock;
        var $_ftp_buff_size;
        var $_login;
        var $_password;
        var $_connected;
        var $_ready;
        var $_code;
        var $_message;
        var $_can_restore;
        var $_port_available;
        var $_curtype;
        var $_features;
        var $_error_array;
        var $AuthorizedTransferMode;
        var $OS_FullName;
        var $_eol_code;
        var $AutoAsciiExt;
        function __construct($port_mode = \FALSE, $verb = \FALSE, $le = \FALSE)
        {
        }
        function ftp_base($port_mode = \FALSE)
        {
        }
        function parselisting($line)
        {
        }
        function SendMSG($message = "", $crlf = \true)
        {
        }
        function SetType($mode = \FTP_AUTOASCII)
        {
        }
        function _settype($mode = \FTP_ASCII)
        {
        }
        function Passive($pasv = \NULL)
        {
        }
        function SetServer($host, $port = 21, $reconnect = \true)
        {
        }
        function SetUmask($umask = 022)
        {
        }
        function SetTimeout($timeout = 30)
        {
        }
        function connect($server = \NULL)
        {
        }
        function quit($force = \false)
        {
        }
        function login($user = \NULL, $pass = \NULL)
        {
        }
        function pwd()
        {
        }
        function cdup()
        {
        }
        function chdir($pathname)
        {
        }
        function rmdir($pathname)
        {
        }
        function mkdir($pathname)
        {
        }
        function rename($from, $to)
        {
        }
        function filesize($pathname)
        {
        }
        function abort()
        {
        }
        function mdtm($pathname)
        {
        }
        function systype()
        {
        }
        function delete($pathname)
        {
        }
        function site($command, $fnction = "site")
        {
        }
        function chmod($pathname, $mode)
        {
        }
        function restore($from)
        {
        }
        function features()
        {
        }
        function rawlist($pathname = "", $arg = "")
        {
        }
        function nlist($pathname = "", $arg = "")
        {
        }
        function is_exists($pathname)
        {
        }
        function file_exists($pathname)
        {
        }
        function fget($fp, $remotefile, $rest = 0)
        {
        }
        function get($remotefile, $localfile = \NULL, $rest = 0)
        {
        }
        function fput($remotefile, $fp, $rest = 0)
        {
        }
        function put($localfile, $remotefile = \NULL, $rest = 0)
        {
        }
        function mput($local = ".", $remote = \NULL, $continious = \false)
        {
        }
        function mget($remote, $local = ".", $continious = \false)
        {
        }
        function mdel($remote, $continious = \false)
        {
        }
        function mmkdir($dir, $mode = 0777)
        {
        }
        function glob($pattern, $handle = \NULL)
        {
        }
        function glob_pattern_match($pattern, $subject)
        {
        }
        function glob_regexp($pattern, $subject)
        {
        }
        function dirlist($remote)
        {
        }
        function _checkCode()
        {
        }
        function _list($arg = "", $cmd = "LIST", $fnction = "_list")
        {
        }
        function PushError($fctname, $msg, $desc = \false)
        {
        }
        function PopError()
        {
        }
    }
    /**
     * Socket Based FTP implementation
     *
     * @package PemFTP
     * @subpackage Socket
     * @since 2.5.0
     *
     * @version 1.0
     * @copyright Alexey Dotsenko
     * @author Alexey Dotsenko
     * @link https://www.phpclasses.org/package/1743-PHP-FTP-client-in-pure-PHP.html
     * @license LGPL https://opensource.org/licenses/lgpl-license.html
     */
    class ftp_sockets extends \ftp_base
    {
        function __construct($verb = \FALSE, $le = \FALSE)
        {
        }
        function _settimeout($sock)
        {
        }
        function _connect($host, $port)
        {
        }
        function _readmsg($fnction = "_readmsg")
        {
        }
        function _exec($cmd, $fnction = "_exec")
        {
        }
        function _data_prepare($mode = \FTP_ASCII)
        {
        }
        function _data_read($mode = \FTP_ASCII, $fp = \NULL)
        {
        }
        function _data_write($mode = \FTP_ASCII, $fp = \NULL)
        {
        }
        function _data_write_block($mode, $block)
        {
        }
        function _data_close()
        {
        }
        function _quit()
        {
        }
    }
    class ftp extends \ftp_sockets
    {
    }
    /**
     * A class for displaying various tree-like structures.
     *
     * Extend the Walker class to use it, see examples below. Child classes
     * do not need to implement all of the abstract methods in the class. The child
     * only needs to implement the methods that are needed.
     *
     * @since 2.1.0
     *
     * @package WordPress
     * @abstract
     */
    #[\AllowDynamicProperties]
    class Walker
    {
        /**
         * What the class handles.
         *
         * @since 2.1.0
         * @var string
         */
        public $tree_type;
        /**
         * DB fields to use.
         *
         * @since 2.1.0
         * @var string[]
         */
        public $db_fields;
        /**
         * Max number of pages walked by the paged walker.
         *
         * @since 2.7.0
         * @var int
         */
        public $max_pages = 1;
        /**
         * Whether the current element has children or not.
         *
         * To be used in start_el().
         *
         * @since 4.0.0
         * @var bool
         */
        public $has_children;
        /**
         * Starts the list before the elements are added.
         *
         * The $args parameter holds additional values that may be used with the child
         * class methods. This method is called at the start of the output list.
         *
         * @since 2.1.0
         * @abstract
         *
         * @param string $output Used to append additional content (passed by reference).
         * @param int    $depth  Depth of the item.
         * @param array  $args   An array of additional arguments.
         */
        public function start_lvl(&$output, $depth = 0, $args = array())
        {
        }
        /**
         * Ends the list of after the elements are added.
         *
         * The $args parameter holds additional values that may be used with the child
         * class methods. This method finishes the list at the end of output of the elements.
         *
         * @since 2.1.0
         * @abstract
         *
         * @param string $output Used to append additional content (passed by reference).
         * @param int    $depth  Depth of the item.
         * @param array  $args   An array of additional arguments.
         */
        public function end_lvl(&$output, $depth = 0, $args = array())
        {
        }
        /**
         * Starts the element output.
         *
         * The $args parameter holds additional values that may be used with the child
         * class methods. Also includes the element output.
         *
         * @since 2.1.0
         * @since 5.9.0 Renamed `$object` (a PHP reserved keyword) to `$data_object` for PHP 8 named parameter support.
         * @abstract
         *
         * @param string $output            Used to append additional content (passed by reference).
         * @param object $data_object       The data object.
         * @param int    $depth             Depth of the item.
         * @param array  $args              An array of additional arguments.
         * @param int    $current_object_id Optional. ID of the current item. Default 0.
         */
        public function start_el(&$output, $data_object, $depth = 0, $args = array(), $current_object_id = 0)
        {
        }
        /**
         * Ends the element output, if needed.
         *
         * The $args parameter holds additional values that may be used with the child class methods.
         *
         * @since 2.1.0
         * @since 5.9.0 Renamed `$object` (a PHP reserved keyword) to `$data_object` for PHP 8 named parameter support.
         * @abstract
         *
         * @param string $output      Used to append additional content (passed by reference).
         * @param object $data_object The data object.
         * @param int    $depth       Depth of the item.
         * @param array  $args        An array of additional arguments.
         */
        public function end_el(&$output, $data_object, $depth = 0, $args = array())
        {
        }
        /**
         * Traverses elements to create list from elements.
         *
         * Display one element if the element doesn't have any children otherwise,
         * display the element and its children. Will only traverse up to the max
         * depth and no ignore elements under that depth. It is possible to set the
         * max depth to include all depths, see walk() method.
         *
         * This method should not be called directly, use the walk() method instead.
         *
         * @since 2.5.0
         *
         * @param object $element           Data object.
         * @param array  $children_elements List of elements to continue traversing (passed by reference).
         * @param int    $max_depth         Max depth to traverse.
         * @param int    $depth             Depth of current element.
         * @param array  $args              An array of arguments.
         * @param string $output            Used to append additional content (passed by reference).
         * @phpstan-return void
         */
        public function display_element($element, &$children_elements, $max_depth, $depth, $args, &$output)
        {
        }
        /**
         * Displays array of elements hierarchically.
         *
         * Does not assume any existing order of elements.
         *
         * $max_depth = -1 means flatly display every element.
         * $max_depth = 0 means display all levels.
         * $max_depth > 0 specifies the number of display levels.
         *
         * @since 2.1.0
         * @since 5.3.0 Formalized the existing `...$args` parameter by adding it
         *              to the function signature.
         *
         * @param array $elements  An array of elements.
         * @param int   $max_depth The maximum hierarchical depth.
         * @param mixed ...$args   Optional additional arguments.
         * @return string The hierarchical item output.
         */
        public function walk($elements, $max_depth, ...$args)
        {
        }
        /**
         * Produces a page of nested elements.
         *
         * Given an array of hierarchical elements, the maximum depth, a specific page number,
         * and number of elements per page, this function first determines all top level root elements
         * belonging to that page, then lists them and all of their children in hierarchical order.
         *
         * $max_depth = 0 means display all levels.
         * $max_depth > 0 specifies the number of display levels.
         *
         * @since 2.7.0
         * @since 5.3.0 Formalized the existing `...$args` parameter by adding it
         *              to the function signature.
         *
         * @param array $elements  An array of elements.
         * @param int   $max_depth The maximum hierarchical depth.
         * @param int   $page_num  The specific page number, beginning with 1.
         * @param int   $per_page  Number of elements per page.
         * @param mixed ...$args   Optional additional arguments.
         * @return string XHTML of the specified page of elements.
         */
        public function paged_walk($elements, $max_depth, $page_num, $per_page, ...$args)
        {
        }
        /**
         * Calculates the total number of root elements.
         *
         * @since 2.7.0
         *
         * @param array $elements Elements to list.
         * @return int Number of root elements.
         */
        public function get_number_of_root_elements($elements)
        {
        }
        /**
         * Unsets all the children for a given top level element.
         *
         * @since 2.7.0
         *
         * @param object $element           The top level element.
         * @param array  $children_elements The children elements.
         * @phpstan-return void
         */
        public function unset_children($element, &$children_elements)
        {
        }
    }
    /**
     * Base class for displaying a list of items in an ajaxified HTML table.
     *
     * @since 3.1.0
     */
    #[\AllowDynamicProperties]
    class WP_List_Table
    {
        /**
         * The current list of items.
         *
         * @since 3.1.0
         *
         * @var array<int|string, mixed>
         */
        public $items;
        /**
         * Various information about the current table.
         *
         * @since 3.1.0
         *
         * @var array<string, mixed>
         */
        protected $_args;
        /**
         * Various information needed for displaying the pagination.
         *
         * @since 3.1.0
         *
         * @var array<string, mixed>
         */
        protected $_pagination_args = array();
        /**
         * The current screen.
         *
         * @since 3.1.0
         *
         * @var WP_Screen
         */
        protected $screen;
        /**
         * The view switcher modes.
         *
         * @since 4.1.0
         *
         * @var array<string, string>
         */
        protected $modes = array();
        /**
         * Stores the value returned by {@see self::get_column_info()}.
         *
         * @since 4.2.0
         *
         * @var array<int, array|string>|null
         */
        protected $_column_headers;
        /**
         * List of private properties made readable for backward compatibility.
         *
         * @since 4.2.0
         *
         * @var string[]
         */
        protected $compat_fields = array('_args', '_pagination_args', 'screen', '_actions', '_pagination');
        /**
         * List of private/protected methods made readable for backward compatibility.
         *
         * @since 4.2.0
         *
         * @var string[]
         */
        protected $compat_methods = array('set_pagination_args', 'get_views', 'get_bulk_actions', 'bulk_actions', 'row_actions', 'months_dropdown', 'view_switcher', 'comments_bubble', 'get_items_per_page', 'pagination', 'get_sortable_columns', 'get_column_info', 'get_table_classes', 'display_tablenav', 'extra_tablenav', 'single_row_columns');
        /**
         * Constructor.
         *
         * The child class should call this constructor from its own constructor to override
         * the default $args.
         *
         * @since 3.2.0
         *
         * @param array|string $args {
         *     Array or string of arguments.
         *
         *     @type string $plural   Plural value used for labels and the objects being listed.
         *                            This affects things such as CSS class-names and nonces used
         *                            in the list table, e.g. 'posts'. Default empty.
         *     @type string $singular Singular label for an object being listed, e.g. 'post'.
         *                            Default empty
         *     @type bool   $ajax     Whether the list table supports Ajax. This includes loading
         *                            and sorting data, for example. If true, the class will call
         *                            the _js_vars() method in the footer to provide variables
         *                            to any scripts handling Ajax events. Default false.
         *     @type string $screen   String containing the hook name used to determine the current
         *                            screen. If left null, the current screen will be automatically set.
         *                            Default null.
         * }
         * @phpstan-param array{
         *   plural?: string,
         *   singular?: string,
         *   ajax?: bool,
         *   screen?: string,
         * } $args
         */
        public function __construct($args = array())
        {
        }
        /**
         * Makes private properties readable for backward compatibility.
         *
         * @since 4.0.0
         * @since 6.4.0 Getting a dynamic property is deprecated.
         *
         * @param string $name Property to get.
         * @return mixed Property.
         */
        public function __get($name)
        {
        }
        /**
         * Makes private properties settable for backward compatibility.
         *
         * @since 4.0.0
         * @since 6.4.0 Setting a dynamic property is deprecated.
         *
         * @param string $name  Property to check if set.
         * @param mixed  $value Property value.
         * @phpstan-return void
         */
        public function __set($name, $value)
        {
        }
        /**
         * Makes private properties checkable for backward compatibility.
         *
         * @since 4.0.0
         * @since 6.4.0 Checking a dynamic property is deprecated.
         *
         * @param string $name Property to check if set.
         * @return bool Whether the property is a back-compat property and it is set.
         */
        public function __isset($name)
        {
        }
        /**
         * Makes private properties un-settable for backward compatibility.
         *
         * @since 4.0.0
         * @since 6.4.0 Unsetting a dynamic property is deprecated.
         *
         * @param string $name Property to unset.
         * @phpstan-return void
         */
        public function __unset($name)
        {
        }
        /**
         * Makes private/protected methods readable for backward compatibility.
         *
         * @since 4.0.0
         *
         * @param string $name      Method to call.
         * @param array  $arguments Arguments to pass when calling.
         * @return mixed|bool Return value of the callback, false otherwise.
         */
        public function __call($name, $arguments)
        {
        }
        /**
         * Checks the current user's permissions.
         *
         * @since 3.1.0
         * @abstract
         */
        public function ajax_user_can()
        {
        }
        /**
         * Prepares the list of items for displaying.
         *
         * @uses WP_List_Table::set_pagination_args()
         *
         * @since 3.1.0
         * @abstract
         */
        public function prepare_items()
        {
        }
        /**
         * Sets all the necessary pagination arguments.
         *
         * @since 3.1.0
         *
         * @param array|string $args Array or string of arguments with information about the pagination.
         * @phpstan-param array{total_items?: int, total_pages?: int, per_page?: int} $args
         * @phpstan-return void
         */
        protected function set_pagination_args($args)
        {
        }
        /**
         * Access the pagination args.
         *
         * @since 3.1.0
         *
         * @param string $key Pagination argument to retrieve. Common values include 'total_items',
         *                    'total_pages', 'per_page', or 'infinite_scroll'.
         * @return int Number of items that correspond to the given pagination argument.
         */
        public function get_pagination_arg($key)
        {
        }
        /**
         * Determines whether the table has items to display or not.
         *
         * @since 3.1.0
         *
         * @return bool Whether the table has items to display.
         */
        public function has_items()
        {
        }
        /**
         * Message to be displayed when there are no items.
         *
         * @since 3.1.0
         */
        public function no_items()
        {
        }
        /**
         * Displays the search box.
         *
         * @since 3.1.0
         *
         * @param string $text     The 'submit' button label.
         * @param string $input_id ID attribute value for the search input field.
         * @phpstan-return void
         */
        public function search_box($text, $input_id)
        {
        }
        /**
         * Generates views links.
         *
         * @since 6.1.0
         *
         * @param array $link_data {
         *     An array of link data.
         *
         *     @type string $url     The link URL.
         *     @type string $label   The link label.
         *     @type bool   $current Optional. Whether this is the currently selected view.
         * }
         * @return string[] An array of link markup. Keys match the `$link_data` input array.
         * @phpstan-param array{
         *   url?: string,
         *   label?: string,
         *   current?: bool,
         * } $link_data
         */
        protected function get_views_links($link_data = array())
        {
        }
        /**
         * Gets the list of views available on this table.
         *
         * The format is an associative array:
         * - `'id' => 'link'`
         *
         * @since 3.1.0
         *
         * @return array<string, string> An associative array of views.
         */
        protected function get_views()
        {
        }
        /**
         * Displays the list of views available on this table.
         *
         * @since 3.1.0
         * @phpstan-return void
         */
        public function views()
        {
        }
        /**
         * Retrieves the list of bulk actions available for this table.
         *
         * The format is an associative array where each element represents either a top level option value and label, or
         * an array representing an optgroup and its options.
         *
         * For a standard option, the array element key is the field value and the array element value is the field label.
         *
         * For an optgroup, the array element key is the label and the array element value is an associative array of
         * options as above.
         *
         * Example:
         *
         *     [
         *         'edit'         => 'Edit',
         *         'delete'       => 'Delete',
         *         'Change State' => [
         *             'feature' => 'Featured',
         *             'sale'    => 'On Sale',
         *         ]
         *     ]
         *
         * @since 3.1.0
         * @since 5.6.0 A bulk action can now contain an array of options in order to create an optgroup.
         *
         * @return array<string, string|array<string, string>> An associative array of bulk actions.
         */
        protected function get_bulk_actions()
        {
        }
        /**
         * Displays the bulk actions dropdown.
         *
         * @since 3.1.0
         *
         * @param string $which The location of the bulk actions: Either 'top' or 'bottom'.
         *                      This is designated as optional for backward compatibility.
         * @phpstan-param 'top'|'bottom' $which
         * @phpstan-return void
         */
        protected function bulk_actions($which = '')
        {
        }
        /**
         * Gets the current action selected from the bulk actions dropdown.
         *
         * @since 3.1.0
         *
         * @return string|false The action name. False if no action was selected.
         */
        public function current_action()
        {
        }
        /**
         * Generates the required HTML for a list of row action links.
         *
         * @since 3.1.0
         *
         * @param string[] $actions        An array of action links.
         * @param bool     $always_visible Whether the actions should be always visible.
         * @return string The HTML for the row actions.
         */
        protected function row_actions($actions, $always_visible = \false)
        {
        }
        /**
         * Displays a dropdown for filtering items in the list table by month.
         *
         * @since 3.1.0
         *
         * @global wpdb      $wpdb      WordPress database abstraction object.
         * @global WP_Locale $wp_locale WordPress date and time locale object.
         *
         * @param string $post_type The post type.
         * @phpstan-return void
         */
        protected function months_dropdown($post_type)
        {
        }
        /**
         * Displays a view switcher.
         *
         * @since 3.1.0
         *
         * @param string $current_mode The current view mode slug, e.g. 'list' or 'excerpt'.
         */
        protected function view_switcher($current_mode)
        {
        }
        /**
         * Displays a comment count bubble.
         *
         * @since 3.1.0
         *
         * @param int $post_id          The post ID.
         * @param int $pending_comments Number of pending comments.
         */
        protected function comments_bubble($post_id, $pending_comments)
        {
        }
        /**
         * Gets the current page number.
         *
         * @since 3.1.0
         *
         * @return int Current page number.
         */
        public function get_pagenum()
        {
        }
        /**
         * Gets the number of items to display on a single page.
         *
         * @since 3.1.0
         *
         * @param string $option        User option name.
         * @param int    $default_value Optional. The number of items to display. Default 20.
         * @return int Number of items to display per page.
         */
        protected function get_items_per_page($option, $default_value = 20)
        {
        }
        /**
         * Displays the pagination.
         *
         * @since 3.1.0
         *
         * @param string $which The location of the pagination: Either 'top' or 'bottom'.
         * @phpstan-param 'top'|'bottom' $which
         * @phpstan-return void
         */
        protected function pagination($which)
        {
        }
        /**
         * Gets a list of columns.
         *
         * The format is:
         * - `'internal-name' => 'Title'`
         *
         * @since 3.1.0
         * @abstract
         *
         * @return array<string, string> An associative array of columns.
         */
        public function get_columns()
        {
        }
        /**
         * Gets a list of sortable columns.
         *
         * The format is:
         * - `'internal-name' => 'orderby'`
         * - `'internal-name' => array( 'orderby', bool, 'abbr', 'orderby-text', 'initially-sorted-column-order' )` -
         * - `'internal-name' => array( 'orderby', 'asc' )` - The second element sets the initial sorting order.
         * - `'internal-name' => array( 'orderby', true )`  - The second element makes the initial order descending.
         *
         * In the second format, passing true as second parameter will make the initial
         * sorting order be descending. Following parameters add a short column name to
         * be used as 'abbr' attribute, a translatable string for the current sorting,
         * and the initial order for the initial sorted column, 'asc' or 'desc' (default: false).
         *
         * @since 3.1.0
         * @since 6.3.0 Added 'abbr', 'orderby-text' and 'initially-sorted-column-order'.
         *
         * @return array<string, array<int, string|bool>|string> An associative array of sortable columns.
         */
        protected function get_sortable_columns()
        {
        }
        /**
         * Gets the name of the default primary column.
         *
         * @since 4.3.0
         *
         * @return string Name of the default primary column, in this case, an empty string.
         */
        protected function get_default_primary_column_name()
        {
        }
        /**
         * Gets the name of the primary column.
         *
         * Public wrapper for WP_List_Table::get_default_primary_column_name().
         *
         * @since 4.4.0
         *
         * @return string Name of the default primary column.
         */
        public function get_primary_column()
        {
        }
        /**
         * Gets the name of the primary column.
         *
         * @since 4.3.0
         *
         * @return string The name of the primary column.
         */
        protected function get_primary_column_name()
        {
        }
        /**
         * Gets a list of all, hidden, and sortable columns, with filter applied.
         *
         * @since 3.1.0
         *
         * @return array<int, array|string> Column information.
         */
        protected function get_column_info()
        {
        }
        /**
         * Returns the number of visible columns.
         *
         * @since 3.1.0
         *
         * @return int The number of visible columns.
         */
        public function get_column_count()
        {
        }
        /**
         * Prints column headers, accounting for hidden and sortable columns.
         *
         * @since 3.1.0
         *
         * @param bool $with_id Whether to set the ID attribute or not. Default true.
         */
        public function print_column_headers($with_id = \true)
        {
        }
        /**
         * Print a table description with information about current sorting and order.
         *
         * For the table initial view, information about initial orderby and order
         * should be provided via get_sortable_columns().
         *
         * @since 6.3.0
         * @phpstan-return void
         */
        public function print_table_description()
        {
        }
        /**
         * Displays the table.
         *
         * @since 3.1.0
         */
        public function display()
        {
        }
        /**
         * Gets a list of CSS classes for the WP_List_Table table tag.
         *
         * @since 3.1.0
         *
         * @return string[] Array of CSS classes for the table tag.
         */
        protected function get_table_classes()
        {
        }
        /**
         * Generates the table navigation above or below the table.
         *
         * @since 3.1.0
         *
         * @param string $which The location of the navigation: Either 'top' or 'bottom'.
         * @phpstan-param 'top'|'bottom' $which
         * @phpstan-return void
         */
        protected function display_tablenav($which)
        {
        }
        /**
         * Displays extra controls between bulk actions and pagination.
         *
         * @since 3.1.0
         *
         * @param string $which The location: 'top' or 'bottom'.
         * @phpstan-param 'top'|'bottom' $which
         */
        protected function extra_tablenav($which)
        {
        }
        /**
         * Generates the tbody element for the list table.
         *
         * @since 3.1.0
         */
        public function display_rows_or_placeholder()
        {
        }
        /**
         * Generates the list table rows.
         *
         * @since 3.1.0
         */
        public function display_rows()
        {
        }
        /**
         * Generates content for a single row of the table.
         *
         * @since 3.1.0
         *
         * @param object|array $item The current item
         */
        public function single_row($item)
        {
        }
        /**
         * Handles an unknown column.
         *
         * @since 4.2.0
         *
         * @param object|array $item        The current item.
         * @param string       $column_name Name of the column.
         */
        protected function column_default($item, $column_name)
        {
        }
        /**
         * Handles the checkbox column output.
         *
         * @since 4.2.0
         *
         * @param object|array $item The current item.
         */
        protected function column_cb($item)
        {
        }
        /**
         * Returns a clean, human-readable label for the primary column's row header.
         *
         * Used as the `aria-label` attribute value on the `<th scope="row">` element,
         * giving screen readers a concise cell name instead of computing it from
         * the full cell content (which may include row action links, excerpts, etc.).
         *
         * Subclasses should override this method to return the item's primary
         * identifier (e.g. post title, plugin name, username). Return an empty string
         * to omit the attribute.
         *
         * @since 7.1.0
         *
         * @param object|array $item The current item.
         * @return string The aria-label value, or an empty string.
         */
        protected function get_primary_column_aria_label($item)
        {
        }
        /**
         * Generates the columns for a single row of the table.
         *
         * @since 3.1.0
         *
         * @param object|array $item The current item.
         */
        protected function single_row_columns($item)
        {
        }
        /**
         * Generates and display row actions links for the list table.
         *
         * @since 4.3.0
         *
         * @param object|array $item        The item being acted upon.
         * @param string       $column_name Current column name.
         * @param string       $primary     Primary column name.
         * @return string The row actions HTML, or an empty string
         *                if the current column is not the primary column.
         */
        protected function handle_row_actions($item, $column_name, $primary)
        {
        }
        /**
         * Handles an incoming ajax request (called from admin-ajax.php)
         *
         * @since 3.1.0
         *
         * @return never
         */
        public function ajax_response()
        {
        }
        /**
         * Sends required variables to JavaScript land.
         *
         * @since 3.1.0
         */
        public function _js_vars()
        {
        }
    }
    /**
     * Class for displaying the list of application password items.
     *
     * @since 5.6.0
     *
     * @see WP_List_Table
     */
    class WP_Application_Passwords_List_Table extends \WP_List_Table
    {
        /**
         * Gets the list of columns.
         *
         * @since 5.6.0
         *
         * @return string[] Array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         * Prepares the list of items for displaying.
         *
         * @since 5.6.0
         *
         * @global int $user_id User ID.
         */
        public function prepare_items()
        {
        }
        /**
         * Handles the name column output.
         *
         * @since 5.6.0
         *
         * @param array $item The current application password item.
         */
        public function column_name($item)
        {
        }
        /**
         * Handles the created column output.
         *
         * @since 5.6.0
         *
         * @param array $item The current application password item.
         */
        public function column_created($item)
        {
        }
        /**
         * Handles the last used column output.
         *
         * @since 5.6.0
         *
         * @param array $item The current application password item.
         */
        public function column_last_used($item)
        {
        }
        /**
         * Handles the last ip column output.
         *
         * @since 5.6.0
         *
         * @param array $item The current application password item.
         */
        public function column_last_ip($item)
        {
        }
        /**
         * Handles the revoke column output.
         *
         * @since 5.6.0
         *
         * @param array $item The current application password item.
         */
        public function column_revoke($item)
        {
        }
        /**
         * Generates content for a single row of the table
         *
         * @since 5.6.0
         *
         * @param array  $item        The current item.
         * @param string $column_name The current column name.
         */
        protected function column_default($item, $column_name)
        {
        }
        /**
         * Generates custom table navigation to prevent conflicting nonces.
         *
         * @since 5.6.0
         *
         * @param string $which The location of the bulk actions: Either 'top' or 'bottom'.
         * @phpstan-param 'top'|'bottom' $which
         */
        protected function display_tablenav($which)
        {
        }
        /**
         * Generates content for a single row of the table.
         *
         * @since 5.6.0
         *
         * @param array $item The current item.
         */
        public function single_row($item)
        {
        }
        /**
         * Gets the name of the default primary column.
         *
         * @since 5.6.0
         *
         * @return string Name of the default primary column, in this case, 'name'.
         */
        protected function get_default_primary_column_name()
        {
        }
        /**
         * Prints the JavaScript template for the new row item.
         *
         * @since 5.6.0
         */
        public function print_js_template_row()
        {
        }
    }
    /**
     * Core class used to implement displaying comments in a list table.
     *
     * @since 3.1.0
     *
     * @see WP_List_Table
     */
    class WP_Comments_List_Table extends \WP_List_Table
    {
        public $checkbox = \true;
        public $pending_count = array();
        public $extra_items;
        /**
         * Constructor.
         *
         * @since 3.1.0
         *
         * @see WP_List_Table::__construct() for more information on default arguments.
         *
         * @global int $post_id The ID of the post to show comments for.
         *
         * @param array $args An associative array of arguments.
         */
        public function __construct($args = array())
        {
        }
        /**
         * Adds avatars to comment author names.
         *
         * @since 3.1.0
         *
         * @param string $name       Comment author name.
         * @param int    $comment_id Comment ID.
         * @return string Avatar with the user name.
         * @phpstan-return non-falsy-string
         */
        public function floated_admin_avatar($name, $comment_id)
        {
        }
        /**
         * Checks if the user can edit posts.
         *
         * @return bool Whether the user can edit posts.
         */
        public function ajax_user_can()
        {
        }
        /**
         * Prepares the comments list items.
         *
         * @global string $mode           Current list table display mode.
         * @global int    $post_id        Current post ID filter.
         * @global string $comment_status Comment status filter.
         * @global string $comment_type   Comment type filter.
         * @global string $search         Search term.
         */
        public function prepare_items()
        {
        }
        /**
         * Gets the number of comments to display per page.
         *
         * @param string $comment_status Comment status.
         * @return int Comments per page.
         */
        public function get_per_page($comment_status = 'all')
        {
        }
        /**
         * Displays a message when no comments are found.
         * @global string $comment_status The current comment status filter.
         */
        public function no_items()
        {
        }
        /**
         * Returns an array of comment status links.
         *
         * @global int    $post_id        The ID of the post to show comments for.
         * @global string $comment_status The current comment status.
         * @global string $comment_type   The current comment type.
         *
         * @return array<string, string> Comment status HTML links keyed by view.
         */
        protected function get_views()
        {
        }
        /**
         * Gets the available bulk actions for the comments list.
         *
         * @global string $comment_status Current comment status filter.
         *
         * @return array<string, string> Bulk action labels keyed by action name.
         */
        protected function get_bulk_actions()
        {
        }
        /**
         * Displays extra controls between bulk actions and pagination.
         *
         * @global string $comment_status Current comment status.
         * @global string $comment_type   Current comment type.
         *
         * @param string $which The location of the extra table nav markup: Either 'top' or 'bottom'.
         * @phpstan-param 'top'|'bottom' $which
         */
        protected function extra_tablenav($which)
        {
        }
        /**
         * Gets the current action selected from the bulk actions dropdown.
         *
         * @return string|false Current action or false if none.
         */
        public function current_action()
        {
        }
        /**
         * Gets the list of columns.
         *
         * @global int $post_id The ID of the post comments are being shown for.
         *
         * @return string[] Array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         * Displays a comment type drop-down for filtering on the Comments list table.
         *
         * @since 5.5.0
         * @since 5.6.0 Renamed from `comment_status_dropdown()` to `comment_type_dropdown()`.
         *
         * @param string $comment_type The current comment type slug.
         */
        protected function comment_type_dropdown($comment_type)
        {
        }
        /**
         * Gets a list of sortable columns.
         *
         * @return array<string, string|array> The sortable columns.
         */
        protected function get_sortable_columns()
        {
        }
        /**
         * Gets the name of the default primary column.
         *
         * @since 4.3.0
         *
         * @return string Name of the default primary column, in this case, 'comment'.
         */
        protected function get_default_primary_column_name()
        {
        }
        /**
         * Displays the comments table.
         *
         * Overrides the parent display() method to render extra comments.
         *
         * @since 3.1.0
         */
        public function display()
        {
        }
        /**
         * Generates content for a single row of the table.
         *
         * @global WP_Post    $post    Global post object.
         * @global WP_Comment $comment Global comment object.
         *
         * @param WP_Comment $item The comment object.
         */
        public function single_row($item)
        {
        }
        /**
         * Generates and displays row actions links.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$comment` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @global string $comment_status Status for the current listed comments.
         *
         * @param WP_Comment $item        The comment object.
         * @param string     $column_name Current column name.
         * @param string     $primary     Primary column name.
         * @return string Row actions output for comments. An empty string
         *                if the current column is not the primary column,
         *                or if the current user cannot edit the comment.
         */
        protected function handle_row_actions($item, $column_name, $primary)
        {
        }
        /**
         * Outputs the checkbox column.
         *
         * @since 5.9.0 Renamed `$comment` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_Comment $item The comment object.
         */
        public function column_cb($item)
        {
        }
        /**
         * Outputs the comment column.
         *
         * @param WP_Comment $comment The comment object.
         */
        public function column_comment($comment)
        {
        }
        /**
         * Outputs the author column.
         *
         * @global string $comment_status The current comment status.
         *
         * @param WP_Comment $comment The comment object.
         */
        public function column_author($comment)
        {
        }
        /**
         * Outputs the date column.
         *
         * @param WP_Comment $comment The comment object.
         */
        public function column_date($comment)
        {
        }
        /**
         * Outputs the response column.
         *
         * @param WP_Comment $comment The comment object.
         * @phpstan-return void
         */
        public function column_response($comment)
        {
        }
        /**
         * Outputs the default column.
         *
         * @since 5.9.0 Renamed `$comment` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_Comment $item        The comment object.
         * @param string     $column_name The custom column's name.
         */
        public function column_default($item, $column_name)
        {
        }
    }
    /**
     * Base WordPress Filesystem class which Filesystem implementations extend.
     *
     * @since 2.5.0
     *
     * @phpstan-type FileListing array{
     *     name: string,
     *     perms?: string,
     *     permsn?: string,
     *     number?: int|string|false,
     *     owner?: string|int<1, max>|false,
     *     group?: string|int<1, max>|false,
     *     size: int|string|false,
     *     lastmodunix?: int|string|false,
     *     lastmod?: string|false,
     *     time: int|string|false,
     *     type: 'd'|'f'|'l',
     *     islink?: bool,
     *     isdir?: bool,
     *     files?: mixed[]|false, // The mixed[] is actually FileListing[] but PHPStan does not support recursive or self-referencing array shapes.
     * }
     */
    #[\AllowDynamicProperties]
    class WP_Filesystem_Base
    {
        /**
         * Whether to display debug data for the connection.
         *
         * @since 2.5.0
         * @var bool
         */
        public $verbose = \false;
        /**
         * Cached list of local filepaths to mapped remote filepaths.
         *
         * @since 2.7.0
         * @var array<string, string>
         */
        public $cache = array();
        /**
         * The Access method of the current connection, Set automatically.
         *
         * @since 2.5.0
         * @var string
         */
        public $method = '';
        /**
         * @var WP_Error
         */
        public $errors = \null;
        /**
         * @var array<string, mixed>
         */
        public $options = array();
        /**
         * Returns the path on the remote filesystem of ABSPATH.
         *
         * @since 2.7.0
         *
         * @return string|false The location of the remote path, or false on failure.
         */
        public function abspath()
        {
        }
        /**
         * Returns the path on the remote filesystem of WP_CONTENT_DIR.
         *
         * @since 2.7.0
         *
         * @return string|false The location of the remote path, or false on failure.
         */
        public function wp_content_dir()
        {
        }
        /**
         * Returns the path on the remote filesystem of WP_PLUGIN_DIR.
         *
         * @since 2.7.0
         *
         * @return string|false The location of the remote path, or false on failure.
         */
        public function wp_plugins_dir()
        {
        }
        /**
         * Returns the path on the remote filesystem of the Themes Directory.
         *
         * @since 2.7.0
         *
         * @param string|false $theme Optional. The theme stylesheet or template for the directory.
         *                            Default false.
         * @return string|false The location of the remote path, or false on failure.
         */
        public function wp_themes_dir($theme = \false)
        {
        }
        /**
         * Returns the path on the remote filesystem of WP_LANG_DIR.
         *
         * @since 3.2.0
         *
         * @return string|false The location of the remote path, or false on failure.
         */
        public function wp_lang_dir()
        {
        }
        /**
         * Locates a folder on the remote filesystem.
         *
         * @since 2.5.0
         * @deprecated 2.7.0 use WP_Filesystem_Base::abspath() or WP_Filesystem_Base::wp_*_dir() instead.
         * @see WP_Filesystem_Base::abspath()
         * @see WP_Filesystem_Base::wp_content_dir()
         * @see WP_Filesystem_Base::wp_plugins_dir()
         * @see WP_Filesystem_Base::wp_themes_dir()
         * @see WP_Filesystem_Base::wp_lang_dir()
         *
         * @param string $base    Optional. The folder to start searching from. Default '.'.
         * @param bool   $verbose Optional. True to display debug information. Default false.
         * @return string|false The location of the remote path, or false on failure.
         */
        public function find_base_dir($base = '.', $verbose = \false)
        {
        }
        /**
         * Locates a folder on the remote filesystem.
         *
         * @since 2.5.0
         * @deprecated 2.7.0 use WP_Filesystem_Base::abspath() or WP_Filesystem_Base::wp_*_dir() methods instead.
         * @see WP_Filesystem_Base::abspath()
         * @see WP_Filesystem_Base::wp_content_dir()
         * @see WP_Filesystem_Base::wp_plugins_dir()
         * @see WP_Filesystem_Base::wp_themes_dir()
         * @see WP_Filesystem_Base::wp_lang_dir()
         *
         * @param string $base    Optional. The folder to start searching from. Default '.'.
         * @param bool   $verbose Optional. True to display debug information. Default false.
         * @return string|false The location of the remote path, or false on failure.
         */
        public function get_base_dir($base = '.', $verbose = \false)
        {
        }
        /**
         * Locates a folder on the remote filesystem.
         *
         * Assumes that on Windows systems, Stripping off the Drive
         * letter is OK Sanitizes \\ to / in Windows filepaths.
         *
         * @since 2.7.0
         *
         * @param string $folder the folder to locate.
         * @return string|false The location of the remote path, false on failure.
         */
        public function find_folder($folder)
        {
        }
        /**
         * Locates a folder on the remote filesystem.
         *
         * Expects Windows sanitized path.
         *
         * @since 2.7.0
         *
         * @param string $folder The folder to locate.
         * @param string $base   The folder to start searching from.
         * @param bool   $loop   If the function has recursed. Internal use only.
         * @return string|false The location of the remote path, false to cease looping.
         */
        public function search_for_folder($folder, $base = '.', $loop = \false)
        {
        }
        /**
         * Returns the *nix-style file permissions for a file.
         *
         * From the PHP documentation page for fileperms().
         *
         * @link https://www.php.net/manual/en/function.fileperms.php
         *
         * @since 2.5.0
         *
         * @param string $file String filename.
         * @return string The *nix-style representation of permissions.
         */
        public function gethchmod($file)
        {
        }
        /**
         * Gets the permissions of the specified file or filepath in their octal format.
         *
         * @since 2.5.0
         *
         * @param string $file Path to the file.
         * @return string Mode of the file (the last 3 digits).
         */
        public function getchmod($file)
        {
        }
        /**
         * Converts *nix-style file permissions to an octal number.
         *
         * Converts '-rw-r--r--' to 0644
         * From "info at rvgate dot nl"'s comment on the PHP documentation for chmod()
         *
         * @link https://www.php.net/manual/en/function.chmod.php#49614
         *
         * @since 2.5.0
         *
         * @param string $mode string The *nix-style file permissions.
         * @return string Octal representation of permissions.
         */
        public function getnumchmodfromh($mode)
        {
        }
        /**
         * Determines if the string provided contains binary characters.
         *
         * @since 2.7.0
         *
         * @param string $text String to test against.
         * @return bool True if string is binary, false otherwise.
         */
        public function is_binary($text)
        {
        }
        /**
         * Changes the owner of a file or directory.
         *
         * Default behavior is to do nothing, override this in your subclass, if desired.
         *
         * @since 2.5.0
         *
         * @param string     $file      Path to the file or directory.
         * @param string|int $owner     A user name or number.
         * @param bool       $recursive Optional. If set to true, changes file owner recursively.
         *                              Default false.
         * @return bool True on success, false on failure.
         */
        public function chown($file, $owner, $recursive = \false)
        {
        }
        /**
         * Connects filesystem.
         *
         * @since 2.5.0
         * @abstract
         *
         * @return bool True on success, false on failure (always true for WP_Filesystem_Direct).
         */
        public function connect()
        {
        }
        /**
         * Reads entire file into a string.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $file Name of the file to read.
         * @return string|false Read data on success, false on failure.
         */
        public function get_contents($file)
        {
        }
        /**
         * Reads entire file into an array.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $file Path to the file.
         * @return string[]|false File contents in an array on success, false on failure.
         */
        public function get_contents_array($file)
        {
        }
        /**
         * Writes a string to a file.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string    $file     Remote path to the file where to write the data.
         * @param string    $contents The data to write.
         * @param int|false $mode     Optional. The file permissions as octal number, usually 0644.
         *                            Default false.
         * @return bool True on success, false on failure.
         */
        public function put_contents($file, $contents, $mode = \false)
        {
        }
        /**
         * Gets the current working directory.
         *
         * @since 2.5.0
         * @abstract
         *
         * @return string|false The current working directory on success, false on failure.
         */
        public function cwd()
        {
        }
        /**
         * Changes current directory.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $dir The new current directory.
         * @return bool True on success, false on failure.
         */
        public function chdir($dir)
        {
        }
        /**
         * Changes the file group.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string     $file      Path to the file.
         * @param string|int $group     A group name or number.
         * @param bool       $recursive Optional. If set to true, changes file group recursively.
         *                              Default false.
         * @return bool True on success, false on failure.
         */
        public function chgrp($file, $group, $recursive = \false)
        {
        }
        /**
         * Changes filesystem permissions.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string    $file      Path to the file.
         * @param int|false $mode      Optional. The permissions as octal number, usually 0644 for files,
         *                             0755 for directories. Default false.
         * @param bool      $recursive Optional. If set to true, changes file permissions recursively.
         *                             Default false.
         * @return bool True on success, false on failure.
         */
        public function chmod($file, $mode = \false, $recursive = \false)
        {
        }
        /**
         * Gets the file owner.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $file Path to the file.
         * @return string|false Username of the owner on success, false on failure.
         */
        public function owner($file)
        {
        }
        /**
         * Gets the file's group.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $file Path to the file.
         * @return string|false The group on success, false on failure.
         */
        public function group($file)
        {
        }
        /**
         * Copies a file.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string    $source      Path to the source file.
         * @param string    $destination Path to the destination file.
         * @param bool      $overwrite   Optional. Whether to overwrite the destination file if it exists.
         *                               Default false.
         * @param int|false $mode        Optional. The permissions as octal number, usually 0644 for files,
         *                               0755 for dirs. Default false.
         * @return bool True on success, false on failure.
         */
        public function copy($source, $destination, $overwrite = \false, $mode = \false)
        {
        }
        /**
         * Moves a file.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $source      Path to the source file.
         * @param string $destination Path to the destination file.
         * @param bool   $overwrite   Optional. Whether to overwrite the destination file if it exists.
         *                            Default false.
         * @return bool True on success, false on failure.
         */
        public function move($source, $destination, $overwrite = \false)
        {
        }
        /**
         * Deletes a file or directory.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string       $file      Path to the file or directory.
         * @param bool         $recursive Optional. If set to true, deletes files and folders recursively.
         *                                Default false.
         * @param string|false $type      Type of resource. 'f' for file, 'd' for directory.
         *                                Default false.
         * @return bool True on success, false on failure.
         */
        public function delete($file, $recursive = \false, $type = \false)
        {
        }
        /**
         * Checks if a file or directory exists.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $path Path to file or directory.
         * @return bool Whether $path exists or not.
         */
        public function exists($path)
        {
        }
        /**
         * Checks if resource is a file.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $file File path.
         * @return bool Whether $file is a file.
         */
        public function is_file($file)
        {
        }
        /**
         * Checks if resource is a directory.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $path Directory path.
         * @return bool Whether $path is a directory.
         */
        public function is_dir($path)
        {
        }
        /**
         * Checks if a file is readable.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $file Path to file.
         * @return bool Whether $file is readable.
         */
        public function is_readable($file)
        {
        }
        /**
         * Checks if a file or directory is writable.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $path Path to file or directory.
         * @return bool Whether $path is writable.
         */
        public function is_writable($path)
        {
        }
        /**
         * Gets the file's last access time.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $file Path to file.
         * @return int|false Unix timestamp representing last access time, false on failure.
         */
        public function atime($file)
        {
        }
        /**
         * Gets the file modification time.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $file Path to file.
         * @return int|false Unix timestamp representing modification time, false on failure.
         */
        public function mtime($file)
        {
        }
        /**
         * Gets the file size (in bytes).
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $file Path to file.
         * @return int|false Size of the file in bytes on success, false on failure.
         */
        public function size($file)
        {
        }
        /**
         * Sets the access and modification times of a file.
         *
         * Note: If $file doesn't exist, it will be created.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $file  Path to file.
         * @param int    $time  Optional. Modified time to set for file.
         *                      Default 0.
         * @param int    $atime Optional. Access time to set for file.
         *                      Default 0.
         * @return bool True on success, false on failure.
         */
        public function touch($file, $time = 0, $atime = 0)
        {
        }
        /**
         * Creates a directory.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string           $path  Path for new directory.
         * @param int|false        $chmod Optional. The permissions as octal number (or false to skip chmod).
         *                                Default false.
         * @param string|int|false $chown Optional. A user name or number (or false to skip chown).
         *                                Default false.
         * @param string|int|false $chgrp Optional. A group name or number (or false to skip chgrp).
         *                                Default false.
         * @return bool True on success, false on failure.
         */
        public function mkdir($path, $chmod = \false, $chown = \false, $chgrp = \false)
        {
        }
        /**
         * Deletes a directory.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $path      Path to directory.
         * @param bool   $recursive Optional. Whether to recursively remove files/directories.
         *                          Default false.
         * @return bool True on success, false on failure.
         */
        public function rmdir($path, $recursive = \false)
        {
        }
        /**
         * Gets details for files in a directory or a specific file.
         *
         * @since 2.5.0
         * @abstract
         *
         * @param string $path           Path to directory or file.
         * @param bool   $include_hidden Optional. Whether to include details of hidden ("." prefixed) files.
         *                               Default true.
         * @param bool   $recursive      Optional. Whether to recursively include file details in nested directories.
         *                               Default false.
         * @return array|false {
         *     Array of arrays containing file information. False if unable to list directory contents.
         *
         *     @type array ...$0 {
         *         Array of file information. Note that some elements may not be available on all filesystems.
         *
         *         @type string           $name        Name of the file or directory.
         *         @type string           $perms       *nix representation of permissions.
         *         @type string           $permsn      Octal representation of permissions.
         *         @type int|string|false $number      File number. May be a numeric string. False if not available.
         *         @type string|false     $owner       Owner name or ID, or false if not available.
         *         @type string|false     $group       File permissions group, or false if not available.
         *         @type int|string|false $size        Size of file in bytes. May be a numeric string.
         *                                             False if not available.
         *         @type int|string|false $lastmodunix Last modified unix timestamp. May be a numeric string.
         *                                             False if not available.
         *         @type string|false     $lastmod     Last modified month (3 letters) and day (without leading 0), or
         *                                             false if not available.
         *         @type int|string|false $time        Last modified time. A Unix timestamp on FTP transports, or false if not available.
         *         @type string           $type        Type of resource. 'f' for file, 'd' for directory, 'l' for link.
         *         @type array|false      $files       If a directory and `$recursive` is true, contains another array of
         *                                             files. False if unable to list directory contents.
         *     }
         * }
         * @phpstan-return array<string, FileListing>|false
         */
        public function dirlist($path, $include_hidden = \true, $recursive = \false)
        {
        }
    }
    /**
     * Core class used to implement displaying links in a list table.
     *
     * @since 3.1.0
     *
     * @see WP_List_Table
     */
    class WP_Links_List_Table extends \WP_List_Table
    {
        /**
         * Constructor.
         *
         * @since 3.1.0
         *
         * @see WP_List_Table::__construct() for more information on default arguments.
         *
         * @param array $args An associative array of arguments.
         */
        public function __construct($args = array())
        {
        }
        /**
         * @return bool
         */
        public function ajax_user_can()
        {
        }
        /**
         * @global int    $cat_id  Link category ID.
         * @global string $s       Search string.
         * @global string $orderby The field to order the links by.
         * @global string $order   The direction to order the links.
         */
        public function prepare_items()
        {
        }
        /**
         * Displays the message for no items.
         */
        public function no_items()
        {
        }
        /**
         * Gets the list of bulk actions.
         *
         * @return array
         */
        protected function get_bulk_actions()
        {
        }
        /**
         * @global int $cat_id Link category ID.
         * @param string $which The location: 'top' or 'bottom'.
         * @phpstan-param 'top'|'bottom' $which
         * @phpstan-return void
         */
        protected function extra_tablenav($which)
        {
        }
        /**
         * @return string[] Array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         * Gets the list of sortable columns.
         *
         * @return array
         */
        protected function get_sortable_columns()
        {
        }
        /**
         * Gets the name of the default primary column.
         *
         * @since 4.3.0
         *
         * @return string Name of the default primary column, in this case, 'name'.
         */
        protected function get_default_primary_column_name()
        {
        }
        /**
         * Handles the checkbox column output.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$link` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param object $item The current link object.
         */
        public function column_cb($item)
        {
        }
        /**
         * Handles the link name column output.
         *
         * @since 4.3.0
         *
         * @param object $link The current link object.
         */
        public function column_name($link)
        {
        }
        /**
         * Handles the link URL column output.
         *
         * @since 4.3.0
         *
         * @param object $link The current link object.
         */
        public function column_url($link)
        {
        }
        /**
         * Handles the link categories column output.
         *
         * @since 4.3.0
         *
         * @global int $cat_id Link category ID.
         *
         * @param object $link The current link object.
         */
        public function column_categories($link)
        {
        }
        /**
         * Handles the link relation column output.
         *
         * @since 4.3.0
         *
         * @param object $link The current link object.
         */
        public function column_rel($link)
        {
        }
        /**
         * Handles the link visibility column output.
         *
         * @since 4.3.0
         *
         * @param object $link The current link object.
         */
        public function column_visible($link)
        {
        }
        /**
         * Handles the link rating column output.
         *
         * @since 4.3.0
         *
         * @param object $link The current link object.
         */
        public function column_rating($link)
        {
        }
        /**
         * Handles the default column output.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$link` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param object $item        Link object.
         * @param string $column_name Current column name.
         */
        public function column_default($item, $column_name)
        {
        }
        /**
         * Generates the list table rows.
         *
         * @since 3.1.0
         */
        public function display_rows()
        {
        }
        /**
         * Generates and displays row action links.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$link` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param object $item        Link being acted upon.
         * @param string $column_name Current column name.
         * @param string $primary     Primary column name.
         * @return string Row actions output for links, or an empty string
         *                if the current column is not the primary column.
         */
        protected function handle_row_actions($item, $column_name, $primary)
        {
        }
        /**
         * Returns a clean label for the primary (Name) column's row header `aria-label`.
         *
         * Provides screen readers with just the link name as the row header name,
         * preventing them from computing the name from the full cell content.
         *
         * @since 7.1.0
         *
         * @param object $link The current link object.
         * @return string The link name.
         */
        protected function get_primary_column_aria_label($link)
        {
        }
    }
    /**
     * Core class used to implement displaying media items in a list table.
     *
     * @since 3.1.0
     *
     * @see WP_List_Table
     */
    class WP_Media_List_Table extends \WP_List_Table
    {
        /**
         * Holds the number of pending comments for each post.
         *
         * @since 4.4.0
         * @var array
         */
        protected $comment_pending_count = array();
        /**
         * Constructor.
         *
         * @since 3.1.0
         *
         * @see WP_List_Table::__construct() for more information on default arguments.
         *
         * @param array $args An associative array of arguments.
         */
        public function __construct($args = array())
        {
        }
        /**
         * Checks if the current user has permissions to upload files.
         *
         * @since 3.1.0
         *
         * @return bool Whether the user can upload files.
         */
        public function ajax_user_can()
        {
        }
        /**
         * Prepares the list of items for displaying.
         *
         * @since 3.1.0
         *
         * @global string   $mode                  List table view mode.
         * @global WP_Query $wp_query              WordPress Query object.
         * @global array    $post_mime_types       An array of post mime types.
         * @global array    $avail_post_mime_types An array of available post mime types.
         */
        public function prepare_items()
        {
        }
        /**
         * Gets an array of links for the available views on this table.
         *
         * @since 3.1.0
         *
         * @global array $post_mime_types       An array of post mime types.
         * @global array $avail_post_mime_types An array of available post mime types.
         * @return array<string, string> An array of links for the available views.
         */
        protected function get_views()
        {
        }
        /**
         * @return array<string, string> An associative array of bulk actions.
         */
        protected function get_bulk_actions()
        {
        }
        /**
         * Displays extra controls between bulk actions and pagination.
         *
         * @since 3.1.0
         *
         * @param string $which The location of the extra table nav: 'top' or 'bottom'.
         * @phpstan-param 'top'|'bottom' $which
         * @phpstan-return void
         */
        protected function extra_tablenav($which)
        {
        }
        /**
         * @return string|false The current action.
         */
        public function current_action()
        {
        }
        /**
         * @return bool Whether the list table has items to display.
         */
        public function has_items()
        {
        }
        /**
         * Displays a message when no media items are found.
         *
         * @since 3.1.0
         */
        public function no_items()
        {
        }
        /**
         * Overrides parent views to use the filter bar display.
         *
         * @global string $mode List table view mode.
         */
        public function views()
        {
        }
        /**
         * @return string[] Array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         * @return array<string, array<int, mixed>> An array of sortable columns.
         */
        protected function get_sortable_columns()
        {
        }
        /**
         * Handles the checkbox column output.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$post` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_Post $item The current WP_Post object.
         */
        public function column_cb($item)
        {
        }
        /**
         * Handles the title column output.
         *
         * @since 4.3.0
         *
         * @param WP_Post $post The current WP_Post object.
         */
        public function column_title($post)
        {
        }
        /**
         * Handles the author column output.
         *
         * @since 4.3.0
         * @since 6.8.0 Added fallback text when author's name is unknown.
         *
         * @param WP_Post $post The current WP_Post object.
         */
        public function column_author($post)
        {
        }
        /**
         * Handles the description column output.
         *
         * @since 4.3.0
         * @deprecated 6.2.0
         *
         * @param WP_Post $post The current WP_Post object.
         */
        public function column_desc($post)
        {
        }
        /**
         * Handles the date column output.
         *
         * @since 4.3.0
         *
         * @param WP_Post $post The current WP_Post object.
         */
        public function column_date($post)
        {
        }
        /**
         * Handles the parent column output.
         *
         * @since 4.3.0
         *
         * @param WP_Post $post The current WP_Post object.
         */
        public function column_parent($post)
        {
        }
        /**
         * Handles the comments column output.
         *
         * @since 4.3.0
         *
         * @param WP_Post $post The current WP_Post object.
         */
        public function column_comments($post)
        {
        }
        /**
         * Handles output for the default column.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$post` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_Post $item        The current WP_Post object.
         * @param string  $column_name Current column name.
         * @phpstan-return void
         */
        public function column_default($item, $column_name)
        {
        }
        /**
         * Generates the list table rows.
         *
         * @since 3.1.0
         *
         * @global WP_Post  $post     Global post object.
         * @global WP_Query $wp_query WordPress Query object.
         */
        public function display_rows()
        {
        }
        /**
         * Gets the name of the default primary column.
         *
         * @since 4.3.0
         *
         * @return string Name of the default primary column, in this case, 'title'.
         */
        protected function get_default_primary_column_name()
        {
        }
        /**
         * Generates and displays row action links.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$post` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_Post $item        Attachment being acted upon.
         * @param string  $column_name Current column name.
         * @param string  $primary     Primary column name.
         * @return string Row actions output for media attachments, or an empty string
         *                if the current column is not the primary column.
         */
        protected function handle_row_actions($item, $column_name, $primary)
        {
        }
        /**
         * Returns a clean label for the primary (File) column's row header `aria-label`.
         *
         * Provides screen readers with just the attachment title as the row header
         * name, preventing them from computing the name from the full cell content.
         *
         * @since 7.1.0
         *
         * @param WP_Post $post The current WP_Post object.
         * @return string The attachment title.
         */
        protected function get_primary_column_aria_label($post)
        {
        }
    }
    /**
     * Core class used to implement displaying sites in a list table for the network admin.
     *
     * @since 3.1.0
     *
     * @see WP_List_Table
     */
    class WP_MS_Sites_List_Table extends \WP_List_Table
    {
        /**
         * Site status list.
         *
         * @since 4.3.0
         * @var array
         */
        public $status_list;
        /**
         * Constructor.
         *
         * @since 3.1.0
         *
         * @see WP_List_Table::__construct() for more information on default arguments.
         *
         * @param array $args An associative array of arguments.
         */
        public function __construct($args = array())
        {
        }
        /**
         * Checks if the current user has permissions to manage sites.
         *
         * @since 3.1.0
         *
         * @return bool Whether the user can manage sites.
         */
        public function ajax_user_can()
        {
        }
        /**
         * Prepares the list of sites for display.
         *
         * @since 3.1.0
         *
         * @global string $mode List table view mode.
         * @global string $s    Search string.
         * @global wpdb   $wpdb WordPress database abstraction object.
         */
        public function prepare_items()
        {
        }
        /**
         * Displays a message when no sites are found.
         *
         * @since 3.1.0
         */
        public function no_items()
        {
        }
        /**
         * Gets links to filter sites by status.
         *
         * @since 5.3.0
         *
         * @return array
         */
        protected function get_views()
        {
        }
        /**
         * Gets an associative array of bulk actions for this table.
         *
         * @since 3.1.0
         *
         * @return array<string, string> An associative array of bulk actions.
         */
        protected function get_bulk_actions()
        {
        }
        /**
         * Displays the pagination.
         *
         * @since 3.1.0
         *
         * @global string $mode List table view mode.
         *
         * @param string $which The location of the pagination nav markup: Either 'top' or 'bottom'.
         * @phpstan-param 'top'|'bottom' $which
         */
        protected function pagination($which)
        {
        }
        /**
         * Displays extra controls between bulk actions and pagination.
         *
         * @since 5.3.0
         *
         * @param string $which The location of the extra table nav markup: Either 'top' or 'bottom'.
         * @phpstan-param 'top'|'bottom' $which
         */
        protected function extra_tablenav($which)
        {
        }
        /**
         * Gets an array of column titles keyed by their column name.
         *
         * @since 3.1.0
         *
         * @return string[] Array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         * Gets an array of sortable columns.
         *
         * @since 3.1.0
         *
         * @return array<string, mixed> An array of sortable columns.
         */
        protected function get_sortable_columns()
        {
        }
        /**
         * Handles the checkbox column output.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$blog` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param array $item Current site.
         */
        public function column_cb($item)
        {
        }
        /**
         * Handles the ID column output.
         *
         * @since 4.4.0
         *
         * @param array $blog Current site.
         */
        public function column_id($blog)
        {
        }
        /**
         * Handles the site name column output.
         *
         * @since 4.3.0
         *
         * @global string $mode List table view mode.
         *
         * @param array $blog Current site.
         */
        public function column_blogname($blog)
        {
        }
        /**
         * Handles the lastupdated column output.
         *
         * @since 4.3.0
         *
         * @global string $mode List table view mode.
         *
         * @param array $blog Current site.
         */
        public function column_lastupdated($blog)
        {
        }
        /**
         * Handles the registered column output.
         *
         * @since 4.3.0
         *
         * @global string $mode List table view mode.
         *
         * @param array $blog Current site.
         */
        public function column_registered($blog)
        {
        }
        /**
         * Handles the users column output.
         *
         * @since 4.3.0
         *
         * @param array $blog Current site.
         */
        public function column_users($blog)
        {
        }
        /**
         * Handles the plugins column output.
         *
         * @since 4.3.0
         *
         * @param array $blog Current site.
         */
        public function column_plugins($blog)
        {
        }
        /**
         * Handles output for the default column.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$blog` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param array  $item        Current site.
         * @param string $column_name Current column name.
         */
        public function column_default($item, $column_name)
        {
        }
        /**
         * Generates the list table rows.
         *
         * @since 3.1.0
         */
        public function display_rows()
        {
        }
        /**
         * Determines whether to output comma-separated site states.
         *
         * @since 5.3.0
         *
         * @param array<string, mixed> $site An array of site data.
         */
        protected function site_states($site)
        {
        }
        /**
         * Gets the name of the default primary column.
         *
         * @since 4.3.0
         *
         * @return string Name of the default primary column, in this case, 'blogname'.
         */
        protected function get_default_primary_column_name()
        {
        }
        /**
         * Generates and displays row action links.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$blog` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param array  $item        Site being acted upon.
         * @param string $column_name Current column name.
         * @param string $primary     Primary column name.
         * @return string Row actions output for sites in Multisite, or an empty string
         *                if the current column is not the primary column.
         */
        protected function handle_row_actions($item, $column_name, $primary)
        {
        }
        /**
         * Returns a clean label for the primary (URL) column's row header `aria-label`.
         *
         * Provides screen readers with just the site title as the row header name,
         * preventing them from computing the name from the full cell content.
         *
         * @since 7.1.0
         *
         * @param array $blog The current site properties array.
         * @return string The site title, or the site URL (domain + path) if the title is empty.
         */
        protected function get_primary_column_aria_label($blog)
        {
        }
    }
    /**
     * Core class used to implement displaying themes in a list table for the network admin.
     *
     * @since 3.1.0
     *
     * @see WP_List_Table
     */
    class WP_MS_Themes_List_Table extends \WP_List_Table
    {
        public $site_id;
        public $is_site_themes;
        /**
         * Whether to show the auto-updates UI.
         *
         * @since 5.5.0
         *
         * @var bool True if auto-updates UI is to be shown, false otherwise.
         */
        protected $show_autoupdates = \true;
        /**
         * Constructor.
         *
         * @since 3.1.0
         *
         * @see WP_List_Table::__construct() for more information on default arguments.
         *
         * @global string $status The current theme status.
         * @global int    $page   The current page number.
         *
         * @param array $args An associative array of arguments.
         */
        public function __construct($args = array())
        {
        }
        /**
         * Gets the list of CSS classes for the table tag.
         *
         * @return string[] The list of CSS classes.
         */
        protected function get_table_classes()
        {
        }
        /**
         * Checks if the current user has permissions to perform AJAX actions.
         *
         * @return bool True if the current user has permissions, false otherwise.
         */
        public function ajax_user_can()
        {
        }
        /**
         * Prepares the themes list for display.
         *
         * @global string             $status  The current theme status.
         * @global array<string, int> $totals  An array of theme counts for each status.
         * @global int                $page    The current page number.
         * @global string             $orderby The column to order the themes list by.
         * @global string             $order   The order of the themes list (ASC or DESC).
         * @global string             $s       The search string.
         */
        public function prepare_items()
        {
        }
        /**
         * Filters a theme by the search term.
         *
         * @param WP_Theme $theme The WP_Theme object to check.
         * @return bool True if the theme matches the search term, false otherwise.
         */
        public function _search_callback($theme)
        {
        }
        /**
         * Compares the order of two themes by a specific field.
         *
         * @global string $orderby The column to order the themes list by.
         * @global string $order   The order of the themes list (ASC or DESC).
         *
         * @param WP_Theme $theme_a The first theme to compare.
         * @param WP_Theme $theme_b The second theme to compare.
         * @return int 0 if equal, -1 if the first is less than the second, 1 if more.
         */
        public function _order_callback($theme_a, $theme_b)
        {
        }
        /**
         * Displays the message when there are no items to list.
         */
        public function no_items()
        {
        }
        /**
         * Gets the list of columns for the list table.
         *
         * @return array<string, string> Array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         * Gets the list of sortable columns for the list table.
         *
         * @return array<string, array<int, mixed>> An array of sortable columns.
         */
        protected function get_sortable_columns()
        {
        }
        /**
         * Gets the name of the primary column.
         *
         * @since 4.3.0
         *
         * @return string Unalterable name of the primary column name, in this case, 'name'.
         */
        protected function get_primary_column_name()
        {
        }
        /**
         * Gets the list of views (statuses) for the list table.
         *
         * @global array<string, int> $totals An array of theme counts for each status.
         * @global string             $status The current theme status.
         *
         * @return array<string, string> The list of views.
         */
        protected function get_views()
        {
        }
        /**
         * Gets the list of bulk actions for the list table.
         *
         * @global string $status The current theme status.
         *
         * @return array<string, string> The list of bulk actions.
         */
        protected function get_bulk_actions()
        {
        }
        /**
         * Generates the list table rows.
         *
         * @since 3.1.0
         */
        public function display_rows()
        {
        }
        /**
         * Handles the checkbox column output.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$theme` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_Theme $item The current WP_Theme object.
         */
        public function column_cb($item)
        {
        }
        /**
         * Handles the name column output.
         *
         * @since 4.3.0
         *
         * @global string $status The current theme status.
         * @global int    $page   The current page number.
         * @global string $s      The search string.
         *
         * @param WP_Theme $theme The current WP_Theme object.
         */
        public function column_name($theme)
        {
        }
        /**
         * Handles the description column output.
         *
         * @since 4.3.0
         *
         * @global string             $status The current theme status.
         * @global array<string, int> $totals An array of theme counts for each status.
         *
         * @param WP_Theme $theme The current WP_Theme object.
         */
        public function column_description($theme)
        {
        }
        /**
         * Handles the auto-updates column output.
         *
         * @since 5.5.0
         *
         * @global string $status The current theme status.
         * @global int    $page   The current page number.
         *
         * @param WP_Theme $theme The current WP_Theme object.
         */
        public function column_autoupdates($theme)
        {
        }
        /**
         * Handles default column output.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$theme` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_Theme $item        The current WP_Theme object.
         * @param string   $column_name The current column name.
         */
        public function column_default($item, $column_name)
        {
        }
        /**
         * Handles the output for a single table row.
         *
         * @since 4.3.0
         *
         * @param WP_Theme $item The current WP_Theme object.
         */
        public function single_row_columns($item)
        {
        }
        /**
         * Handles the output for a single table row.
         *
         * @global string             $status The current theme status.
         * @global array<string, int> $totals An array of theme counts for each status.
         *
         * @param WP_Theme $theme The current WP_Theme object.
         */
        public function single_row($theme)
        {
        }
    }
    /**
     * Core class used to implement displaying users in a list table for the network admin.
     *
     * @since 3.1.0
     *
     * @see WP_List_Table
     */
    class WP_MS_Users_List_Table extends \WP_List_Table
    {
        /**
         * @return bool
         */
        public function ajax_user_can()
        {
        }
        /**
         * @global string $mode       List table view mode.
         * @global string $usersearch
         * @global string $role
         */
        public function prepare_items()
        {
        }
        /**
         * @return array
         */
        protected function get_bulk_actions()
        {
        }
        /**
         */
        public function no_items()
        {
        }
        /**
         * @global string $role
         * @return array
         */
        protected function get_views()
        {
        }
        /**
         * @global string $mode List table view mode.
         *
         * @param string $which
         */
        protected function pagination($which)
        {
        }
        /**
         * @return string[] Array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         * @return array
         */
        protected function get_sortable_columns()
        {
        }
        /**
         * Handles the checkbox column output.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$user` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_User $item The current WP_User object.
         * @phpstan-return void
         */
        public function column_cb($item)
        {
        }
        /**
         * Handles the ID column output.
         *
         * @since 4.4.0
         *
         * @param WP_User $user The current WP_User object.
         */
        public function column_id($user)
        {
        }
        /**
         * Handles the username column output.
         *
         * @since 4.3.0
         *
         * @param WP_User $user The current WP_User object.
         */
        public function column_username($user)
        {
        }
        /**
         * Handles the name column output.
         *
         * @since 4.3.0
         *
         * @param WP_User $user The current WP_User object.
         */
        public function column_name($user)
        {
        }
        /**
         * Handles the email column output.
         *
         * @since 4.3.0
         *
         * @param WP_User $user The current WP_User object.
         */
        public function column_email($user)
        {
        }
        /**
         * Handles the registered date column output.
         *
         * @since 4.3.0
         *
         * @global string $mode List table view mode.
         *
         * @param WP_User $user The current WP_User object.
         */
        public function column_registered($user)
        {
        }
        /**
         * @since 4.3.0
         *
         * @param WP_User $user
         * @param string  $classes
         * @param string  $data
         * @param string  $primary
         */
        protected function _column_blogs($user, $classes, $data, $primary)
        {
        }
        /**
         * Handles the sites column output.
         *
         * @since 4.3.0
         *
         * @param WP_User $user The current WP_User object.
         * @phpstan-return void
         */
        public function column_blogs($user)
        {
        }
        /**
         * Handles the default column output.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$user` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_User $item        The current WP_User object.
         * @param string  $column_name The current column name.
         */
        public function column_default($item, $column_name)
        {
        }
        /**
         * Generates the list table rows.
         *
         * @since 3.1.0
         */
        public function display_rows()
        {
        }
        /**
         * Gets the name of the default primary column.
         *
         * @since 4.3.0
         *
         * @return string Name of the default primary column, in this case, 'username'.
         */
        protected function get_default_primary_column_name()
        {
        }
        /**
         * Generates and displays row action links.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$user` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_User $item        User being acted upon.
         * @param string  $column_name Current column name.
         * @param string  $primary     Primary column name.
         * @return string Row actions output for users in Multisite, or an empty string
         *                if the current column is not the primary column.
         */
        protected function handle_row_actions($item, $column_name, $primary)
        {
        }
        /**
         * Returns a clean label for the primary (Username) column's row header `aria-label`.
         *
         * Provides screen readers with just the user login as the row header name,
         * preventing them from computing the name from the full cell content.
         *
         * @since 7.1.0
         *
         * @param WP_User $user The current WP_User object.
         * @return string The user login.
         */
        protected function get_primary_column_aria_label($user)
        {
        }
    }
    /**
     * Core class used to implement displaying plugins to install in a list table.
     *
     * @since 3.1.0
     *
     * @see WP_List_Table
     */
    class WP_Plugin_Install_List_Table extends \WP_List_Table
    {
        public $order = 'ASC';
        public $orderby = \null;
        public $groups = array();
        /**
         * @return bool
         */
        public function ajax_user_can()
        {
        }
        /**
         * Returns the list of known plugins.
         *
         * Uses the transient data from the updates API to determine the known
         * installed plugins.
         *
         * @since 4.9.0
         *
         * @return array
         */
        protected function get_installed_plugins()
        {
        }
        /**
         * Returns a list of slugs of installed plugins, if known.
         *
         * Uses the transient data from the updates API to determine the slugs of
         * known installed plugins. This might be better elsewhere, perhaps even
         * within get_plugins().
         *
         * @since 4.0.0
         *
         * @return array
         */
        protected function get_installed_plugin_slugs()
        {
        }
        /**
         * @global array  $tabs
         * @global string $tab
         * @global int    $paged
         * @global string $type
         * @global string $term
         * @phpstan-return void
         */
        public function prepare_items()
        {
        }
        /**
         */
        public function no_items()
        {
        }
        /**
         * @global array $tabs
         * @global string $tab
         *
         * @return array
         */
        protected function get_views()
        {
        }
        /**
         * Overrides parent views so we can use the filter bar display.
         *
         * @global string $tab The current tab.
         */
        public function views()
        {
        }
        /**
         * Displays the plugin install table.
         *
         * Overrides the parent display() method to provide a different container.
         *
         * @since 4.0.0
         */
        public function display()
        {
        }
        /**
         * @global string $tab
         *
         * @param string $which
         * @phpstan-return void
         */
        protected function display_tablenav($which)
        {
        }
        /**
         * @return array
         */
        protected function get_table_classes()
        {
        }
        /**
         * @return string[] Array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         * Generates the list table rows.
         *
         * @since 3.1.0
         */
        public function display_rows()
        {
        }
        /**
         * Returns a notice containing a list of dependencies required by the plugin.
         *
         * @since 6.5.0
         *
         * @param array  $plugin_data An array of plugin data. See {@see plugins_api()}
         *                            for the list of possible values.
         * @return string A notice containing a list of dependencies required by the plugin,
         *                or an empty string if none is required.
         * @phpstan-param object|array{
         *   slug?: string,
         *   per_page?: int,
         *   page?: int,
         *   number?: int,
         *   search?: string,
         *   tag?: string,
         *   author?: string,
         *   user?: string,
         *   browse?: string,
         *   locale?: string,
         *   installed_plugins?: string,
         *   is_ssl?: bool,
         *   fields?: array{
         *     short_description?: bool,
         *     description?: bool,
         *     sections?: bool,
         *     tested?: bool,
         *     requires?: bool,
         *     requires_php?: bool,
         *     rating?: bool,
         *     ratings?: bool,
         *     downloaded?: bool,
         *     downloadlink?: bool,
         *     last_updated?: bool,
         *     added?: bool,
         *     tags?: bool,
         *     compatibility?: bool,
         *     homepage?: bool,
         *     versions?: bool,
         *     donate_link?: bool,
         *     reviews?: bool,
         *     banners?: bool,
         *     icons?: bool,
         *     active_installs?: bool,
         *     contributors?: bool,
         *   },
         * } $plugin_data See plugins_api()
         */
        protected function get_dependencies_notice($plugin_data)
        {
        }
        /**
         * Creates a 'More details' link for the plugin.
         *
         * @since 6.5.0
         *
         * @param string $name The plugin's name.
         * @param string $slug The plugin's slug.
         * @return string The 'More details' link for the plugin.
         */
        protected function get_more_details_link($name, $slug)
        {
        }
    }
    /**
     * Core class used to implement displaying installed plugins in a list table.
     *
     * @since 3.1.0
     *
     * @see WP_List_Table
     */
    class WP_Plugins_List_Table extends \WP_List_Table
    {
        /**
         * Whether to show the auto-updates UI.
         *
         * @since 5.5.0
         *
         * @var bool True if auto-updates UI is to be shown, false otherwise.
         */
        protected $show_autoupdates = \true;
        /**
         * Constructor.
         *
         * @since 3.2.0
         *
         * @see WP_List_Table::__construct() for more information on default arguments.
         *
         * @global string $status
         * @global int    $page
         *
         * @param array $args An associative array of arguments.
         */
        public function __construct($args = array())
        {
        }
        /**
         * Gets the CSS classes for the list table element.
         *
         * @since 3.1.0
         *
         * @return string[] Array of CSS classes for the table tag.
         */
        protected function get_table_classes()
        {
        }
        /**
         * Checks whether the current user can activate plugins for this screen.
         *
         * @since 3.1.0
         *
         * @return bool Whether the current user can activate plugins.
         */
        public function ajax_user_can()
        {
        }
        /**
         * Prepares the list of items for displaying.
         *
         * @since 3.1.0
         *
         * @global string                                             $status  Current plugin status filter slug.
         * @global array<string, array<string, array<string, mixed>>> $plugins Array of plugin data arrays grouped by status.
         * @global array<string, int>                                 $totals  Count of plugins for each status group.
         * @global int                                                $page    Current page number.
         * @global string                                             $orderby Column name to sort by.
         * @global string                                             $order   Sort direction, 'ASC' or 'DESC'.
         * @global string                                             $s       URL-encoded search term.
         */
        public function prepare_items()
        {
        }
        /**
         * Callback to filter plugins by a search term.
         *
         * @since 3.1.0
         *
         * @global string $s URL encoded search term.
         *
         * @param array<string, mixed> $plugin Plugin data array to check against the search term.
         * @return bool True if the plugin matches the search term, false otherwise.
         */
        public function _search_callback($plugin)
        {
        }
        /**
         * Callback to sort plugins by a given column.
         *
         * @since 3.1.0
         *
         * @global string $orderby The column name to sort by.
         * @global string $order   The sort direction ('ASC' or 'DESC').
         *
         * @param array<string, mixed> $plugin_a First plugin data array to compare.
         * @param array<string, mixed> $plugin_b Second plugin data array to compare.
         * @return int Negative if $plugin_a sorts before $plugin_b, positive if after, 0 if equal.
         */
        public function _order_callback($plugin_a, $plugin_b)
        {
        }
        /**
         * Message to be displayed when there are no items.
         *
         * @since 3.1.0
         *
         * @global array<string, array<string, array<string, mixed>>> $plugins Array of plugin data arrays grouped by status.
         */
        public function no_items()
        {
        }
        /**
         * Displays the search box.
         *
         * @since 4.6.0
         *
         * @param string $text     The 'submit' button label.
         * @param string $input_id ID attribute value for the search input field.
         * @phpstan-return void
         */
        public function search_box($text, $input_id)
        {
        }
        /**
         * Gets the list of columns for this list table.
         *
         * @since 3.1.0
         *
         * @global string $status Current plugin status filter slug.
         *
         * @return array<string, string> An associative array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         * Gets the list of sortable columns for this list table.
         *
         * @since 3.1.0
         *
         * @return array<string, array<int, string|bool>|string> An associative array of sortable columns.
         */
        protected function get_sortable_columns()
        {
        }
        /**
         * Gets an associative array of status filter links for the views area.
         *
         * @since 3.1.0
         *
         * @global array<string, int> $totals Count of plugins for each status group.
         * @global string             $status Current plugin status filter slug.
         *
         * @return array<string, string> An associative array of views.
         */
        protected function get_views()
        {
        }
        /**
         * Gets the available bulk actions for the plugins list table.
         *
         * @since 3.1.0
         *
         * @global string $status Current plugin status filter slug.
         *
         * @return array<string, string> An associative array of bulk actions.
         */
        protected function get_bulk_actions()
        {
        }
        /**
         * Displays the bulk actions dropdown.
         *
         * @since 3.1.0
         *
         * @global string $status Current plugin status filter slug.
         *
         * @param string $which The location of the bulk actions: Either 'top' or 'bottom'.
         *                      This is designated as optional for backward compatibility.
         * @phpstan-param 'top'|'bottom' $which
         * @phpstan-return void
         */
        public function bulk_actions($which = '')
        {
        }
        /**
         * Displays extra table navigation for the plugins list table.
         *
         * @since 3.1.0
         *
         * @global string $status Current plugin status filter slug.
         *
         * @param string $which The location: 'top' or 'bottom'.
         * @phpstan-param 'top'|'bottom' $which
         * @phpstan-return void
         */
        protected function extra_tablenav($which)
        {
        }
        /**
         * Gets the current action selected from the bulk actions dropdown.
         *
         * Also handles the 'clear-recent-list' action from the Recently Active plugins screen.
         *
         * @since 3.1.0
         *
         * @return string|false The action name. False if no action was selected.
         */
        public function current_action()
        {
        }
        /**
         * Generates the list table rows.
         *
         * @since 3.1.0
         *
         * @global string $status Current plugin status filter slug.
         * @phpstan-return void
         */
        public function display_rows()
        {
        }
        /**
         * Generates the markup for a single plugin row.
         *
         * @since 3.1.0
         *
         * @global string             $status Current plugin status filter slug.
         * @global int                $page   Current page number.
         * @global string             $s      URL-encoded search term.
         * @global array<string, int> $totals Count of plugins for each status group.
         *
         * @param array $item The current item. An array containing the plugin file path and plugin data.
         * @phpstan-param array{string, array<string, mixed>} $item
         */
        public function single_row($item)
        {
        }
        /**
         * Gets the name of the primary column for this specific list table.
         *
         * @since 4.3.0
         *
         * @return string Unalterable name for the primary column, in this case, 'name'.
         */
        protected function get_primary_column_name()
        {
        }
        /**
         * Prints a list of other plugins that depend on the plugin.
         *
         * @since 6.5.0
         *
         * @param string $dependency The dependency's filepath, relative to the plugins directory.
         * @phpstan-return void
         */
        protected function add_dependents_to_dependency_plugin_row($dependency)
        {
        }
        /**
         * Prints a list of other plugins that the plugin depends on.
         *
         * @since 6.5.0
         *
         * @param string $dependent The dependent plugin's filepath, relative to the plugins directory.
         * @phpstan-return void
         */
        protected function add_dependencies_to_dependent_plugin_row($dependent)
        {
        }
        /**
         * Returns a 'View details' like link for a dependency.
         *
         * @since 6.5.0
         *
         * @param string $name The dependency's name.
         * @param string $slug The dependency's slug.
         * @return string A 'View details' link for the dependency.
         */
        protected function get_dependency_view_details_link($name, $slug)
        {
        }
        /**
         * Returns a 'View details' link for the plugin.
         *
         * @since 6.5.0
         *
         * @param string $name The plugin's name.
         * @param string $slug The plugin's slug.
         * @return string A 'View details' link for the plugin.
         */
        protected function get_view_details_link($name, $slug)
        {
        }
    }
    /**
     * Core class used to implement displaying post comments in a list table.
     *
     * @since 3.1.0
     *
     * @see WP_Comments_List_Table
     */
    class WP_Post_Comments_List_Table extends \WP_Comments_List_Table
    {
        /**
         * @return array
         */
        protected function get_column_info()
        {
        }
        /**
         * @return array
         */
        protected function get_table_classes()
        {
        }
        /**
         * @param bool $output_empty
         */
        public function display($output_empty = \false)
        {
        }
        /**
         * @param bool $comment_status
         * @return int
         */
        public function get_per_page($comment_status = \false)
        {
        }
    }
    /**
     * Core class used to implement displaying posts in a list table.
     *
     * @since 3.1.0
     *
     * @see WP_List_Table
     */
    class WP_Posts_List_Table extends \WP_List_Table
    {
        /**
         * Whether the items should be displayed hierarchically or linearly.
         *
         * @since 3.1.0
         * @var bool
         */
        protected $hierarchical_display;
        /**
         * Holds the number of pending comments for each post.
         *
         * @since 3.1.0
         * @var array
         */
        protected $comment_pending_count;
        /**
         * Current level for output.
         *
         * @since 4.3.0
         * @var int
         */
        protected $current_level = 0;
        /**
         * Constructor.
         *
         * @since 3.1.0
         *
         * @see WP_List_Table::__construct() for more information on default arguments.
         *
         * @global WP_Post_Type $post_type_object Global post type object.
         * @global wpdb         $wpdb             WordPress database abstraction object.
         *
         * @param array $args An associative array of arguments.
         */
        public function __construct($args = array())
        {
        }
        /**
         * Sets whether the table layout should be hierarchical or not.
         *
         * @since 4.2.0
         *
         * @param bool $display Whether the table layout should be hierarchical.
         */
        public function set_hierarchical_display($display)
        {
        }
        /**
         * @return bool
         */
        public function ajax_user_can()
        {
        }
        /**
         * @global string   $mode             List table view mode.
         * @global array    $avail_post_stati
         * @global WP_Query $wp_query         WordPress Query object.
         * @global int      $per_page
         */
        public function prepare_items()
        {
        }
        /**
         * @return bool
         */
        public function has_items()
        {
        }
        /**
         */
        public function no_items()
        {
        }
        /**
         * Determines if the current view is the "All" view.
         *
         * @since 4.2.0
         *
         * @return bool Whether the current view is the "All" view.
         */
        protected function is_base_request()
        {
        }
        /**
         * Creates a link to edit.php with params.
         *
         * @since 4.4.0
         *
         * @param string[] $args      Associative array of URL parameters for the link.
         * @param string   $link_text Link text.
         * @param string   $css_class Optional. Class attribute. Default empty string.
         * @return string The formatted link string.
         */
        protected function get_edit_link($args, $link_text, $css_class = '')
        {
        }
        /**
         * @global array $locked_post_status This seems to be deprecated.
         * @global array $avail_post_stati
         * @return array
         */
        protected function get_views()
        {
        }
        /**
         * @return array
         */
        protected function get_bulk_actions()
        {
        }
        /**
         * Displays a categories drop-down for filtering on the Posts list table.
         *
         * @since 4.6.0
         *
         * @global int $cat Currently selected category.
         *
         * @param string $post_type Post type slug.
         * @phpstan-return void
         */
        protected function categories_dropdown($post_type)
        {
        }
        /**
         * Displays a formats drop-down for filtering items.
         *
         * @since 5.2.0
         *
         * @param string $post_type Post type slug.
         * @phpstan-return void
         */
        protected function formats_dropdown($post_type)
        {
        }
        /**
         * @param string $which
         */
        protected function extra_tablenav($which)
        {
        }
        /**
         * @return string
         */
        public function current_action()
        {
        }
        /**
         * @global string $mode List table view mode.
         *
         * @return array
         */
        protected function get_table_classes()
        {
        }
        /**
         * @return string[] Array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         * @return array
         */
        protected function get_sortable_columns()
        {
        }
        /**
         * Generates the list table rows.
         *
         * @since 3.1.0
         *
         * @global WP_Query $wp_query WordPress Query object.
         * @global int      $per_page
         *
         * @param array $posts
         * @param int   $level
         */
        public function display_rows($posts = array(), $level = 0)
        {
        }
        /**
         * Gets a trimmed excerpt to display in place of a missing post title.
         *
         * Only returns text in the Compact list view for posts that have no title,
         * do not require a password, and that the current user is allowed to read.
         *
         * @since 7.1.0
         *
         * @global string $mode List table view mode.
         *
         * @param WP_Post $post The current WP_Post object.
         * @return string The escaped, trimmed excerpt, or an empty string.
         */
        protected function get_no_title_excerpt($post)
        {
        }
        /**
         * Handles the checkbox column output.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$post` to `$item` to match parent class for PHP 8 named parameter support.
         * @since 7.1.0 Includes a trimmed excerpt for untitled posts in Compact view.
         *
         * @param WP_Post $item The current WP_Post object.
         */
        public function column_cb($item)
        {
        }
        /**
         * @since 4.3.0
         *
         * @param WP_Post $post
         * @param string  $classes
         * @param string  $data
         * @param string  $primary
         */
        protected function _column_title($post, $classes, $data, $primary)
        {
        }
        /**
         * Returns a clean label for the primary (title) column's row header `aria-label`.
         *
         * Provides screen readers with just the post title as the row header name,
         * preventing them from computing the name from the full cell content
         * (which includes row action links, post states, and possibly an excerpt).
         *
         * @since 7.1.0
         *
         * @param WP_Post $item The current post object.
         * @return string The post title, or 'no title' if no title.
         */
        protected function get_primary_column_aria_label($item)
        {
        }
        /**
         * Handles the title column output.
         *
         * @since 4.3.0
         * @since 7.1.0 Includes a trimmed excerpt for untitled posts in Compact view.
         *
         * @global string $mode List table view mode.
         *
         * @param WP_Post $post The current WP_Post object.
         */
        public function column_title($post)
        {
        }
        /**
         * Handles the post date column output.
         *
         * @since 4.3.0
         *
         * @global string $mode List table view mode.
         *
         * @param WP_Post $post The current WP_Post object.
         */
        public function column_date($post)
        {
        }
        /**
         * Handles the comments column output.
         *
         * @since 4.3.0
         *
         * @param WP_Post $post The current WP_Post object.
         */
        public function column_comments($post)
        {
        }
        /**
         * Handles the post author column output.
         *
         * @since 4.3.0
         * @since 6.8.0 Added fallback text when author's name is unknown.
         *
         * @param WP_Post $post The current WP_Post object.
         */
        public function column_author($post)
        {
        }
        /**
         * Handles the default column output.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$post` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_Post $item        The current WP_Post object.
         * @param string  $column_name The current column name.
         * @phpstan-return void
         */
        public function column_default($item, $column_name)
        {
        }
        /**
         * @global WP_Post $post Global post object.
         *
         * @param int|WP_Post $post
         * @param int         $level
         */
        public function single_row($post, $level = 0)
        {
        }
        /**
         * Gets the name of the default primary column.
         *
         * @since 4.3.0
         *
         * @return string Name of the default primary column, in this case, 'title'.
         */
        protected function get_default_primary_column_name()
        {
        }
        /**
         * Generates and displays row action links.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$post` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_Post $item        Post being acted upon.
         * @param string  $column_name Current column name.
         * @param string  $primary     Primary column name.
         * @return string Row actions output for posts, or an empty string
         *                if the current column is not the primary column.
         */
        protected function handle_row_actions($item, $column_name, $primary)
        {
        }
        /**
         * Outputs the hidden row displayed when inline editing
         *
         * @since 3.1.0
         *
         * @global string $mode List table view mode.
         */
        public function inline_edit()
        {
        }
    }
    /**
     * List Table API: WP_Privacy_Requests_Table class
     *
     * @package WordPress
     * @subpackage Administration
     * @since 4.9.6
     */
    abstract class WP_Privacy_Requests_Table extends \WP_List_Table
    {
        /**
         * Action name for the requests this table will work with. Classes
         * which inherit from WP_Privacy_Requests_Table should define this.
         *
         * Example: 'export_personal_data'.
         *
         * @since 4.9.6
         *
         * @var string $request_type Name of action.
         */
        protected $request_type = 'INVALID';
        /**
         * Post type to be used.
         *
         * @since 4.9.6
         *
         * @var string $post_type The post type.
         */
        protected $post_type = 'INVALID';
        /**
         * Gets columns to show in the list table.
         *
         * @since 4.9.6
         *
         * @return string[] Array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         * Normalizes the admin URL to the current page (by request_type).
         *
         * @since 5.3.0
         *
         * @return string URL to the current admin page.
         */
        protected function get_admin_url()
        {
        }
        /**
         * Gets a list of sortable columns.
         *
         * @since 4.9.6
         *
         * @return array Default sortable columns.
         */
        protected function get_sortable_columns()
        {
        }
        /**
         * Returns the default primary column.
         *
         * @since 4.9.6
         *
         * @return string Default primary column name.
         */
        protected function get_default_primary_column_name()
        {
        }
        /**
         * Counts the number of requests for each status.
         *
         * @since 4.9.6
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @return object Number of posts for each status.
         */
        protected function get_request_counts()
        {
        }
        /**
         * Gets an associative array ( id => link ) with the list of views available on this table.
         *
         * @since 4.9.6
         *
         * @return string[] An array of HTML links keyed by their view.
         */
        protected function get_views()
        {
        }
        /**
         * Gets bulk actions.
         *
         * @since 4.9.6
         *
         * @return array Array of bulk action labels keyed by their action.
         */
        protected function get_bulk_actions()
        {
        }
        /**
         * Process bulk actions.
         *
         * @since 4.9.6
         * @since 5.6.0 Added support for the `complete` action.
         * @phpstan-return void
         */
        public function process_bulk_action()
        {
        }
        /**
         * Prepares items to output.
         *
         * @since 4.9.6
         * @since 5.1.0 Added support for column sorting.
         */
        public function prepare_items()
        {
        }
        /**
         * Returns the markup for the Checkbox column.
         *
         * @since 4.9.6
         *
         * @param WP_User_Request $item Item being shown.
         * @return string Checkbox column markup.
         */
        public function column_cb($item)
        {
        }
        /**
         * Status column.
         *
         * @since 4.9.6
         *
         * @param WP_User_Request $item Item being shown.
         * @return string|null Status column markup. Returns a string if no status is found,
         *                     otherwise it displays the markup.
         */
        public function column_status($item)
        {
        }
        /**
         * Converts a timestamp for display.
         *
         * @since 4.9.6
         *
         * @param int $timestamp Event timestamp.
         * @return string Human readable date.
         */
        protected function get_timestamp_as_date($timestamp)
        {
        }
        /**
         * Handles the default column.
         *
         * @since 4.9.6
         * @since 5.7.0 Added `manage_{$this->screen->id}_custom_column` action.
         *
         * @param WP_User_Request $item        Item being shown.
         * @param string          $column_name Name of column being shown.
         */
        public function column_default($item, $column_name)
        {
        }
        /**
         * Returns the markup for the Created timestamp column. Overridden by children.
         *
         * @since 5.7.0
         *
         * @param WP_User_Request $item Item being shown.
         * @return string Human readable date.
         */
        public function column_created_timestamp($item)
        {
        }
        /**
         * Actions column. Overridden by children.
         *
         * @since 4.9.6
         *
         * @param WP_User_Request $item Item being shown.
         * @return string Email column markup.
         */
        public function column_email($item)
        {
        }
        /**
         * Returns the markup for the next steps column. Overridden by children.
         *
         * @since 4.9.6
         *
         * @param WP_User_Request $item Item being shown.
         */
        public function column_next_steps($item)
        {
        }
        /**
         * Generates content for a single row of the table,
         *
         * @since 4.9.6
         *
         * @param WP_User_Request $item The current item.
         */
        public function single_row($item)
        {
        }
        /**
         * Embeds scripts used to perform actions. Overridden by children.
         *
         * @since 4.9.6
         */
        public function embed_scripts()
        {
        }
    }
    /**
     * WP_Privacy_Data_Export_Requests_Table class.
     *
     * @since 4.9.6
     */
    class WP_Privacy_Data_Export_Requests_List_Table extends \WP_Privacy_Requests_Table
    {
        /**
         * Action name for the requests this table will work with.
         *
         * @since 4.9.6
         *
         * @var string $request_type Name of action.
         */
        protected $request_type = 'export_personal_data';
        /**
         * Post type for the requests.
         *
         * @since 4.9.6
         *
         * @var string $post_type The post type.
         */
        protected $post_type = 'user_request';
        /**
         * Actions column.
         *
         * @since 4.9.6
         *
         * @param WP_User_Request $item Item being shown.
         * @return string Email column markup.
         */
        public function column_email($item)
        {
        }
        /**
         * Displays the next steps column.
         *
         * @since 4.9.6
         *
         * @param WP_User_Request $item Item being shown.
         */
        public function column_next_steps($item)
        {
        }
        /**
         * Returns a clean label for the primary (Requester) column's row header `aria-label`.
         *
         * Provides screen readers with just the item email as the row header name,
         * preventing them from computing the name from the full cell content.
         *
         * @since 7.1.0
         *
         * @param WP_User_Request $item Item being shown.
         * @return string The user request item email.
         */
        protected function get_primary_column_aria_label($item)
        {
        }
    }
    /**
     * WP_Privacy_Data_Removal_Requests_List_Table class.
     *
     * @since 4.9.6
     */
    class WP_Privacy_Data_Removal_Requests_List_Table extends \WP_Privacy_Requests_Table
    {
        /**
         * Action name for the requests this table will work with.
         *
         * @since 4.9.6
         *
         * @var string $request_type Name of action.
         */
        protected $request_type = 'remove_personal_data';
        /**
         * Post type for the requests.
         *
         * @since 4.9.6
         *
         * @var string $post_type The post type.
         */
        protected $post_type = 'user_request';
        /**
         * Outputs the Actions column.
         *
         * @since 4.9.6
         *
         * @param WP_User_Request $item Item being shown.
         * @return string Email column markup.
         */
        public function column_email($item)
        {
        }
        /**
         * Outputs the Next steps column.
         *
         * @since 4.9.6
         *
         * @param WP_User_Request $item Item being shown.
         */
        public function column_next_steps($item)
        {
        }
        /**
         * Returns a clean label for the primary (Requester) column's row header `aria-label`.
         *
         * Provides screen readers with just the item email as the row header name,
         * preventing them from computing the name from the full cell content.
         *
         * @since 7.1.0
         *
         * @param WP_User_Request $item Item being shown.
         * @return string The user request item email.
         */
        protected function get_primary_column_aria_label($item)
        {
        }
    }
    /**
     * Core class used to implement an admin screen API.
     *
     * @since 3.3.0
     */
    #[\AllowDynamicProperties]
    final class WP_Screen
    {
        /**
         * Any action associated with the screen.
         *
         * 'add' for *-add.php and *-new.php screens. Empty otherwise.
         *
         * @since 3.3.0
         * @var string
         */
        public $action;
        /**
         * The base type of the screen.
         *
         * This is typically the same as `$id` but with any post types and taxonomies stripped.
         * For example, for an `$id` of 'edit-post' the base is 'edit'.
         *
         * @since 3.3.0
         * @var string
         */
        public $base;
        /**
         * The unique ID of the screen.
         *
         * @since 3.3.0
         * @var string
         */
        public $id;
        /**
         * Whether the screen is in the network admin.
         *
         * Deprecated. Use in_admin() instead.
         *
         * @since 3.3.0
         * @deprecated 3.5.0
         * @var bool
         */
        public $is_network;
        /**
         * Whether the screen is in the user admin.
         *
         * Deprecated. Use in_admin() instead.
         *
         * @since 3.3.0
         * @deprecated 3.5.0
         * @var bool
         */
        public $is_user;
        /**
         * The base menu parent.
         *
         * This is derived from `$parent_file` by removing the query string and any .php extension.
         * `$parent_file` values of 'edit.php?post_type=page' and 'edit.php?post_type=post'
         * have a `$parent_base` of 'edit'.
         *
         * @since 3.3.0
         * @var ?string
         */
        public $parent_base;
        /**
         * The parent_file for the screen per the admin menu system.
         *
         * Some `$parent_file` values are 'edit.php?post_type=page', 'edit.php', and 'options-general.php'.
         *
         * @since 3.3.0
         * @var ?string
         */
        public $parent_file;
        /**
         * The post type associated with the screen, if any.
         *
         * The 'edit.php?post_type=page' screen has a post type of 'page'.
         * The 'edit-tags.php?taxonomy=$taxonomy&post_type=page' screen has a post type of 'page'.
         *
         * @since 3.3.0
         * @var string
         */
        public $post_type;
        /**
         * The taxonomy associated with the screen, if any.
         *
         * The 'edit-tags.php?taxonomy=category' screen has a taxonomy of 'category'.
         *
         * @since 3.3.0
         * @var string
         */
        public $taxonomy;
        /**
         * Whether the screen is using the block editor.
         *
         * @since 5.0.0
         * @var bool
         */
        public $is_block_editor = \false;
        /**
         * Fetches a screen object.
         *
         * @since 3.3.0
         *
         * @global string $hook_suffix
         *
         * @param string|WP_Screen $hook_name Optional. The hook name (also known as the hook suffix) used to determine the screen.
         *                                    Defaults to the current $hook_suffix global.
         * @return WP_Screen Screen object.
         */
        public static function get($hook_name = '')
        {
        }
        /**
         * Makes the screen object the current screen.
         *
         * @see set_current_screen()
         * @since 3.3.0
         *
         * @global WP_Screen $current_screen WordPress current screen object.
         * @global string    $typenow        The post type of the current screen.
         * @global string    $taxnow         The taxonomy of the current screen.
         */
        public function set_current_screen()
        {
        }
        /**
         * Indicates whether the screen is in a particular admin.
         *
         * @since 3.5.0
         *
         * @param string $admin The admin to check against (network | user | site).
         *                      If empty any of the three admins will result in true.
         * @return bool True if the screen is in the indicated admin, false otherwise.
         */
        public function in_admin($admin = \null)
        {
        }
        /**
         * Sets or returns whether the block editor is loading on the current screen.
         *
         * @since 5.0.0
         *
         * @param bool $set Optional. Sets whether the block editor is loading on the current screen or not.
         * @return bool True if the block editor is being loaded, false otherwise.
         */
        public function is_block_editor($set = \null)
        {
        }
        /**
         * Sets the old string-based contextual help for the screen for backward compatibility.
         *
         * @since 3.3.0
         *
         * @param WP_Screen $screen A screen object.
         * @param string    $help   Help text.
         */
        public static function add_old_compat_help($screen, $help)
        {
        }
        /**
         * Sets the parent information for the screen.
         *
         * This is called in admin-header.php after the menu parent for the screen has been determined.
         *
         * @since 3.3.0
         *
         * @param string $parent_file The parent file of the screen. Typically the $parent_file global.
         */
        public function set_parentage($parent_file)
        {
        }
        /**
         * Adds an option for the screen.
         *
         * Call this in template files after admin.php is loaded and before admin-header.php is loaded
         * to add screen options.
         *
         * @since 3.3.0
         *
         * @param string $option Option ID.
         * @param mixed  $args   Option-dependent arguments.
         */
        public function add_option($option, $args = array())
        {
        }
        /**
         * Removes an option from the screen.
         *
         * @since 3.8.0
         *
         * @param string $option Option ID.
         */
        public function remove_option($option)
        {
        }
        /**
         * Removes all options from the screen.
         *
         * @since 3.8.0
         */
        public function remove_options()
        {
        }
        /**
         * Gets the options registered for the screen.
         *
         * @since 3.8.0
         *
         * @return array Options with arguments.
         */
        public function get_options()
        {
        }
        /**
         * Gets the arguments for an option for the screen.
         *
         * @since 3.3.0
         *
         * @param string       $option Option name.
         * @param string|false $key    Optional. Specific array key for when the option is an array.
         *                             Default false.
         * @return ?string The option value if set, null otherwise.
         */
        public function get_option($option, $key = \false)
        {
        }
        /**
         * Gets the help tabs registered for the screen.
         *
         * @since 3.4.0
         * @since 4.4.0 Help tabs are ordered by their priority.
         *
         * @return array Help tabs with arguments.
         */
        public function get_help_tabs()
        {
        }
        /**
         * Gets the arguments for a help tab.
         *
         * @since 3.4.0
         *
         * @param string $id Help Tab ID.
         * @return ?array Help tab arguments, or null if no help tabs added.
         */
        public function get_help_tab($id)
        {
        }
        /**
         * Adds a help tab to the contextual help for the screen.
         *
         * Call this on the `load-$pagenow` hook for the relevant screen,
         * or fetch the `$current_screen` object, or use get_current_screen()
         * and then call the method from the object.
         *
         * You may need to filter `$current_screen` using an if or switch statement
         * to prevent new help tabs from being added to ALL admin screens.
         *
         * @since 3.3.0
         * @since 4.4.0 The `$priority` argument was added.
         *
         * @param array $args {
         *     Array of arguments used to display the help tab.
         *
         *     @type string   $title    Title for the tab. Default false.
         *     @type string   $id       Tab ID. Must be HTML-safe and should be unique for this menu.
         *                              It is NOT allowed to contain any empty spaces. Default false.
         *     @type string   $content  Optional. Help tab content in plain text or HTML. Default empty string.
         *     @type callable $callback Optional. A callback to generate the tab content. Default false.
         *     @type int      $priority Optional. The priority of the tab, used for ordering. Default 10.
         * }
         * @phpstan-param array{
         *   title?: string,
         *   id?: string,
         *   content?: string,
         *   callback?: callable,
         *   priority?: int,
         * } $args
         * @phpstan-return void
         */
        public function add_help_tab($args)
        {
        }
        /**
         * Removes a help tab from the contextual help for the screen.
         *
         * @since 3.3.0
         *
         * @param string $id The help tab ID.
         */
        public function remove_help_tab($id)
        {
        }
        /**
         * Removes all help tabs from the contextual help for the screen.
         *
         * @since 3.3.0
         */
        public function remove_help_tabs()
        {
        }
        /**
         * Gets the content from a contextual help sidebar.
         *
         * @since 3.4.0
         *
         * @return string Contents of the help sidebar.
         */
        public function get_help_sidebar()
        {
        }
        /**
         * Adds a sidebar to the contextual help for the screen.
         *
         * Call this in template files after admin.php is loaded and before admin-header.php is loaded
         * to add a sidebar to the contextual help.
         *
         * @since 3.3.0
         *
         * @param string $content Sidebar content in plain text or HTML.
         */
        public function set_help_sidebar($content)
        {
        }
        /**
         * Gets the number of layout columns the user has selected.
         *
         * The layout_columns option controls the max number and default number of
         * columns. This method returns the number of columns within that range selected
         * by the user via Screen Options. If no selection has been made, the default
         * provisioned in layout_columns is returned. If the screen does not support
         * selecting the number of layout columns, 0 is returned.
         *
         * @since 3.4.0
         *
         * @return int Number of columns to display.
         */
        public function get_columns()
        {
        }
        /**
         * Gets the accessible hidden headings and text used in the screen.
         *
         * @since 4.4.0
         *
         * @see set_screen_reader_content() For more information on the array format.
         *
         * @return string[] An associative array of screen reader text strings.
         */
        public function get_screen_reader_content()
        {
        }
        /**
         * Gets a screen reader text string.
         *
         * @since 4.4.0
         *
         * @param string $key Screen reader text array named key.
         * @return ?string Screen reader text string, or null if no text is associated with the key.
         */
        public function get_screen_reader_text($key)
        {
        }
        /**
         * Adds accessible hidden headings and text for the screen.
         *
         * @since 4.4.0
         *
         * @param array $content {
         *     An associative array of screen reader text strings.
         *
         *     @type string $heading_views      Screen reader text for the filter links heading.
         *                                      Default 'Filter items list'.
         *     @type string $heading_pagination Screen reader text for the pagination heading.
         *                                      Default 'Items list navigation'.
         *     @type string $heading_list       Screen reader text for the items list heading.
         *                                      Default 'Items list'.
         * }
         * @phpstan-param array{
         *   heading_views?: string,
         *   heading_pagination?: string,
         *   heading_list?: string,
         * } $content
         */
        public function set_screen_reader_content($content = array())
        {
        }
        /**
         * Removes all the accessible hidden headings and text for the screen.
         *
         * @since 4.4.0
         */
        public function remove_screen_reader_content()
        {
        }
        /**
         * Renders the screen's help section.
         *
         * This will trigger the deprecated filters for backward compatibility.
         *
         * @since 3.3.0
         *
         * @global string $screen_layout_columns
         * @phpstan-return void
         */
        public function render_screen_meta()
        {
        }
        /**
         * @since 3.3.0
         *
         * @global array $wp_meta_boxes Global meta box state.
         *
         * @return bool Whether to show the Screen Options tab for the current screen.
         */
        public function show_screen_options()
        {
        }
        /**
         * Renders the screen options tab.
         *
         * @since 3.3.0
         *
         * @param array $options {
         *     Options for the tab.
         *
         *     @type bool $wrap Whether the screen-options-wrap div will be included. Defaults to true.
         * }
         * @phpstan-param array{
         *   wrap?: bool,
         * } $options
         */
        public function render_screen_options($options = array())
        {
        }
        /**
         * Renders the meta boxes preferences.
         *
         * @since 4.4.0
         *
         * @global array $wp_meta_boxes Global meta box state.
         * @phpstan-return void
         */
        public function render_meta_boxes_preferences()
        {
        }
        /**
         * Renders the list table columns preferences.
         *
         * @since 4.4.0
         * @phpstan-return void
         */
        public function render_list_table_columns_preferences()
        {
        }
        /**
         * Renders the option for number of columns on the page.
         *
         * @since 3.3.0
         * @phpstan-return void
         */
        public function render_screen_layout()
        {
        }
        /**
         * Renders the items per page option.
         *
         * @since 3.3.0
         * @phpstan-return void
         */
        public function render_per_page_options()
        {
        }
        /**
         * Renders the list table view mode preferences.
         *
         * @since 4.4.0
         *
         * @global string $mode List table view mode.
         * @phpstan-return void
         */
        public function render_view_mode()
        {
        }
        /**
         * Renders screen reader text.
         *
         * @since 4.4.0
         *
         * @param string $key The screen reader text array named key.
         * @param string $tag Optional. The HTML tag to wrap the screen reader text. Default h2.
         * @phpstan-return void
         */
        public function render_screen_reader_content($key = '', $tag = 'h2')
        {
        }
    }
    /**
     * Core class used to implement displaying terms in a list table.
     *
     * @since 3.1.0
     *
     * @see WP_List_Table
     */
    class WP_Terms_List_Table extends \WP_List_Table
    {
        public $callback_args;
        /**
         * Constructor.
         *
         * @since 3.1.0
         *
         * @see WP_List_Table::__construct() for more information on default arguments.
         *
         * @global string      $post_type Global post type.
         * @global string      $taxonomy  Global taxonomy.
         * @global string      $action
         * @global WP_Taxonomy $tax       Global taxonomy object.
         *
         * @param array $args An associative array of arguments.
         */
        public function __construct($args = array())
        {
        }
        /**
         * @return bool
         */
        public function ajax_user_can()
        {
        }
        /**
         */
        public function prepare_items()
        {
        }
        /**
         */
        public function no_items()
        {
        }
        /**
         * @return array
         */
        protected function get_bulk_actions()
        {
        }
        /**
         * @return string
         */
        public function current_action()
        {
        }
        /**
         * @return string[] Array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         * @return array
         */
        protected function get_sortable_columns()
        {
        }
        /**
         * @phpstan-return void
         */
        public function display_rows_or_placeholder()
        {
        }
        /**
         * @global string $taxonomy Global taxonomy.
         *
         * @param WP_Term $tag   Term object.
         * @param int     $level
         */
        public function single_row($tag, $level = 0)
        {
        }
        /**
         * @since 5.9.0 Renamed `$tag` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_Term $item Term object.
         * @return string
         */
        public function column_cb($item)
        {
        }
        /**
         * @param WP_Term $tag Term object.
         * @return string
         */
        public function column_name($tag)
        {
        }
        /**
         * Gets the name of the default primary column.
         *
         * @since 4.3.0
         *
         * @return string Name of the default primary column, in this case, 'name'.
         */
        protected function get_default_primary_column_name()
        {
        }
        /**
         * Generates and displays row action links.
         *
         * @since 4.3.0
         * @since 5.9.0 Renamed `$tag` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_Term $item        Tag being acted upon.
         * @param string  $column_name Current column name.
         * @param string  $primary     Primary column name.
         * @return string Row actions output for terms, or an empty string
         *                if the current column is not the primary column.
         */
        protected function handle_row_actions($item, $column_name, $primary)
        {
        }
        /**
         * @param WP_Term $tag Term object.
         * @return string
         */
        public function column_description($tag)
        {
        }
        /**
         * @param WP_Term $tag Term object.
         * @return string
         */
        public function column_slug($tag)
        {
        }
        /**
         * @param WP_Term $tag Term object.
         * @return string
         */
        public function column_posts($tag)
        {
        }
        /**
         * @param WP_Term $tag Term object.
         * @return string
         */
        public function column_links($tag)
        {
        }
        /**
         * @since 5.9.0 Renamed `$tag` to `$item` to match parent class for PHP 8 named parameter support.
         *
         * @param WP_Term $item        Term object.
         * @param string  $column_name Name of the column.
         * @return string
         */
        public function column_default($item, $column_name)
        {
        }
        /**
         * Outputs the hidden row displayed when inline editing
         *
         * @since 3.1.0
         * @phpstan-return void
         */
        public function inline_edit()
        {
        }
        /**
         * Returns a clean label for the primary (Name) column's row header `aria-label`.
         *
         * Provides screen readers with just the term name as the row header name,
         * preventing them from computing the name from the full cell content.
         *
         * @since 7.1.0
         *
         * @param WP_Term $term Term object.
         * @return string The term name.
         */
        protected function get_primary_column_aria_label($term)
        {
        }
    }
    /**
     * Core class used to implement displaying installed themes in a list table.
     *
     * @since 3.1.0
     *
     * @see WP_List_Table
     */
    class WP_Themes_List_Table extends \WP_List_Table
    {
        protected $search_terms = array();
        public $features = array();
        /**
         * Constructor.
         *
         * @since 3.1.0
         *
         * @see WP_List_Table::__construct() for more information on default arguments.
         *
         * @param array $args An associative array of arguments.
         */
        public function __construct($args = array())
        {
        }
        /**
         * @return bool
         */
        public function ajax_user_can()
        {
        }
        /**
         */
        public function prepare_items()
        {
        }
        /**
         * @phpstan-return void
         */
        public function no_items()
        {
        }
        /**
         * @param string $which
         * @phpstan-return void
         */
        public function tablenav($which = 'top')
        {
        }
        /**
         * Displays the themes table.
         *
         * Overrides the parent display() method to provide a different container.
         *
         * @since 3.1.0
         */
        public function display()
        {
        }
        /**
         * @return string[] Array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         */
        public function display_rows_or_placeholder()
        {
        }
        /**
         * Generates the list table rows.
         *
         * @since 3.1.0
         */
        public function display_rows()
        {
        }
        /**
         * @param WP_Theme $theme
         * @return bool
         */
        public function search_theme($theme)
        {
        }
        /**
         * Send required variables to JavaScript land
         *
         * @since 3.4.0
         *
         * @param array $extra_args
         */
        public function _js_vars($extra_args = array())
        {
        }
    }
    /**
     * Core class used to implement displaying themes to install in a list table.
     *
     * @since 3.1.0
     *
     * @see WP_Themes_List_Table
     */
    class WP_Theme_Install_List_Table extends \WP_Themes_List_Table
    {
        public $features = array();
        /**
         * @return bool Whether the user can install themes.
         */
        public function ajax_user_can()
        {
        }
        /**
         * @global array  $tabs
         * @global string $tab
         * @global int    $paged
         * @global string $type
         * @global array  $theme_field_defaults
         * @phpstan-return void
         */
        public function prepare_items()
        {
        }
        /**
         */
        public function no_items()
        {
        }
        /**
         * @global array $tabs
         * @global string $tab
         * @return array<string, string> An array of links for the available views.
         */
        protected function get_views()
        {
        }
        /**
         * Displays the theme install table.
         *
         * Overrides the parent display() method to provide a different container.
         *
         * @since 3.1.0
         */
        public function display()
        {
        }
        /**
         * Generates the list table rows.
         *
         * @since 3.1.0
         */
        public function display_rows()
        {
        }
        /**
         * Prints a theme from the WordPress.org API.
         *
         * @since 3.1.0
         *
         * @global array $themes_allowedtags
         *
         * @param stdClass $theme {
         *     An object that contains theme data returned by the WordPress.org API.
         *
         *     @type string $name           Theme name, e.g. 'Twenty Twenty-One'.
         *     @type string $slug           Theme slug, e.g. 'twentytwentyone'.
         *     @type string $version        Theme version, e.g. '1.1'.
         *     @type string $author         Theme author username, e.g. 'melchoyce'.
         *     @type string $preview_url    Preview URL, e.g. 'https://2021.wordpress.net/'.
         *     @type string $screenshot_url Screenshot URL, e.g. 'https://wordpress.org/themes/twentytwentyone/'.
         *     @type float  $rating         Rating score.
         *     @type int    $num_ratings    The number of ratings.
         *     @type string $homepage       Theme homepage, e.g. 'https://wordpress.org/themes/twentytwentyone/'.
         *     @type string $description    Theme description.
         *     @type string $download_link  Theme ZIP download URL.
         * }
         * @phpstan-param object{
         *   name?: string,
         *   slug?: string,
         *   version?: string,
         *   author?: string,
         *   preview_url?: string,
         *   screenshot_url?: string,
         *   rating?: float,
         *   num_ratings?: int,
         *   homepage?: string,
         *   description?: string,
         *   download_link?: string,
         * } $theme
         * @phpstan-return void
         */
        public function single_row($theme)
        {
        }
        /**
         * Prints the wrapper for the theme installer.
         */
        public function theme_installer()
        {
        }
        /**
         * Prints the wrapper for the theme installer with a provided theme's data.
         * Used to make the theme installer work for no-js.
         *
         * @param stdClass $theme A WordPress.org Theme API object.
         */
        public function theme_installer_single($theme)
        {
        }
        /**
         * Prints the info for a theme (to be used in the theme installer modal).
         *
         * @global array $themes_allowedtags
         *
         * @param stdClass $theme A WordPress.org Theme API object.
         * @phpstan-return void
         */
        public function install_theme_info($theme)
        {
        }
        /**
         * Send required variables to JavaScript land
         *
         * @since 3.4.0
         *
         * @global string $tab  Current tab within Themes->Install screen
         * @global string $type Type of search.
         *
         * @param array $extra_args Unused.
         */
        public function _js_vars($extra_args = array())
        {
        }
    }
    /**
     * Core class used to implement displaying users in a list table.
     *
     * @since 3.1.0
     *
     * @see WP_List_Table
     */
    class WP_Users_List_Table extends \WP_List_Table
    {
        /**
         * Site ID to generate the Users list table for.
         *
         * @since 3.1.0
         * @var int
         */
        public $site_id;
        /**
         * Whether or not the current Users list table is for Multisite.
         *
         * @since 3.1.0
         * @var bool
         */
        public $is_site_users;
        /**
         * Constructor.
         *
         * @since 3.1.0
         *
         * @see WP_List_Table::__construct() for more information on default arguments.
         *
         * @param array $args An associative array of arguments.
         */
        public function __construct($args = array())
        {
        }
        /**
         * Checks the current user's permissions.
         *
         * @since 3.1.0
         *
         * @return bool
         */
        public function ajax_user_can()
        {
        }
        /**
         * Prepares the users list for display.
         *
         * @since 3.1.0
         *
         * @global string $role
         * @global string $usersearch
         */
        public function prepare_items()
        {
        }
        /**
         * Outputs 'no users' message.
         *
         * @since 3.1.0
         */
        public function no_items()
        {
        }
        /**
         * Returns an associative array listing all the views that can be used
         * with this table.
         *
         * Provides a list of roles and user count for that role for easy
         * filtering of the user table.
         *
         * @since 3.1.0
         *
         * @global string $role
         *
         * @return string[] An array of HTML links keyed by their view.
         */
        protected function get_views()
        {
        }
        /**
         * Retrieves an associative array of bulk actions available on this table.
         *
         * @since 3.1.0
         *
         * @return array Array of bulk action labels keyed by their action.
         */
        protected function get_bulk_actions()
        {
        }
        /**
         * Outputs the controls to allow user roles to be changed in bulk.
         *
         * @since 3.1.0
         *
         * @param string $which Whether this is being invoked above ("top")
         *                      or below the table ("bottom").
         */
        protected function extra_tablenav($which)
        {
        }
        /**
         * Captures the bulk action required, and return it.
         *
         * Overridden from the base class implementation to capture
         * the role change drop-down.
         *
         * @since 3.1.0
         *
         * @return string The bulk action required.
         */
        public function current_action()
        {
        }
        /**
         * Gets a list of columns for the list table.
         *
         * @since 3.1.0
         *
         * @return string[] Array of column titles keyed by their column name.
         */
        public function get_columns()
        {
        }
        /**
         * Gets a list of sortable columns for the list table.
         *
         * @since 3.1.0
         *
         * @return array Array of sortable columns.
         */
        protected function get_sortable_columns()
        {
        }
        /**
         * Generates the list table rows.
         *
         * @since 3.1.0
         */
        public function display_rows()
        {
        }
        /**
         * Generates HTML for a single row on the users.php admin panel.
         *
         * @since 3.1.0
         * @since 4.2.0 The `$style` parameter was deprecated.
         * @since 4.4.0 The `$role` parameter was deprecated.
         *
         * @param WP_User $user_object The current user object.
         * @param string  $style       Deprecated. Not used.
         * @param string  $role        Deprecated. Not used.
         * @param int     $numposts    Optional. Post count to display for this user. Defaults
         *                             to zero, as in, a new user has made zero posts.
         * @return string Output for a single row.
         */
        public function single_row($user_object, $style = '', $role = '', $numposts = 0)
        {
        }
        /**
         * Gets the name of the default primary column.
         *
         * @since 4.3.0
         *
         * @return string Name of the default primary column, in this case, 'username'.
         */
        protected function get_default_primary_column_name()
        {
        }
        /**
         * Returns an array of translated user role names for a given user object.
         *
         * @since 4.4.0
         *
         * @param WP_User $user_object The WP_User object.
         * @return string[] An array of user role names keyed by role.
         */
        protected function get_role_list($user_object)
        {
        }
    }
}

namespace WpOrg\Requests {
    /**
     * Requests for PHP
     *
     * Inspired by Requests for Python.
     *
     * Based on concepts from SimplePie_File, RequestCore and WP_Http.
     *
     * @package Requests
     */
    class Requests
    {
        /**
         * POST method
         *
         * @var string
         */
        const POST = 'POST';
        /**
         * PUT method
         *
         * @var string
         */
        const PUT = 'PUT';
        /**
         * GET method
         *
         * @var string
         */
        const GET = 'GET';
        /**
         * HEAD method
         *
         * @var string
         */
        const HEAD = 'HEAD';
        /**
         * DELETE method
         *
         * @var string
         */
        const DELETE = 'DELETE';
        /**
         * OPTIONS method
         *
         * @var string
         */
        const OPTIONS = 'OPTIONS';
        /**
         * TRACE method
         *
         * @var string
         */
        const TRACE = 'TRACE';
        /**
         * PATCH method
         *
         * @link https://tools.ietf.org/html/rfc5789
         * @var string
         */
        const PATCH = 'PATCH';
        /**
         * Default size of buffer size to read streams
         *
         * @var integer
         */
        const BUFFER_SIZE = 1160;
        /**
         * Option defaults.
         *
         * @see \WpOrg\Requests\Requests::get_default_options()
         * @see \WpOrg\Requests\Requests::request() for values returned by this method
         *
         * @since 2.0.0
         *
         * @var array
         */
        const OPTION_DEFAULTS = ['timeout' => 10, 'connect_timeout' => 10, 'useragent' => 'php-requests/' . self::VERSION, 'protocol_version' => 1.1, 'redirected' => 0, 'redirects' => 10, 'follow_redirects' => true, 'blocking' => true, 'type' => self::GET, 'filename' => false, 'auth' => false, 'proxy' => false, 'cookies' => false, 'max_bytes' => false, 'idn' => true, 'hooks' => null, 'transport' => null, 'verify' => null, 'verifyname' => true];
        /**
         * Default supported Transport classes.
         *
         * @since 2.0.0
         *
         * @var array
         */
        const DEFAULT_TRANSPORTS = [\WpOrg\Requests\Transport\Curl::class => \WpOrg\Requests\Transport\Curl::class, \WpOrg\Requests\Transport\Fsockopen::class => \WpOrg\Requests\Transport\Fsockopen::class];
        /**
         * Current version of Requests
         *
         * @var string
         */
        const VERSION = '2.0.17';
        /**
         * Selected transport name
         *
         * Use {@see \WpOrg\Requests\Requests::get_transport()} instead
         *
         * @var array
         */
        public static $transport = [];
        /**
         * Registered transport classes
         *
         * @var array
         */
        protected static $transports = [];
        /**
         * Default certificate path.
         *
         * @see \WpOrg\Requests\Requests::get_certificate_path()
         * @see \WpOrg\Requests\Requests::set_certificate_path()
         *
         * @var string
         */
        protected static $certificate_path = __DIR__ . '/../certificates/cacert.pem';
        /**
         * Register a transport
         *
         * @param string $transport Transport class to add, must support the \WpOrg\Requests\Transport interface
         */
        public static function add_transport($transport)
        {
        }
        /**
         * Get the fully qualified class name (FQCN) for a working transport.
         *
         * @param array<string, bool> $capabilities Optional. Associative array of capabilities to test against, i.e. `['<capability>' => true]`.
         * @return string FQCN of the transport to use, or an empty string if no transport was
         *                found which provided the requested capabilities.
         */
        protected static function get_transport_class(array $capabilities = [])
        {
        }
        /**
         * Get a working transport.
         *
         * @param array<string, bool> $capabilities Optional. Associative array of capabilities to test against, i.e. `['<capability>' => true]`.
         * @return \WpOrg\Requests\Transport
         * @throws \WpOrg\Requests\Exception If no valid transport is found (`notransport`).
         */
        protected static function get_transport(array $capabilities = [])
        {
        }
        /**
         * Checks to see if we have a transport for the capabilities requested.
         *
         * Supported capabilities can be found in the {@see \WpOrg\Requests\Capability}
         * interface as constants.
         *
         * Example usage:
         * `Requests::has_capabilities([Capability::SSL => true])`.
         *
         * @param array<string, bool> $capabilities Optional. Associative array of capabilities to test against, i.e. `['<capability>' => true]`.
         * @return bool Whether the transport has the requested capabilities.
         */
        public static function has_capabilities(array $capabilities = [])
        {
        }
        /**#@+
         * @see \WpOrg\Requests\Requests::request()
         * @param string $url
         * @param array $headers
         * @param array $options
         * @return \WpOrg\Requests\Response
         */
        /**
         * Send a GET request
         */
        public static function get($url, $headers = [], $options = [])
        {
        }
        /**
         * Send a HEAD request
         */
        public static function head($url, $headers = [], $options = [])
        {
        }
        /**
         * Send a DELETE request
         */
        public static function delete($url, $headers = [], $options = [])
        {
        }
        /**
         * Send a TRACE request
         */
        public static function trace($url, $headers = [], $options = [])
        {
        }
        /**#@-*/
        /**#@+
         * @see \WpOrg\Requests\Requests::request()
         * @param string $url
         * @param array $headers
         * @param array $data
         * @param array $options
         * @return \WpOrg\Requests\Response
         */
        /**
         * Send a POST request
         */
        public static function post($url, $headers = [], $data = [], $options = [])
        {
        }
        /**
         * Send a PUT request
         */
        public static function put($url, $headers = [], $data = [], $options = [])
        {
        }
        /**
         * Send an OPTIONS request
         */
        public static function options($url, $headers = [], $data = [], $options = [])
        {
        }
        /**
         * Send a PATCH request
         *
         * Note: Unlike {@see \WpOrg\Requests\Requests::post()} and {@see \WpOrg\Requests\Requests::put()},
         * `$headers` is required, as the specification recommends that should send an ETag
         *
         * @link https://tools.ietf.org/html/rfc5789
         */
        public static function patch($url, $headers, $data = [], $options = [])
        {
        }
        /**#@-*/
        /**
         * Main interface for HTTP requests
         *
         * This method initiates a request and sends it via a transport before
         * parsing.
         *
         * The `$options` parameter takes an associative array with the following
         * options:
         *
         * - `timeout`: How long should we wait for a response?
         *    Note: for cURL, a minimum of 1 second applies, as DNS resolution
         *    operates at second-resolution only.
         *    (float, seconds with a millisecond precision, default: 10, example: 0.01)
         * - `connect_timeout`: How long should we wait while trying to connect?
         *    (float, seconds with a millisecond precision, default: 10, example: 0.01)
         * - `useragent`: Useragent to send to the server
         *    (string, default: php-requests/$version)
         * - `follow_redirects`: Should we follow 3xx redirects?
         *    (boolean, default: true)
         * - `redirects`: How many times should we redirect before erroring?
         *    (integer, default: 10)
         * - `blocking`: Should we block processing on this request?
         *    (boolean, default: true)
         * - `filename`: File to stream the body to instead.
         *    (string|boolean, default: false)
         * - `auth`: Authentication handler or array of user/password details to use
         *    for Basic authentication
         *    (\WpOrg\Requests\Auth|array|boolean, default: false)
         * - `proxy`: Proxy details to use for proxy by-passing and authentication
         *    (\WpOrg\Requests\Proxy|array|string|boolean, default: false)
         * - `max_bytes`: Limit for the response body size.
         *    (integer|boolean, default: false)
         * - `idn`: Enable IDN parsing
         *    (boolean, default: true)
         * - `transport`: Custom transport. Either a class name, or a
         *    transport object. Defaults to the first working transport from
         *    {@see \WpOrg\Requests\Requests::getTransport()}
         *    (string|\WpOrg\Requests\Transport, default: {@see \WpOrg\Requests\Requests::getTransport()})
         * - `hooks`: Hooks handler.
         *    (\WpOrg\Requests\HookManager, default: new WpOrg\Requests\Hooks())
         * - `verify`: Should we verify SSL certificates? Allows passing in a custom
         *    certificate file as a string. (Using true uses the system-wide root
         *    certificate store instead, but this may have different behaviour
         *    across transports.)
         *    (string|boolean, default: certificates/cacert.pem)
         * - `verifyname`: Should we verify the common name in the SSL certificate?
         *    (boolean, default: true)
         * - `data_format`: How should we send the `$data` parameter?
         *    (string, one of 'query' or 'body', default: 'query' for
         *    HEAD/GET/DELETE, 'body' for POST/PUT/OPTIONS/PATCH)
         *
         * @param string|Stringable $url URL to request
         * @param array $headers Extra headers to send with the request
         * @param array|null $data Data to send either as a query string for GET/HEAD requests, or in the body for POST requests
         * @param string $type HTTP request type (use Requests constants)
         * @param array $options Options for the request (see description for more information)
         * @return \WpOrg\Requests\Response
         *
         * @throws \WpOrg\Requests\Exception\InvalidArgument When the passed $url argument is not a string or Stringable.
         * @throws \WpOrg\Requests\Exception\InvalidArgument When the passed $type argument is not a string.
         * @throws \WpOrg\Requests\Exception\InvalidArgument When the passed $options argument is not an array.
         * @throws \WpOrg\Requests\Exception On invalid URLs (`nonhttp`)
         */
        public static function request($url, $headers = [], $data = [], $type = self::GET, $options = [])
        {
        }
        /**
         * Send multiple HTTP requests simultaneously
         *
         * The `$requests` parameter takes an associative or indexed array of
         * request fields. The key of each request can be used to match up the
         * request with the returned data, or with the request passed into your
         * `multiple.request.complete` callback.
         *
         * The request fields value is an associative array with the following keys:
         *
         * - `url`: Request URL Same as the `$url` parameter to
         *    {@see \WpOrg\Requests\Requests::request()}
         *    (string, required)
         * - `headers`: Associative array of header fields. Same as the `$headers`
         *    parameter to {@see \WpOrg\Requests\Requests::request()}
         *    (array, default: `array()`)
         * - `data`: Associative array of data fields or a string. Same as the
         *    `$data` parameter to {@see \WpOrg\Requests\Requests::request()}
         *    (array|string, default: `array()`)
         * - `type`: HTTP request type (use \WpOrg\Requests\Requests constants). Same as the `$type`
         *    parameter to {@see \WpOrg\Requests\Requests::request()}
         *    (string, default: `\WpOrg\Requests\Requests::GET`)
         * - `cookies`: Associative array of cookie name to value, or cookie jar.
         *    (array|\WpOrg\Requests\Cookie\Jar)
         *
         * If the `$options` parameter is specified, individual requests will
         * inherit options from it. This can be used to use a single hooking system,
         * or set all the types to `\WpOrg\Requests\Requests::POST`, for example.
         *
         * In addition, the `$options` parameter takes the following global options:
         *
         * - `complete`: A callback for when a request is complete. Takes two
         *    parameters, a \WpOrg\Requests\Response/\WpOrg\Requests\Exception reference, and the
         *    ID from the request array (Note: this can also be overridden on a
         *    per-request basis, although that's a little silly)
         *    (callback)
         *
         * @param array $requests Requests data (see description for more information)
         * @param array $options Global and default options (see {@see \WpOrg\Requests\Requests::request()})
         * @return array Responses (either \WpOrg\Requests\Response or a \WpOrg\Requests\Exception object)
         *
         * @throws \WpOrg\Requests\Exception\InvalidArgument When the passed $requests argument is not an array or iterable object with array access.
         * @throws \WpOrg\Requests\Exception\InvalidArgument When the passed $options argument is not an array.
         */
        public static function request_multiple($requests, $options = [])
        {
        }
        /**
         * Get the default options
         *
         * @see \WpOrg\Requests\Requests::request() for values returned by this method
         * @param boolean $multirequest Is this a multirequest?
         * @return array Default option values
         */
        protected static function get_default_options($multirequest = false)
        {
        }
        /**
         * Get default certificate path.
         *
         * @return string Default certificate path.
         */
        public static function get_certificate_path()
        {
        }
        /**
         * Set default certificate path.
         *
         * @param string|Stringable|bool $path Certificate path, pointing to a PEM file.
         *
         * @throws \WpOrg\Requests\Exception\InvalidArgument When the passed $url argument is not a string, Stringable or boolean.
         */
        public static function set_certificate_path($path)
        {
        }
        /**
         * Set the default values
         *
         * The $options parameter is updated with the results.
         *
         * @param string $url URL to request
         * @param array $headers Extra headers to send with the request
         * @param array|null $data Data to send either as a query string for GET/HEAD requests, or in the body for POST requests
         * @param string $type HTTP request type
         * @param array $options Options for the request
         * @return void
         *
         * @throws \WpOrg\Requests\Exception When the $url is not an http(s) URL.
         */
        protected static function set_defaults(&$url, &$headers, &$data, &$type, &$options)
        {
        }
        /**
         * HTTP response parser
         *
         * @param string $headers Full response text including headers and body
         * @param string $url Original request URL
         * @param array $req_headers Original $headers array passed to {@link request()}, in case we need to follow redirects
         * @param array $req_data Original $data array passed to {@link request()}, in case we need to follow redirects
         * @param array $options Original $options array passed to {@link request()}, in case we need to follow redirects
         * @return \WpOrg\Requests\Response
         *
         * @throws \WpOrg\Requests\Exception On missing head/body separator (`requests.no_crlf_separator`)
         * @throws \WpOrg\Requests\Exception On missing head/body separator (`noversion`)
         * @throws \WpOrg\Requests\Exception On missing head/body separator (`toomanyredirects`)
         */
        protected static function parse_response($headers, $url, $req_headers, $req_data, $options)
        {
        }
        /**
         * Callback for `transport.internal.parse_response`
         *
         * Internal use only. Converts a raw HTTP response to a \WpOrg\Requests\Response
         * while still executing a multiple request.
         *
         * `$response` is either set to a \WpOrg\Requests\Response instance, or a \WpOrg\Requests\Exception object
         *
         * @param string $response Full response text including headers and body (will be overwritten with Response instance)
         * @param array $request Request data as passed into {@see \WpOrg\Requests\Requests::request_multiple()}
         * @return void
         */
        public static function parse_multiple(&$response, $request)
        {
        }
        /**
         * Decoded a chunked body as per RFC 2616
         *
         * @link https://tools.ietf.org/html/rfc2616#section-3.6.1
         * @param string $data Chunked body
         * @return string Decoded body
         */
        protected static function decode_chunked($data)
        {
        }
        /**
         * Convert a key => value array to a 'key: value' array for headers
         *
         * @param iterable $dictionary Dictionary of header values
         * @return array List of headers
         *
         * @throws \WpOrg\Requests\Exception\InvalidArgument When the passed argument is not iterable.
         */
        public static function flatten($dictionary)
        {
        }
        /**
         * Decompress an encoded body
         *
         * Implements gzip, compress and deflate. Guesses which it is by attempting
         * to decode.
         *
         * @param string $data Compressed data in one of the above formats
         * @return string Decompressed string
         *
         * @throws \WpOrg\Requests\Exception\InvalidArgument When the passed argument is not a string.
         */
        public static function decompress($data)
        {
        }
        /**
         * Decompression of deflated string while staying compatible with the majority of servers.
         *
         * Certain Servers will return deflated data with headers which PHP's gzinflate()
         * function cannot handle out of the box. The following function has been created from
         * various snippets on the gzinflate() PHP documentation.
         *
         * Warning: Magic numbers within. Due to the potential different formats that the compressed
         * data may be returned in, some "magic offsets" are needed to ensure proper decompression
         * takes place. For a simple progmatic way to determine the magic offset in use, see:
         * https://core.trac.wordpress.org/ticket/18273
         *
         * @since 1.6.0
         * @link https://core.trac.wordpress.org/ticket/18273
         * @link https://www.php.net/gzinflate#70875
         * @link https://www.php.net/gzinflate#77336
         *
         * @param string $gz_data String to decompress.
         * @return string|bool False on failure.
         *
         * @throws \WpOrg\Requests\Exception\InvalidArgument When the passed argument is not a string.
         */
        public static function compatible_gzinflate($gz_data)
        {
        }
    }
}

namespace {
    /**
     * Requests for PHP
     *
     * Inspired by Requests for Python.
     *
     * Based on concepts from SimplePie_File, RequestCore and WP_Http.
     *
     * @package Requests
     *
     * @deprecated 6.2.0 Use `WpOrg\Requests\Requests` instead for the actual functionality and
     *                   use `WpOrg\Requests\Autoload` for the autoloading.
     */
    class Requests extends \WpOrg\Requests\Requests
    {
        /**
         * Deprecated autoloader for Requests.
         *
         * @deprecated 6.2.0 Use the `WpOrg\Requests\Autoload::load()` method instead.
         *
         * @codeCoverageIgnore
         *
         * @param string $class Class name to load
         */
        public static function autoloader($class)
        {
        }
        /**
         * Register the built-in autoloader
         *
         * @deprecated 6.2.0 Include the `WpOrg\Requests\Autoload` class and
         *                   call `WpOrg\Requests\Autoload::register()` instead.
         *
         * @codeCoverageIgnore
         */
        public static function register_autoloader()
        {
        }
    }
    /**
     * Core class used to create an HTML list of categories.
     *
     * @since 2.1.0
     *
     * @see Walker
     */
    class Walker_Category extends \Walker
    {
        /**
         * What the class handles.
         *
         * @since 2.1.0
         * @var string
         *
         * @see Walker::$tree_type
         */
        public $tree_type = 'category';
        /**
         * Database fields to use.
         *
         * @since 2.1.0
         * @var string[]
         *
         * @see Walker::$db_fields
         * @todo Decouple this
         */
        public $db_fields = array('parent' => 'parent', 'id' => 'term_id');
        /**
         * Starts the list before the elements are added.
         *
         * @since 2.1.0
         *
         * @see Walker::start_lvl()
         *
         * @param string $output Used to append additional content. Passed by reference.
         * @param int    $depth  Optional. Depth of category. Used for tab indentation. Default 0.
         * @param array  $args   Optional. An array of arguments. Will only append content if style argument
         *                       value is 'list'. See wp_list_categories(). Default empty array.
         * @phpstan-param array{
         *   current_category?: int|int[],
         *   depth?: int,
         *   echo?: bool|int,
         *   exclude?: int[]|string,
         *   exclude_tree?: int[]|string,
         *   feed?: string,
         *   feed_image?: string,
         *   feed_type?: string,
         *   hide_title_if_empty?: bool,
         *   separator?: string,
         *   show_count?: bool|int,
         *   show_option_all?: string,
         *   show_option_none?: string,
         *   style?: string,
         *   taxonomy?: string,
         *   title_li?: string,
         *   use_desc_for_title?: bool|int,
         *   walker?: Walker,
         * } $args See wp_list_categories()
         * @phpstan-return void
         */
        public function start_lvl(&$output, $depth = 0, $args = array())
        {
        }
        /**
         * Ends the list of after the elements are added.
         *
         * @since 2.1.0
         *
         * @see Walker::end_lvl()
         *
         * @param string $output Used to append additional content. Passed by reference.
         * @param int    $depth  Optional. Depth of category. Used for tab indentation. Default 0.
         * @param array  $args   Optional. An array of arguments. Will only append content if style argument
         *                       value is 'list'. See wp_list_categories(). Default empty array.
         * @phpstan-param array{
         *   current_category?: int|int[],
         *   depth?: int,
         *   echo?: bool|int,
         *   exclude?: int[]|string,
         *   exclude_tree?: int[]|string,
         *   feed?: string,
         *   feed_image?: string,
         *   feed_type?: string,
         *   hide_title_if_empty?: bool,
         *   separator?: string,
         *   show_count?: bool|int,
         *   show_option_all?: string,
         *   show_option_none?: string,
         *   style?: string,
         *   taxonomy?: string,
         *   title_li?: string,
         *   use_desc_for_title?: bool|int,
         *   walker?: Walker,
         * } $args See wp_list_categories()
         * @phpstan-return void
         */
        public function end_lvl(&$output, $depth = 0, $args = array())
        {
        }
        /**
         * Starts the element output.
         *
         * @since 2.1.0
         * @since 5.9.0 Renamed `$category` to `$data_object` and `$id` to `$current_object_id`
         *              to match parent class for PHP 8 named parameter support.
         *
         * @see Walker::start_el()
         *
         * @param string  $output            Used to append additional content (passed by reference).
         * @param WP_Term $data_object       Category data object.
         * @param int     $depth             Optional. Depth of category in reference to parents. Default 0.
         * @param array   $args              Optional. An array of arguments. See wp_list_categories().
         *                                   Default empty array.
         * @param int     $current_object_id Optional. ID of the current category. Default 0.
         * @phpstan-param array{
         *   current_category?: int|int[],
         *   depth?: int,
         *   echo?: bool|int,
         *   exclude?: int[]|string,
         *   exclude_tree?: int[]|string,
         *   feed?: string,
         *   feed_image?: string,
         *   feed_type?: string,
         *   hide_title_if_empty?: bool,
         *   separator?: string,
         *   show_count?: bool|int,
         *   show_option_all?: string,
         *   show_option_none?: string,
         *   style?: string,
         *   taxonomy?: string,
         *   title_li?: string,
         *   use_desc_for_title?: bool|int,
         *   walker?: Walker,
         * } $args See wp_list_categories()
         * @phpstan-return void
         */
        public function start_el(&$output, $data_object, $depth = 0, $args = array(), $current_object_id = 0)
        {
        }
        /**
         * Ends the element output, if needed.
         *
         * @since 2.1.0
         * @since 5.9.0 Renamed `$page` to `$data_object` to match parent class for PHP 8 named parameter support.
         *
         * @see Walker::end_el()
         *
         * @param string $output      Used to append additional content (passed by reference).
         * @param object $data_object Category data object. Not used.
         * @param int    $depth       Optional. Depth of category. Not used.
         * @param array  $args        Optional. An array of arguments. Only uses 'list' for whether should
         *                            append to output. See wp_list_categories(). Default empty array.
         * @phpstan-param array{
         *   current_category?: int|int[],
         *   depth?: int,
         *   echo?: bool|int,
         *   exclude?: int[]|string,
         *   exclude_tree?: int[]|string,
         *   feed?: string,
         *   feed_image?: string,
         *   feed_type?: string,
         *   hide_title_if_empty?: bool,
         *   separator?: string,
         *   show_count?: bool|int,
         *   show_option_all?: string,
         *   show_option_none?: string,
         *   style?: string,
         *   taxonomy?: string,
         *   title_li?: string,
         *   use_desc_for_title?: bool|int,
         *   walker?: Walker,
         * } $args See wp_list_categories()
         * @phpstan-return void
         */
        public function end_el(&$output, $data_object, $depth = 0, $args = array())
        {
        }
    }
    /**
     * Core walker class used to create an HTML list of pages.
     *
     * @since 2.1.0
     *
     * @see Walker
     */
    class Walker_Page extends \Walker
    {
        /**
         * What the class handles.
         *
         * @since 2.1.0
         * @var string
         *
         * @see Walker::$tree_type
         */
        public $tree_type = 'page';
        /**
         * Database fields to use.
         *
         * @since 2.1.0
         * @var string[]
         *
         * @see Walker::$db_fields
         * @todo Decouple this.
         */
        public $db_fields = array('parent' => 'post_parent', 'id' => 'ID');
        /**
         * Outputs the beginning of the current level in the tree before elements are output.
         *
         * @since 2.1.0
         *
         * @see Walker::start_lvl()
         *
         * @param string $output Used to append additional content (passed by reference).
         * @param int    $depth  Optional. Depth of page. Used for padding. Default 0.
         * @param array  $args   Optional. Arguments for outputting the next level.
         *                       Default empty array.
         */
        public function start_lvl(&$output, $depth = 0, $args = array())
        {
        }
        /**
         * Outputs the end of the current level in the tree after elements are output.
         *
         * @since 2.1.0
         *
         * @see Walker::end_lvl()
         *
         * @param string $output Used to append additional content (passed by reference).
         * @param int    $depth  Optional. Depth of page. Used for padding. Default 0.
         * @param array  $args   Optional. Arguments for outputting the end of the current level.
         *                       Default empty array.
         */
        public function end_lvl(&$output, $depth = 0, $args = array())
        {
        }
        /**
         * Outputs the beginning of the current element in the tree.
         *
         * @see Walker::start_el()
         * @since 2.1.0
         * @since 5.9.0 Renamed `$page` to `$data_object` and `$current_page` to `$current_object_id`
         *              to match parent class for PHP 8 named parameter support.
         *
         * @param string  $output            Used to append additional content. Passed by reference.
         * @param WP_Post $data_object       Page data object.
         * @param int     $depth             Optional. Depth of page. Used for padding. Default 0.
         * @param array   $args              Optional. Array of arguments. Default empty array.
         * @param int     $current_object_id Optional. ID of the current page. Default 0.
         */
        public function start_el(&$output, $data_object, $depth = 0, $args = array(), $current_object_id = 0)
        {
        }
        /**
         * Outputs the end of the current element in the tree.
         *
         * @since 2.1.0
         * @since 5.9.0 Renamed `$page` to `$data_object` to match parent class for PHP 8 named parameter support.
         *
         * @see Walker::end_el()
         *
         * @param string  $output      Used to append additional content. Passed by reference.
         * @param WP_Post $data_object Page data object. Not used.
         * @param int     $depth       Optional. Depth of page. Default 0 (unused).
         * @param array   $args        Optional. Array of arguments. Default empty array.
         */
        public function end_el(&$output, $data_object, $depth = 0, $args = array())
        {
        }
    }
    /**
     * Send XML response back to Ajax request.
     *
     * @package WordPress
     * @since 2.1.0
     */
    #[\AllowDynamicProperties]
    class WP_Ajax_Response
    {
        /**
         * Store XML responses to send.
         *
         * @since 2.1.0
         * @var array
         */
        public $responses = array();
        /**
         * Constructor - Passes args to WP_Ajax_Response::add().
         *
         * @since 2.1.0
         *
         * @see WP_Ajax_Response::add()
         *
         * @param string|array $args Optional. Will be passed to add() method.
         */
        public function __construct($args = '')
        {
        }
        /**
         * Appends data to an XML response based on given arguments.
         *
         * With `$args` defaults, extra data output would be:
         *
         *     <response action='{$action}_$id'>
         *      <$what id='$id' position='$position'>
         *          <response_data><![CDATA[$data]]></response_data>
         *      </$what>
         *     </response>
         *
         * @since 2.1.0
         *
         * @param string|array $args {
         *     Optional. An array or string of XML response arguments.
         *
         *     @type string          $what         XML-RPC response type. Used as a child element of `<response>`.
         *                                         Default 'object' (`<object>`).
         *     @type string|false    $action       Value to use for the `action` attribute in `<response>`. Will be
         *                                         appended with `_$id` on output. If false, `$action` will default to
         *                                         the value of `$_POST['action']`. Default false.
         *     @type int|WP_Error    $id           The response ID, used as the response type `id` attribute. Also
         *                                         accepts a `WP_Error` object if the ID does not exist. Default 0.
         *     @type int|false       $old_id       The previous response ID. Used as the value for the response type
         *                                         `old_id` attribute. False hides the attribute. Default false.
         *     @type string          $position     Value of the response type `position` attribute. Accepts 1 (bottom),
         *                                         -1 (top), HTML ID (after), or -HTML ID (before). Default 1 (bottom).
         *     @type string|WP_Error $data         The response content/message. Also accepts a WP_Error object if the
         *                                         ID does not exist. Default empty.
         *     @type array           $supplemental An array of extra strings that will be output within a `<supplemental>`
         *                                         element as CDATA. Default empty array.
         * }
         * @return string XML response.
         * @phpstan-param array{
         *   what?: string,
         *   action?: string|false,
         *   id?: int|WP_Error,
         *   old_id?: int|false,
         *   position?: string,
         *   data?: string|WP_Error,
         *   supplemental?: array,
         * } $args
         * @phpstan-return non-falsy-string
         */
        public function add($args = '')
        {
        }
        /**
         * Display XML formatted responses.
         *
         * Sets the content type header to text/xml.
         *
         * @since 2.1.0
         *
         * @return never
         */
        public function send()
        {
        }
    }
    /**
     * Class representing a parsed instance of a block.
     *
     * @since 5.5.0
     * @property array $attributes
     */
    #[\AllowDynamicProperties]
    class WP_Block
    {
        /**
         * Original parsed array representation of block.
         *
         * @since 5.5.0
         * @var array
         */
        public $parsed_block;
        /**
         * Name of block.
         *
         * @example "core/paragraph"
         *
         * @since 5.5.0
         * @var string|null
         */
        public $name;
        /**
         * Block type associated with the instance.
         *
         * @since 5.5.0
         * @var WP_Block_Type
         */
        public $block_type;
        /**
         * Block context values.
         *
         * @since 5.5.0
         * @var array
         */
        public $context = array();
        /**
         * All available context of the current hierarchy.
         *
         * @since 5.5.0
         * @var array
         */
        protected $available_context = array();
        /**
         * Block type registry.
         *
         * @since 5.9.0
         * @var WP_Block_Type_Registry
         */
        protected $registry;
        /**
         * List of inner blocks (of this same class)
         *
         * @since 5.5.0
         * @var WP_Block_List
         */
        public $inner_blocks = array();
        /**
         * Resultant HTML from inside block comment delimiters after removing inner
         * blocks.
         *
         * @example "...Just <!-- wp:test /--> testing..." -> "Just testing..."
         *
         * @since 5.5.0
         * @var string
         */
        public $inner_html = '';
        /**
         * List of string fragments and null markers where inner blocks were found
         *
         * @example array(
         *   'inner_html'    => 'BeforeInnerAfter',
         *   'inner_blocks'  => array( block, block ),
         *   'inner_content' => array( 'Before', null, 'Inner', null, 'After' ),
         * )
         *
         * @since 5.5.0
         * @var array
         */
        public $inner_content = array();
        /**
         * Constructor.
         *
         * Populates object properties from the provided block instance argument.
         *
         * The given array of context values will not necessarily be available on
         * the instance itself, but is treated as the full set of values provided by
         * the block's ancestry. This is assigned to the private `available_context`
         * property. Only values which are configured to consumed by the block via
         * its registered type will be assigned to the block's `context` property.
         *
         * @since 5.5.0
         *
         * @param array                  $block             {
         *     An associative array of a single parsed block object. See WP_Block_Parser_Block.
         *
         *     @type string|null $blockName    Name of block.
         *     @type array       $attrs        Attributes from block comment delimiters.
         *     @type array       $innerBlocks  List of inner blocks. An array of arrays that
         *                                     have the same structure as this one.
         *     @type string      $innerHTML    HTML from inside block comment delimiters.
         *     @type array       $innerContent List of string fragments and null markers where inner blocks were found.
         * }
         * @param array                  $available_context Optional array of ancestry context values.
         * @param WP_Block_Type_Registry $registry          Optional block type registry.
         * @phpstan-param array{
         *   blockName?: string|null,
         *   attrs?: array,
         *   innerBlocks?: array,
         *   innerHTML?: string,
         *   innerContent?: array,
         * } $block
         */
        public function __construct($block, $available_context = array(), $registry = \null)
        {
        }
        /**
         * Updates the context for the current block and its inner blocks.
         *
         * The method updates the context of inner blocks, if any, by passing down
         * any context values the block provides (`provides_context`).
         *
         * If the block has inner blocks, the method recursively processes them by creating new instances of `WP_Block`
         * for each inner block and updating their context based on the block's `provides_context` property.
         *
         * @since 6.8.0
         */
        public function refresh_context_dependents()
        {
        }
        /**
         * Updates the parsed block content for the current block and its inner blocks.
         *
         * This method sets the `inner_html` and `inner_content` properties of the block based on the parsed
         * block content provided during initialization. It ensures that the block instance reflects the
         * most up-to-date content for both the inner HTML and any string fragments around inner blocks.
         *
         * If the block has inner blocks, this method initializes a new `WP_Block_List` for them, ensuring the
         * correct content and context are updated for each nested block.
         *
         * @since 6.8.0
         */
        public function refresh_parsed_block_dependents()
        {
        }
        /**
         * Returns a value from an inaccessible property.
         *
         * This is used to lazily initialize the `attributes` property of a block,
         * such that it is only prepared with default attributes at the time that
         * the property is accessed. For all other inaccessible properties, a `null`
         * value is returned.
         *
         * @since 5.5.0
         *
         * @param string $name Property name.
         * @return array|null Prepared attributes, or null.
         */
        public function __get($name)
        {
        }
        /**
         * Generates the render output for the block.
         *
         * @since 5.5.0
         * @since 6.5.0 Added block bindings processing.
         * @since 7.1.0 Preserve inner blocks when binding a rich text attribute.
         *
         * @global WP_Post $post Global post object.
         *
         * @param array $options {
         *     Optional options object.
         *
         *     @type bool $dynamic Defaults to 'true'. Optionally set to false to avoid using the block's render_callback.
         * }
         * @return string Rendered block output.
         * @phpstan-param array{
         *   dynamic?: bool,
         * } $options
         */
        public function render($options = array())
        {
        }
    }
    /**
     * Core class used to organize comments as instantiated objects with defined members.
     *
     * The `@property-read` fields below are not stored on the comment. They are proxied to the
     * comment's post by {@see WP_Comment::__get()}, and are null when the comment is not attached
     * to a post or when that post no longer exists.
     *
     * @since 4.4.0
     *
     * @property-read numeric-string|''|null $post_author
     * @property-read string|null            $post_date
     * @property-read string|null            $post_date_gmt
     * @property-read string|null            $post_content
     * @property-read string|null            $post_title
     * @property-read string|null            $post_excerpt
     * @property-read non-empty-string|null  $post_status
     * @property-read non-empty-string|null  $comment_status
     * @property-read non-empty-string|null  $ping_status
     * @property-read string|null            $post_name
     * @property-read string|null            $to_ping
     * @property-read string|null            $pinged
     * @property-read string|null            $post_modified
     * @property-read string|null            $post_modified_gmt
     * @property-read string|null            $post_content_filtered
     * @property-read non-negative-int|null  $post_parent
     * @property-read string|null            $guid
     * @property-read int|null               $menu_order
     * @property-read non-empty-string|null  $post_type
     * @property-read string|null            $post_mime_type
     * @property-read numeric-string|null    $comment_count
     *
     * @phpstan-type Data_Array array{
     *     comment_ID: numeric-string,
     *     comment_post_ID: numeric-string,
     *     comment_author: string,
     *     comment_author_email: string,
     *     comment_author_url: string,
     *     comment_author_IP: string,
     *     comment_date: non-empty-string,
     *     comment_date_gmt: non-empty-string,
     *     comment_content: string,
     *     comment_karma: numeric-string,
     *     comment_approved: non-empty-string,
     *     comment_agent: string,
     *     comment_type: string,
     *     comment_parent: numeric-string,
     *     user_id: numeric-string,
     *     ...
     * }
     */
    #[\AllowDynamicProperties]
    final class WP_Comment
    {
        /**
         * Comment ID.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 4.4.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $comment_ID;
        /**
         * ID of the post the comment is associated with.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 4.4.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $comment_post_ID = '0';
        /**
         * Comment author name.
         *
         * @since 4.4.0
         * @var string
         */
        public $comment_author = '';
        /**
         * Comment author email address.
         *
         * @since 4.4.0
         * @var string
         */
        public $comment_author_email = '';
        /**
         * Comment author URL.
         *
         * @since 4.4.0
         * @var string
         */
        public $comment_author_url = '';
        /**
         * Comment author IP address (IPv4 format).
         *
         * @since 4.4.0
         * @var string
         */
        public $comment_author_IP = '';
        /**
         * Comment date in YYYY-MM-DD HH:MM:SS format.
         *
         * @since 4.4.0
         * @var string
         * @phpstan-var non-empty-string
         */
        public $comment_date = '0000-00-00 00:00:00';
        /**
         * Comment GMT date in YYYY-MM-DD HH::MM:SS format.
         *
         * @since 4.4.0
         * @var string
         * @phpstan-var non-empty-string
         */
        public $comment_date_gmt = '0000-00-00 00:00:00';
        /**
         * Comment content.
         *
         * @since 4.4.0
         * @var string
         */
        public $comment_content;
        /**
         * Comment karma count.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 4.4.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $comment_karma = '0';
        /**
         * Comment approval status.
         *
         * The values used in core are '0' (unapproved), '1' (approved), 'spam', 'trash',
         * and 'post-trashed' (set for every comment on a post that is moved to the trash).
         *
         * @since 4.4.0
         * @var string
         * @phpstan-var non-empty-string
         */
        public $comment_approved = '1';
        /**
         * Comment author HTTP user agent.
         *
         * @since 4.4.0
         * @var string
         */
        public $comment_agent = '';
        /**
         * Comment type.
         *
         * The values used in core are 'comment', 'pingback', 'trackback', and 'note'. Custom
         * comment types are possible.
         *
         * Comments created before 5.5.0 may store an empty string rather than 'comment', so this
         * cannot be relied upon to be non-empty. {@see get_comment_type()} normalizes that case
         * when reading.
         *
         * @since 4.4.0
         * @since 5.5.0 Default value changed to `comment`.
         * @var string
         */
        public $comment_type = 'comment';
        /**
         * Parent comment ID.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 4.4.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $comment_parent = '0';
        /**
         * Comment author ID.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 4.4.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $user_id = '0';
        /**
         * Retrieves a WP_Comment instance.
         *
         * @since 4.4.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param int $id Comment ID.
         * @phpstan-param int|numeric-string $id
         * @return WP_Comment|false Comment object, otherwise false.
         */
        public static function get_instance($id)
        {
        }
        /**
         * Constructor.
         *
         * Populates properties with object vars.
         *
         * @since 4.4.0
         *
         * @param object $comment Comment object.
         */
        public function __construct($comment)
        {
        }
        /**
         * Converts object to array.
         *
         * @since 4.4.0
         *
         * @return array<string, mixed> Object as array.
         * @phpstan-return Data_Array
         */
        public function to_array(): array
        {
        }
        /**
         * Gets the children of a comment.
         *
         * @since 4.4.0
         * @since 7.1.0 A `count` or `fields` query now returns its result directly rather than
         *              erroneously storing it in the comment's children cache.
         *
         * @param array $args {
         *     Array of arguments used to pass to {@see get_comments()} and determine format.
         *     Any other argument accepted by {@see WP_Comment_Query::__construct()} may also be passed, and is
         *     forwarded to `get_comments()`. Note that `parent` is always overridden with this comment's ID.
         *     A `$count` or `$fields` query returns the direct children only, and does not populate the
         *     comment's cached children, since that cache holds `WP_Comment` objects.
         *
         *     @type string $format        Return value format. 'tree' for a hierarchical tree, 'flat' for a flattened array.
         *                                 Default 'tree'.
         *     @type string $status        Comment status to limit results by. Accepts 'hold' (`comment_status=0`),
         *                                 'approve' (`comment_status=1`), 'all', or a custom comment status.
         *                                 Default 'all'.
         *     @type string $hierarchical  Whether to include comment descendants in the results.
         *                                 'threaded' returns a tree, with each comment's children
         *                                 stored in a `children` property on the `WP_Comment` object.
         *                                 'flat' returns a flat array of found comments plus their children.
         *                                 Pass `false` to leave out descendants.
         *                                 The parameter is ignored (forced to `false`) when `$fields` is 'ids' or 'counts'.
         *                                 Accepts 'threaded', 'flat', or false. Default: 'threaded'.
         *     @type string|array $orderby Comment status or array of statuses. To use 'meta_value'
         *                                 or 'meta_value_num', `$meta_key` must also be defined.
         *                                 To sort by a specific `$meta_query` clause, use that
         *                                 clause's array key. Accepts 'comment_agent',
         *                                 'comment_approved', 'comment_author',
         *                                 'comment_author_email', 'comment_author_IP',
         *                                 'comment_author_url', 'comment_content', 'comment_date',
         *                                 'comment_date_gmt', 'comment_ID', 'comment_karma',
         *                                 'comment_parent', 'comment_post_ID', 'comment_type',
         *                                 'user_id', 'comment__in', 'meta_value', 'meta_value_num',
         *                                 the value of $meta_key, and the array keys of
         *                                 `$meta_query`. Also accepts false, an empty array, or
         *                                 'none' to disable `ORDER BY` clause.
         *     @type string $fields        Which fields to return. Accepts 'ids' for comment IDs, or an
         *                                 empty string for full `WP_Comment` objects. Default empty.
         *     @type bool   $count         Whether to return a comment count rather than comments.
         *                                 Default false.
         *     @type string $type          Limit results to comments of a given type, such as 'comment',
         *                                 'pingback', 'trackback', or 'note'. Accepts 'all' for every
         *                                 type. Default empty.
         *     @type int    $number        Maximum number of comments to retrieve. Default empty (no limit).
         *     @type int    $post_id       Limit results to comments on a given post. Default 0.
         *     @type string $order         How to order retrieved comments. Accepts 'ASC' or 'DESC'.
         *                                 Default 'DESC'.
         * }
         * @return WP_Comment[]|int[]|int Array of `WP_Comment` objects, an array of comment IDs when
         *                                `$fields` is 'ids', or the number of children when `$count`
         *                                is true.
         *
         * @phpstan-param array{
         *                    format?: 'tree'|'flat',
         *                    status?: 'hold'|'approve'|'all'|string,
         *                    hierarchical?: 'threaded'|'flat'|false,
         *                    orderby?: string|string[]|false,
         *                    fields?: 'ids'|'',
         *                    count?: bool,
         *                    type?: string,
         *                    number?: int,
         *                    post_id?: int,
         *                    order?: 'ASC'|'DESC',
         *                    ...
         *                } $args
         * @phpstan-return (
         *     $args is array{ count: true, ... } ? non-negative-int : (
         *         $args is array{ fields: 'ids', ... } ? non-negative-int[] : (
         *             $args is array{ format: 'flat', ... } ? list<WP_Comment> : array<int, WP_Comment>
         *         )
         *     )
         * )
         */
        public function get_children($args = array())
        {
        }
        /**
         * Adds a child to the comment.
         *
         * Used by `WP_Comment_Query` when bulk-filling descendants.
         *
         * @since 4.4.0
         *
         * @param WP_Comment $child Child comment.
         */
        public function add_child(\WP_Comment $child): void
        {
        }
        /**
         * Gets a child comment by ID.
         *
         * @since 4.4.0
         *
         * @param int $child_id ID of the child.
         * @return WP_Comment|false Returns the comment object if found, otherwise false.
         */
        public function get_child($child_id)
        {
        }
        /**
         * Sets the 'populated_children' flag.
         *
         * This flag is important for ensuring that calling `get_children()` on a childless comment will not trigger
         * unneeded database queries.
         *
         * @since 4.4.0
         *
         * @param bool $set Whether the comment's children have already been populated.
         */
        public function populated_children($set): void
        {
        }
        /**
         * Determines whether a non-public property is set.
         *
         * If `$name` matches a post field, the comment post will be loaded and the post's value checked.
         *
         * @since 4.4.0
         * @since 7.1.0 Returns false instead of causing a fatal error when the comment's post cannot be found.
         *
         * @param string $name Property to check if set.
         * @return bool Whether the property is set.
         */
        public function __isset($name)
        {
        }
        /**
         * Magic getter.
         *
         * If `$name` matches a post field, the comment post will be loaded and the post's value returned.
         *
         * @since 4.4.0
         * @since 7.1.0 Returns null instead of the global post's field when the comment is not attached to
         *              a post, and no longer raises a warning when the comment's post cannot be found.
         *
         * @param string $name Property name.
         * @return mixed
         */
        public function __get($name)
        {
        }
    }
    /**
     * Core base class extended to register items.
     *
     * @since 2.6.0
     *
     * @see _WP_Dependency
     */
    #[\AllowDynamicProperties]
    class WP_Dependencies
    {
        /**
         * An array of all registered dependencies keyed by handle.
         *
         * @since 2.6.8
         *
         * @var _WP_Dependency[]
         */
        public $registered = array();
        /**
         * An array of handles of queued dependencies.
         *
         * @since 2.6.8
         *
         * @var string[]
         */
        public $queue = array();
        /**
         * An array of handles of dependencies to queue.
         *
         * @since 2.6.0
         *
         * @var string[]
         */
        public $to_do = array();
        /**
         * An array of handles of dependencies already queued.
         *
         * @since 2.6.0
         *
         * @var string[]
         */
        public $done = array();
        /**
         * An array of additional arguments passed when a handle is registered.
         *
         * The keys are dependency handles and the values are query strings which are appended to the item URL's query
         * string, after the `ver` if provided.
         *
         * @since 2.6.0
         *
         * @var array<string, string>
         */
        public $args = array();
        /**
         * An array of dependency groups to enqueue.
         *
         * Each entry is keyed by handle and represents the integer group level or boolean
         * false if the handle has no group.
         *
         * @since 2.8.0
         *
         * @var (int|false)[]
         * @phpstan-var array<string, int|false>
         */
        public $groups = array();
        /**
         * A handle group to enqueue.
         *
         * @since 2.8.0
         *
         * @deprecated 4.5.0
         * @var int
         */
        public $group = 0;
        /**
         * Processes the items and dependencies.
         *
         * Processes the items passed to it or the queue, and their dependencies.
         *
         * @since 2.6.0
         * @since 2.8.0 Added the `$group` parameter.
         *
         * @param string|string[]|false $handles Optional. Items to be processed: queue (false),
         *                                       single item (string), or multiple items (array of strings).
         *                                       Default false.
         * @param int|false             $group   Optional. Group level: level (int), no group (false).
         * @return string[] Array of handles of items that have been processed.
         */
        public function do_items($handles = \false, $group = \false)
        {
        }
        /**
         * Processes a dependency.
         *
         * @since 2.6.0
         * @since 5.5.0 Added the `$group` parameter.
         *
         * @param string    $handle Name of the item. Should be unique.
         * @param int|false $group  Optional. Group level: level (int), no group (false).
         *                          Default false.
         * @return bool True on success, false if not set.
         */
        public function do_item($handle, $group = \false)
        {
        }
        /**
         * Determines dependencies.
         *
         * Recursively builds an array of items to process taking
         * dependencies into account. Does NOT catch infinite loops.
         *
         * @since 2.1.0
         * @since 2.6.0 Moved from `WP_Scripts`.
         * @since 2.8.0 Added the `$group` parameter.
         *
         * @param string|string[] $handles   Item handle (string) or item handles (array of strings).
         * @param bool            $recursion Optional. Internal flag that function is calling itself.
         *                                   Default false.
         * @param int|false       $group     Optional. Group level: level (int), no group (false).
         *                                   Default false.
         * @return bool True on success, false on failure.
         */
        public function all_deps($handles, $recursion = \false, $group = \false)
        {
        }
        /**
         * Register an item.
         *
         * Registers the item if no item of that name already exists.
         *
         * @since 2.1.0
         * @since 2.6.0 Moved from `WP_Scripts`.
         *
         * @param string           $handle Name of the item. Should be unique.
         * @param string|false     $src    Full URL of the item, or path of the item relative
         *                                 to the WordPress root directory. If source is set to false,
         *                                 the item is an alias of other items it depends on.
         * @param string[]         $deps   Optional. An array of registered item handles this item depends on.
         *                                 Default empty array.
         * @param string|bool|null $ver    Optional. String specifying item version number, if it has one,
         *                                 which is added to the URL as a query string for cache busting purposes.
         *                                 If version is set to false, a version number is automatically added
         *                                 equal to current installed WordPress version.
         *                                 If set to null, no version is added.
         * @param mixed            $args   Optional. Custom property of the item. NOT the class property $args.
         *                                 Examples: $media, $in_footer.
         * @return bool Whether the item has been registered. True on success, false on failure.
         */
        public function add($handle, $src, $deps = array(), $ver = \false, $args = \null)
        {
        }
        /**
         * Add extra item data.
         *
         * Adds data to a registered item.
         *
         * @since 2.6.0
         *
         * @param string $handle Name of the item. Should be unique.
         * @param string $key    The data key.
         * @param mixed  $value  The data value.
         * @return bool True on success, false on failure.
         */
        public function add_data($handle, $key, $value)
        {
        }
        /**
         * Get extra item data.
         *
         * Gets data associated with a registered item.
         *
         * @since 3.3.0
         *
         * @param string $handle Name of the item. Should be unique.
         * @param string $key    The data key.
         * @return mixed Extra item data (string), false otherwise.
         */
        public function get_data($handle, $key)
        {
        }
        /**
         * Un-register an item or items.
         *
         * @since 2.1.0
         * @since 2.6.0 Moved from `WP_Scripts`.
         *
         * @param string|string[] $handles Item handle (string) or item handles (array of strings).
         */
        public function remove($handles)
        {
        }
        /**
         * Queue an item or items.
         *
         * Decodes handles and arguments, then queues handles and stores
         * arguments in the class property $args. For example in extending
         * classes, $args is appended to the item url as a query string.
         * Note $args is NOT the $args property of items in the $registered array.
         *
         * @since 2.1.0
         * @since 2.6.0 Moved from `WP_Scripts`.
         *
         * @param string|string[] $handles Item handle (string) or item handles (array of strings).
         */
        public function enqueue($handles)
        {
        }
        /**
         * Dequeue an item or items.
         *
         * Decodes handles and arguments, then dequeues handles
         * and removes arguments from the class property $args.
         *
         * @since 2.1.0
         * @since 2.6.0 Moved from `WP_Scripts`.
         *
         * @param string|string[] $handles Item handle (string) or item handles (array of strings).
         */
        public function dequeue($handles)
        {
        }
        /**
         * Recursively search the passed dependency tree for a handle.
         *
         * @since 4.0.0
         *
         * @param string[] $queue  An array of queued _WP_Dependency handles.
         * @param string   $handle Name of the item. Should be unique.
         * @return bool Whether the handle is found after recursively searching the dependency tree.
         */
        protected function recurse_deps($queue, $handle)
        {
        }
        /**
         * Query the list for an item.
         *
         * @since 2.1.0
         * @since 2.6.0 Moved from `WP_Scripts`.
         *
         * @param string $handle Name of the item. Should be unique.
         * @param string $status Optional. Status of the item to query. Default 'registered'.
         * @return bool|_WP_Dependency Found, or object Item data.
         * @phpstan-template TStatus 'registered'|'scripts'|'enqueued'|'queued'|'to_do'|'to_print'|'done'|'printed'
         * @phpstan-return ($handle is not non-empty-string ? false : ($status is not TStatus ? false : ($status is 'registered'|'scripts' ? _WP_Dependency|false : bool)))
         */
        public function query($handle, $status = 'registered')
        {
        }
        /**
         * Set item group, unless already in a lower group.
         *
         * @since 2.8.0
         *
         * @param string    $handle    Name of the item. Should be unique.
         * @param bool      $recursion Internal flag that calling function was called recursively.
         * @param int|false $group     Group level: level (int), no group (false).
         * @return bool Not already in the group or a lower group.
         */
        public function set_group($handle, $recursion, $group)
        {
        }
        /**
         * Get etag header for cache validation.
         *
         * @since 6.7.0
         *
         * @global string $wp_version The WordPress version string.
         *
         * @param string[] $load Array of script or style handles to load.
         * @return string Etag header.
         * @phpstan-return non-falsy-string
         */
        public function get_etag($load)
        {
        }
        /**
         * Gets a dependency warning message for a handle.
         *
         * @since 6.9.1
         *
         * @param string   $handle                     Handle with missing dependencies.
         * @param string[] $missing_dependency_handles Missing dependency handles.
         * @return string Formatted, localized warning message.
         */
        protected function get_dependency_warning_message($handle, $missing_dependency_handles)
        {
        }
    }
    /**
     * Class _WP_Dependency
     *
     * Helper class to register a handle and associated data.
     *
     * @access private
     * @since 2.6.0
     */
    #[\AllowDynamicProperties]
    class _WP_Dependency
    {
        /**
         * The handle name.
         *
         * @since 2.6.0
         * @var string
         */
        public $handle;
        /**
         * The handle source.
         *
         * If source is set to false, the item is an alias of other items it depends on.
         *
         * @since 2.6.0
         * @var string|false
         */
        public $src;
        /**
         * An array of handle dependencies.
         *
         * @since 2.6.0
         * @var string[]
         */
        public $deps = array();
        /**
         * The handle version.
         *
         * Used for cache-busting.
         *
         * @since 2.6.0
         * @var string|false|null
         */
        public $ver = \false;
        /**
         * Additional arguments for the handle.
         *
         * @since 2.6.0
         * @var mixed
         */
        public $args = \null;
        /**
         * Extra data to supply to the handle.
         *
         * @since 2.6.0
         * @var array<string, mixed>
         */
        public $extra = array();
        /**
         * Translation textdomain set for this dependency.
         *
         * @since 5.0.0
         * @var string
         */
        public $textdomain;
        /**
         * Translation path set for this dependency.
         *
         * @since 5.0.0
         * @var string|null
         */
        public $translations_path;
        /**
         * Setup dependencies.
         *
         * @since 2.6.0
         * @since 5.3.0 Formalized the existing `...$args` parameter by adding it
         *              to the function signature.
         *
         * @param mixed ...$args Dependency information.
         */
        public function __construct(...$args)
        {
        }
        /**
         * Add handle data.
         *
         * @since 2.6.0
         *
         * @param string $name The data key to add.
         * @param mixed  $data The data value to add.
         * @return bool False if not scalar, true otherwise.
         */
        public function add_data($name, $data)
        {
        }
        /**
         * Sets the translation domain for this dependency.
         *
         * @since 5.0.0
         *
         * @param string $domain The translation textdomain.
         * @param string $path   Optional. The full file path to the directory containing translation files.
         * @return bool False if $domain is not a string, true otherwise.
         */
        public function set_translations($domain, $path = '')
        {
        }
    }
    /**
     * WordPress Error class.
     *
     * Container for checking for WordPress errors and error messages. Return
     * WP_Error and use is_wp_error() to check if this class is returned. Many
     * core WordPress functions pass this class in the event of an error and
     * if not handled properly will result in code errors.
     *
     * @since 2.1.0
     */
    #[\AllowDynamicProperties]
    class WP_Error
    {
        /**
         * Stores the list of errors.
         *
         * @since 2.1.0
         * @var array<int|string, string[]>
         */
        public $errors = array();
        /**
         * Stores the most recently added data for each error code.
         *
         * @since 2.1.0
         * @var array<int|string, mixed>
         */
        public $error_data = array();
        /**
         * Stores previously added data added for error codes, oldest-to-newest by code.
         *
         * @since 5.6.0
         * @var array<int|string, mixed[]>
         */
        protected $additional_data = array();
        /**
         * Initializes the error.
         *
         * If `$code` is empty, the other parameters will be ignored.
         * When `$code` is not empty, `$message` will be used even if
         * it is empty. The `$data` parameter will be used only if it
         * is not empty.
         *
         * Though the class is constructed with a single error code and
         * message, multiple codes can be added using the `add()` method.
         *
         * @since 2.1.0
         *
         * @param string|int $code    Error code.
         * @param string     $message Error message.
         * @param mixed      $data    Optional. Error data. Default empty string.
         * @phpstan-return void
         */
        public function __construct($code = '', $message = '', $data = '')
        {
        }
        /**
         * Retrieves all error codes.
         *
         * @since 2.1.0
         *
         * @return list<int|string> List of error codes, if available.
         */
        public function get_error_codes()
        {
        }
        /**
         * Retrieves the first error code available.
         *
         * @since 2.1.0
         *
         * @return string|int Empty string, if no error codes.
         */
        public function get_error_code()
        {
        }
        /**
         * Retrieves all error messages, or the error messages for the given error code.
         *
         * @since 2.1.0
         *
         * @param string|int $code Optional. Error code to retrieve the messages for.
         *                         Default empty string.
         * @return string[] Error strings on success, or empty array if there are none.
         */
        public function get_error_messages($code = '')
        {
        }
        /**
         * Gets a single error message.
         *
         * This will get the first message available for the code. If no code is
         * given then the first code available will be used.
         *
         * @since 2.1.0
         *
         * @param string|int $code Optional. Error code to retrieve the message for.
         *                         Default empty string.
         * @return string The error message.
         */
        public function get_error_message($code = '')
        {
        }
        /**
         * Retrieves the most recently added error data for an error code.
         *
         * @since 2.1.0
         *
         * @param string|int $code Optional. Error code. Default empty string.
         * @return mixed Error data, if it exists.
         */
        public function get_error_data($code = '')
        {
        }
        /**
         * Verifies if the instance contains errors.
         *
         * @since 5.1.0
         *
         * @return bool If the instance contains errors.
         */
        public function has_errors()
        {
        }
        /**
         * Adds an error or appends an additional message to an existing error.
         *
         * @since 2.1.0
         *
         * @param string|int $code    Error code.
         * @param string     $message Error message.
         * @param mixed      $data    Optional. Error data. Default empty string.
         * @return void
         */
        public function add($code, $message, $data = '')
        {
        }
        /**
         * Adds data to an error with the given code.
         *
         * @since 2.1.0
         * @since 5.6.0 Errors can now contain more than one item of error data. {@see WP_Error::$additional_data}.
         *
         * @param mixed      $data Error data.
         * @param string|int $code Error code.
         * @return void
         */
        public function add_data($data, $code = '')
        {
        }
        /**
         * Retrieves all error data for an error code in the order in which the data was added.
         *
         * @since 5.6.0
         *
         * @param string|int $code Error code.
         * @return mixed[] Array of error data, if it exists.
         */
        public function get_all_error_data($code = '')
        {
        }
        /**
         * Removes the specified error.
         *
         * This function removes all error messages associated with the specified
         * error code, along with any error data for that code.
         *
         * @since 4.1.0
         *
         * @param string|int $code Error code.
         * @return void
         */
        public function remove($code)
        {
        }
        /**
         * Merges the errors in the given error object into this one.
         *
         * @since 5.6.0
         *
         * @param WP_Error $error Error object to merge.
         * @return void
         */
        public function merge_from(\WP_Error $error)
        {
        }
        /**
         * Exports the errors in this object into the given one.
         *
         * @since 5.6.0
         *
         * @param WP_Error $error Error object to export into.
         * @return void
         */
        public function export_to(\WP_Error $error)
        {
        }
        /**
         * Copies errors from one WP_Error instance to another.
         *
         * @since 5.6.0
         *
         * @param WP_Error $from The WP_Error to copy from.
         * @param WP_Error $to   The WP_Error to copy to.
         * @return void
         */
        protected static function copy_errors(\WP_Error $from, \WP_Error $to)
        {
        }
    }
    /**
     * Core class used to implement action and filter hook functionality.
     *
     * @since 4.7.0
     *
     * @see Iterator
     * @see ArrayAccess
     *
     * @phpstan-type Hook_Callback array{
     *   function: callable,
     *   accepted_args: int,
     * }
     *
     * @phpstan-implements Iterator<int, array<non-decimal-int-string, Hook_Callback>>
     * @phpstan-implements ArrayAccess<int, array<non-decimal-int-string, Hook_Callback>>
     */
    #[\AllowDynamicProperties]
    final class WP_Hook implements \Iterator, \ArrayAccess
    {
        /**
         * Hook callbacks keyed by priority.
         *
         * @since 4.7.0
         * @var array
         * @phpstan-var array<int, array<non-decimal-int-string, Hook_Callback>>
         */
        public $callbacks = array();
        /**
         * Adds a callback function to a filter hook.
         *
         * @since 4.7.0
         *
         * @param string   $hook_name     The name of the filter to add the callback to.
         * @param callable $callback      The callback to be run when the filter is applied.
         * @param int      $priority      The order in which the functions associated with a particular filter
         *                                are executed. Lower numbers correspond with earlier execution,
         *                                and functions with the same priority are executed in the order
         *                                in which they were added to the filter.
         * @param int      $accepted_args The number of arguments the function accepts.
         * @phpstan-return void
         */
        public function add_filter($hook_name, $callback, $priority, $accepted_args)
        {
        }
        /**
         * Removes a callback function from a filter hook.
         *
         * @since 4.7.0
         *
         * @param string                $hook_name The filter hook to which the function to be removed is hooked.
         * @param callable|string|array $callback  The callback to be removed from running when the filter is applied.
         *                                         This method can be called unconditionally to speculatively remove
         *                                         a callback that may or may not exist.
         * @param int                   $priority  The exact priority used when adding the original filter callback.
         * @return bool Whether the callback existed before it was removed.
         */
        public function remove_filter($hook_name, $callback, $priority)
        {
        }
        /**
         * Checks if a specific callback has been registered for this hook.
         *
         * When using the `$callback` argument, this function may return a non-boolean value
         * that evaluates to false (e.g. 0), so use the `===` operator for testing the return value.
         *
         * @since 4.7.0
         * @since 6.9.0 Added the `$priority` parameter.
         *
         * @param string                      $hook_name Optional. The name of the filter hook. Default empty.
         * @param callable|string|array|false $callback  Optional. The callback to check for.
         *                                               This method can be called unconditionally to speculatively check
         *                                               a callback that may or may not exist. Default false.
         * @param int|false                   $priority  Optional. The specific priority at which to check for the callback.
         *                                               Default false.
         * @return bool|int If `$callback` is omitted, returns boolean for whether the hook has
         *                  anything registered. When checking a specific function, the priority
         *                  of that hook is returned, or false if the function is not attached.
         *                  If `$callback` and `$priority` are both provided, a boolean is returned
         *                  for whether the specific function is registered at that priority.
         */
        public function has_filter($hook_name = '', $callback = \false, $priority = \false)
        {
        }
        /**
         * Checks if any callbacks have been registered for this hook.
         *
         * @since 4.7.0
         *
         * @return bool True if callbacks have been registered for the current hook, otherwise false.
         */
        public function has_filters()
        {
        }
        /**
         * Removes all callbacks from the current filter.
         *
         * @since 4.7.0
         *
         * @param int|false $priority Optional. The priority number to remove. Default false.
         * @phpstan-return void
         */
        public function remove_all_filters($priority = \false)
        {
        }
        /**
         * Calls the callback functions that have been added to a filter hook.
         *
         * @since 4.7.0
         *
         * @param mixed $value The value to filter.
         * @param array $args  Additional parameters to pass to the callback functions.
         *                     This array is expected to include $value at index 0.
         * @return mixed The filtered value after all hooked functions are applied to it.
         */
        public function apply_filters($value, $args)
        {
        }
        /**
         * Calls the callback functions that have been added to an action hook.
         *
         * @since 4.7.0
         *
         * @param array $args Parameters to pass to the callback functions.
         */
        public function do_action($args)
        {
        }
        /**
         * Processes the functions hooked into the 'all' hook.
         *
         * @since 4.7.0
         *
         * @param array $args Arguments to pass to the hook callbacks. Passed by reference.
         */
        public function do_all_hook(&$args)
        {
        }
        /**
         * Return the current priority level of the currently running iteration of the hook.
         *
         * @since 4.7.0
         *
         * @return int|false If the hook is running, return the current priority level.
         *                   If it isn't running, return false.
         */
        public function current_priority()
        {
        }
        /**
         * Normalizes filters set up before WordPress has initialized to WP_Hook objects.
         *
         * The `$filters` parameter should be an array keyed by hook name, with values
         * containing either:
         *
         *  - A `WP_Hook` instance
         *  - An array of callbacks keyed by their priorities
         *
         * Examples:
         *
         *     $filters = array(
         *         'wp_fatal_error_handler_enabled' => array(
         *             10 => array(
         *                 array(
         *                     'accepted_args' => 0,
         *                     'function'      => function() {
         *                         return false;
         *                     },
         *                 ),
         *             ),
         *         ),
         *     );
         *
         * @since 4.7.0
         *
         * @param array $filters Filters to normalize. See documentation above for details.
         * @phpstan-param array<string, WP_Hook|array<int, array<Hook_Callback>>> $filters
         * @return array<string, WP_Hook> Array of normalized filters keyed by hook name.
         */
        public static function build_preinitialized_hooks($filters)
        {
        }
        /**
         * Determines whether an offset value exists.
         *
         * @since 4.7.0
         *
         * @link https://www.php.net/manual/en/arrayaccess.offsetexists.php
         *
         * @param int $offset An offset to check for.
         * @return bool True if the offset exists, false otherwise.
         */
        #[\ReturnTypeWillChange]
        public function offsetExists($offset)
        {
        }
        /**
         * Retrieves a value at a specified offset.
         *
         * @since 4.7.0
         *
         * @link https://www.php.net/manual/en/arrayaccess.offsetget.php
         *
         * @param int $offset The offset to retrieve.
         * @return array|null If set, the value at the specified offset, null otherwise.
         * @phpstan-return array<non-decimal-int-string, Hook_Callback>|null
         */
        #[\ReturnTypeWillChange]
        public function offsetGet($offset)
        {
        }
        /**
         * Sets a value at a specified offset.
         *
         * @since 4.7.0
         *
         * @link https://www.php.net/manual/en/arrayaccess.offsetset.php
         *
         * @param int|null $offset The offset to assign the value to.
         * @param array    $value The value to set.
         * @phpstan-param array<non-decimal-int-string, Hook_Callback> $value
         */
        #[\ReturnTypeWillChange]
        public function offsetSet($offset, $value)
        {
        }
        /**
         * Unsets a specified offset.
         *
         * @since 4.7.0
         *
         * @link https://www.php.net/manual/en/arrayaccess.offsetunset.php
         *
         * @param int $offset The offset to unset.
         */
        #[\ReturnTypeWillChange]
        public function offsetUnset($offset)
        {
        }
        /**
         * Returns the current element.
         *
         * @since 4.7.0
         *
         * @link https://www.php.net/manual/en/iterator.current.php
         *
         * @return array|false Array of callbacks at current priority, false if there are no more elements.
         * @phpstan-return array<non-decimal-int-string, Hook_Callback>|false
         */
        #[\ReturnTypeWillChange]
        public function current()
        {
        }
        /**
         * Moves forward to the next element.
         *
         * @since 4.7.0
         *
         * @link https://www.php.net/manual/en/iterator.next.php
         *
         * @return array|false Array of callbacks at next priority, false if there are no more elements.
         * @phpstan-return array<non-decimal-int-string, Hook_Callback>|false
         */
        #[\ReturnTypeWillChange]
        public function next()
        {
        }
        /**
         * Returns the key of the current element.
         *
         * @since 4.7.0
         *
         * @link https://www.php.net/manual/en/iterator.key.php
         *
         * @return int|null Returns current priority on success, or NULL on failure
         */
        #[\ReturnTypeWillChange]
        public function key()
        {
        }
        /**
         * Checks if current position is valid.
         *
         * @since 4.7.0
         *
         * @link https://www.php.net/manual/en/iterator.valid.php
         *
         * @return bool Whether the current position is valid.
         */
        #[\ReturnTypeWillChange]
        public function valid()
        {
        }
        /**
         * Rewinds the Iterator to the first element.
         *
         * @since 4.7.0
         *
         * @link https://www.php.net/manual/en/iterator.rewind.php
         */
        #[\ReturnTypeWillChange]
        public function rewind()
        {
        }
    }
    /**
     * Core class used to prepare HTTP responses.
     *
     * @since 4.4.0
     */
    #[\AllowDynamicProperties]
    class WP_HTTP_Response
    {
        /**
         * Response data.
         *
         * @since 4.4.0
         * @var mixed
         */
        public $data;
        /**
         * Response headers.
         *
         * @since 4.4.0
         * @var array
         */
        public $headers;
        /**
         * Response status.
         *
         * @since 4.4.0
         * @var int
         */
        public $status;
        /**
         * Constructor.
         *
         * @since 4.4.0
         *
         * @param mixed $data    Response data. Default null.
         * @param int   $status  Optional. HTTP status code. Default 200.
         * @param array $headers Optional. HTTP header map. Default empty array.
         */
        public function __construct($data = \null, $status = 200, $headers = array())
        {
        }
        /**
         * Retrieves headers associated with the response.
         *
         * @since 4.4.0
         *
         * @return array Map of header name to header value.
         */
        public function get_headers()
        {
        }
        /**
         * Sets all header values.
         *
         * @since 4.4.0
         *
         * @param array $headers Map of header name to header value.
         */
        public function set_headers($headers)
        {
        }
        /**
         * Sets a single HTTP header.
         *
         * @since 4.4.0
         *
         * @param string $key     Header name.
         * @param string $value   Header value.
         * @param bool   $replace Optional. Whether to replace an existing header of the same name.
         *                        Default true.
         */
        public function header($key, $value, $replace = \true)
        {
        }
        /**
         * Retrieves the HTTP return code for the response.
         *
         * @since 4.4.0
         *
         * @return int The 3-digit HTTP status code.
         */
        public function get_status()
        {
        }
        /**
         * Sets the 3-digit HTTP status code.
         *
         * @since 4.4.0
         *
         * @param int $code HTTP status.
         */
        public function set_status($code)
        {
        }
        /**
         * Retrieves the response data.
         *
         * @since 4.4.0
         *
         * @return mixed Response data.
         */
        public function get_data()
        {
        }
        /**
         * Sets the response data.
         *
         * @since 4.4.0
         *
         * @param mixed $data Response data.
         */
        public function set_data($data)
        {
        }
        /**
         * Retrieves the response data for JSON serialization.
         *
         * It is expected that in most implementations, this will return the same as get_data(),
         * however this may be different if you want to do custom JSON data handling.
         *
         * @since 4.4.0
         *
         * @return mixed Any JSON-serializable value.
         */
        public function jsonSerialize()
        {
        }
    }
    /**
     * Core class used to store translated data for a locale.
     *
     * @since 2.1.0
     * @since 4.6.0 Moved to its own file from wp-includes/locale.php.
     */
    #[\AllowDynamicProperties]
    class WP_Locale
    {
        /**
         * Stores the translated strings for the full weekday names.
         *
         * @since 2.1.0
         * @since 6.2.0 Initialized to an empty array.
         * @var string[]
         */
        public $weekday = array();
        /**
         * Stores the translated strings for the one character weekday names.
         *
         * There is a hack to make sure that Tuesday and Thursday, as well
         * as Sunday and Saturday, don't conflict. See init() method for more.
         *
         * @see WP_Locale::init() for how to handle the hack.
         *
         * @since 2.1.0
         * @since 6.2.0 Initialized to an empty array.
         * @var string[]
         */
        public $weekday_initial = array();
        /**
         * Stores the translated strings for the abbreviated weekday names.
         *
         * @since 2.1.0
         * @since 6.2.0 Initialized to an empty array.
         * @var string[]
         */
        public $weekday_abbrev = array();
        /**
         * Stores the translated strings for the full month names.
         *
         * @since 2.1.0
         * @since 6.2.0 Initialized to an empty array.
         * @var string[]
         */
        public $month = array();
        /**
         * Stores the translated strings for the month names in genitive case, if the locale specifies.
         *
         * @since 4.4.0
         * @since 6.2.0 Initialized to an empty array.
         * @var string[]
         */
        public $month_genitive = array();
        /**
         * Stores the translated strings for the abbreviated month names.
         *
         * @since 2.1.0
         * @since 6.2.0 Initialized to an empty array.
         * @var string[]
         */
        public $month_abbrev = array();
        /**
         * Stores the translated strings for 'am' and 'pm'.
         *
         * Also the capitalized versions.
         *
         * @since 2.1.0
         * @since 6.2.0 Initialized to an empty array.
         * @var string[]
         */
        public $meridiem = array();
        /**
         * The text direction of the locale language.
         *
         * Default is left to right 'ltr'.
         *
         * @since 2.1.0
         * @var string
         */
        public $text_direction = 'ltr';
        /**
         * The thousands separator and decimal point values used for localizing numbers.
         *
         * @since 2.3.0
         * @since 6.2.0 Initialized to an empty array.
         * @var array
         */
        public $number_format = array();
        /**
         * The separator string used for localizing list item separator.
         *
         * @since 6.0.0
         * @var string
         */
        public $list_item_separator;
        /**
         * The word count type of the locale language.
         *
         * Default is 'words'.
         *
         * @since 6.2.0
         * @var string
         * @phpstan-var 'characters_excluding_spaces'|'characters_including_spaces'|'words'
         */
        public $word_count_type;
        /**
         * Constructor which calls helper methods to set up object variables.
         *
         * @since 2.1.0
         */
        public function __construct()
        {
        }
        /**
         * Sets up the translated strings and object properties.
         *
         * The method creates the translatable strings for various
         * calendar elements. Which allows for specifying locale
         * specific calendar names and text direction.
         *
         * @since 2.1.0
         *
         * @global string $text_direction
         */
        public function init()
        {
        }
        /**
         * Retrieves the full translated weekday word.
         *
         * Week starts on translated Sunday and can be fetched
         * by using 0 (zero). So the week starts with 0 (zero)
         * and ends on Saturday with is fetched by using 6 (six).
         *
         * @since 2.1.0
         *
         * @param int $weekday_number 0 for Sunday through 6 Saturday.
         * @return string Full translated weekday.
         */
        public function get_weekday($weekday_number)
        {
        }
        /**
         * Retrieves the translated weekday initial.
         *
         * The weekday initial is retrieved by the translated
         * full weekday word. When translating the weekday initial
         * pay attention to make sure that the starting letter does
         * not conflict.
         *
         * @since 2.1.0
         *
         * @param string $weekday_name Full translated weekday word.
         * @return string Translated weekday initial.
         */
        public function get_weekday_initial($weekday_name)
        {
        }
        /**
         * Retrieves the translated weekday abbreviation.
         *
         * The weekday abbreviation is retrieved by the translated
         * full weekday word.
         *
         * @since 2.1.0
         *
         * @param string $weekday_name Full translated weekday word.
         * @return string Translated weekday abbreviation.
         */
        public function get_weekday_abbrev($weekday_name)
        {
        }
        /**
         * Retrieves the full translated month by month number.
         *
         * The $month_number parameter has to be a string
         * because it must have the '0' in front of any number
         * that is less than 10. Starts from '01' and ends at
         * '12'.
         *
         * You can use an integer instead and it will add the
         * '0' before the numbers less than 10 for you.
         *
         * @since 2.1.0
         *
         * @param string|int $month_number '01' through '12'.
         * @return string Translated full month name. If the month number is not found, an empty string is returned.
         */
        public function get_month($month_number)
        {
        }
        /**
         * Retrieves translated version of month abbreviation string.
         *
         * The $month_name parameter is expected to be the translated or
         * translatable version of the month.
         *
         * @since 2.1.0
         *
         * @param string $month_name Translated month to get abbreviated version.
         * @return string Translated abbreviated month.
         */
        public function get_month_abbrev($month_name)
        {
        }
        /**
         * Retrieves translated version of month genitive string.
         *
         * The $month_number parameter has to be a string
         * because it must have the '0' in front of any number
         * that is less than 10. Starts from '01' and ends at
         * '12'.
         *
         * You can use an integer instead and it will add the
         * '0' before the numbers less than 10 for you.
         *
         * @since 6.8.0
         *
         * @param string|int $month_number '01' through '12'.
         * @return string Translated genitive month name.
         */
        public function get_month_genitive($month_number)
        {
        }
        /**
         * Retrieves translated version of meridiem string.
         *
         * The $meridiem parameter is expected to not be translated.
         *
         * @since 2.1.0
         *
         * @param string $meridiem Either 'am', 'pm', 'AM', or 'PM'. Not translated version.
         * @return string Translated version
         * @phpstan-param 'am'|'pm'|'AM'|'PM' $meridiem
         */
        public function get_meridiem($meridiem)
        {
        }
        /**
         * Global variables are deprecated.
         *
         * For backward compatibility only.
         *
         * @since 2.1.0
         * @deprecated For backward compatibility only.
         *
         * @global array $weekday
         * @global array $weekday_initial
         * @global array $weekday_abbrev
         * @global array $month
         * @global array $month_abbrev
         */
        public function register_globals()
        {
        }
        /**
         * Checks if current locale is RTL.
         *
         * @since 3.0.0
         * @return bool Whether locale is RTL.
         */
        public function is_rtl()
        {
        }
        /**
         * Registers date/time format strings for general POT.
         *
         * Private, unused method to add some date/time formats translated
         * on wp-admin/options-general.php to the general POT that would
         * otherwise be added to the admin POT.
         *
         * @since 3.6.0
         */
        public function _strings_for_pot()
        {
        }
        /**
         * Retrieves the localized list item separator.
         *
         * @since 6.0.0
         *
         * @return string Localized list item separator.
         */
        public function get_list_item_separator()
        {
        }
        /**
         * Retrieves the localized word count type.
         *
         * @since 6.2.0
         *
         * @return string Localized word count type. Possible values are `characters_excluding_spaces`,
         *                `characters_including_spaces`, or `words`. Defaults to `words`.
         * @phpstan-return 'characters_excluding_spaces'|'characters_including_spaces'|'words'
         */
        public function get_word_count_type()
        {
        }
    }
    /**
     * Core class that implements an object cache.
     *
     * The WordPress Object Cache is used to save on trips to the database. The
     * Object Cache stores all of the cache data to memory and makes the cache
     * contents available by using a key, which is used to name and later retrieve
     * the cache contents.
     *
     * The Object Cache can be replaced by other caching mechanisms by placing files
     * in the wp-content folder which is looked at in wp-settings. If that file
     * exists, then this file will not be included.
     *
     * @since 2.0.0
     */
    #[\AllowDynamicProperties]
    class WP_Object_Cache
    {
        /**
         * The amount of times the cache data was already stored in the cache.
         *
         * @since 2.5.0
         * @var int
         */
        public $cache_hits = 0;
        /**
         * Amount of times the cache did not have the request in cache.
         *
         * @since 2.0.0
         * @var int
         */
        public $cache_misses = 0;
        /**
         * List of global cache groups.
         *
         * @since 3.0.0
         * @var string[]
         */
        protected $global_groups = array();
        /**
         * Sets up object properties.
         *
         * @since 2.0.8
         */
        public function __construct()
        {
        }
        /**
         * Makes private properties readable for backward compatibility.
         *
         * @since 4.0.0
         *
         * @param string $name Property to get.
         * @return mixed Property.
         */
        public function __get($name)
        {
        }
        /**
         * Makes private properties settable for backward compatibility.
         *
         * @since 4.0.0
         *
         * @param string $name  Property to set.
         * @param mixed  $value Property value.
         */
        public function __set($name, $value)
        {
        }
        /**
         * Makes private properties checkable for backward compatibility.
         *
         * @since 4.0.0
         *
         * @param string $name Property to check if set.
         * @return bool Whether the property is set.
         */
        public function __isset($name)
        {
        }
        /**
         * Makes private properties un-settable for backward compatibility.
         *
         * @since 4.0.0
         *
         * @param string $name Property to unset.
         */
        public function __unset($name)
        {
        }
        /**
         * Serves as a utility function to determine whether a key is valid.
         *
         * @since 6.1.0
         *
         * @param int|string $key Cache key to check for validity.
         * @return bool Whether the key is valid.
         */
        protected function is_valid_key($key)
        {
        }
        /**
         * Serves as a utility function to determine whether a key exists in the cache.
         *
         * @since 3.4.0
         *
         * @param int|string $key   Cache key to check for existence.
         * @param string     $group Cache group for the key existence check.
         * @return bool Whether the key exists in the cache for the given group.
         */
        protected function _exists($key, $group)
        {
        }
        /**
         * Adds data to the cache if it doesn't already exist.
         *
         * @since 2.0.0
         *
         * @uses WP_Object_Cache::_exists() Checks to see if the cache already has data.
         * @uses WP_Object_Cache::set()     Sets the data after the checking the cache
         *                                  contents existence.
         *
         * @param int|string $key    What to call the contents in the cache.
         * @param mixed      $data   The contents to store in the cache.
         * @param string     $group  Optional. Where to group the cache contents. Default 'default'.
         * @param int        $expire Optional. When to expire the cache contents, in seconds.
         *                           Default 0 (no expiration).
         * @return bool True on success, false if cache key and group already exist.
         */
        public function add($key, $data, $group = 'default', $expire = 0)
        {
        }
        /**
         * Adds multiple values to the cache in one call.
         *
         * @since 6.0.0
         *
         * @param array  $data   Array of keys and values to be added.
         * @param string $group  Optional. Where the cache contents are grouped. Default empty.
         * @param int    $expire Optional. When to expire the cache contents, in seconds.
         *                       Default 0 (no expiration).
         * @return bool[] Array of return values, grouped by key. Each value is either
         *                true on success, or false if cache key and group already exist.
         */
        public function add_multiple(array $data, $group = '', $expire = 0)
        {
        }
        /**
         * Replaces the contents in the cache, if contents already exist.
         *
         * @since 2.0.0
         *
         * @see WP_Object_Cache::set()
         *
         * @param int|string $key    What to call the contents in the cache.
         * @param mixed      $data   The contents to store in the cache.
         * @param string     $group  Optional. Where to group the cache contents. Default 'default'.
         * @param int        $expire Optional. When to expire the cache contents, in seconds.
         *                           Default 0 (no expiration).
         * @return bool True if contents were replaced, false if original value does not exist.
         */
        public function replace($key, $data, $group = 'default', $expire = 0)
        {
        }
        /**
         * Sets the data contents into the cache.
         *
         * The cache contents are grouped by the $group parameter followed by the
         * $key. This allows for duplicate IDs in unique groups. Therefore, naming of
         * the group should be used with care and should follow normal function
         * naming guidelines outside of core WordPress usage.
         *
         * The $expire parameter is not used, because the cache will automatically
         * expire for each time a page is accessed and PHP finishes. The method is
         * more for cache plugins which use files.
         *
         * @since 2.0.0
         * @since 6.1.0 Returns false if cache key is invalid.
         *
         * @param int|string $key    What to call the contents in the cache.
         * @param mixed      $data   The contents to store in the cache.
         * @param string     $group  Optional. Where to group the cache contents. Default 'default'.
         * @param int        $expire Optional. Not used.
         * @return bool True if contents were set, false if key is invalid.
         */
        public function set($key, $data, $group = 'default', $expire = 0)
        {
        }
        /**
         * Sets multiple values to the cache in one call.
         *
         * @since 6.0.0
         *
         * @param array  $data   Array of key and value to be set.
         * @param string $group  Optional. Where the cache contents are grouped. Default empty.
         * @param int    $expire Optional. When to expire the cache contents, in seconds.
         *                       Default 0 (no expiration).
         * @return bool[] Array of return values, grouped by key. Each value is always true.
         */
        public function set_multiple(array $data, $group = '', $expire = 0)
        {
        }
        /**
         * Retrieves the cache contents, if it exists.
         *
         * The contents will be first attempted to be retrieved by searching by the
         * key in the cache group. If the cache is hit (success) then the contents
         * are returned.
         *
         * On failure, the number of cache misses will be incremented.
         *
         * @since 2.0.0
         *
         * @param int|string $key   The key under which the cache contents are stored.
         * @param string     $group Optional. Where the cache contents are grouped. Default 'default'.
         * @param bool       $force Optional. Unused. Whether to force an update of the local cache
         *                          from the persistent cache. Default false.
         * @param bool|null  $found Optional. Whether the key was found in the cache (passed by reference).
         *                          Disambiguates a return of false, a storable value. Default null.
         * @return mixed|false The cache contents on success, false on failure to retrieve contents.
         */
        public function get($key, $group = 'default', $force = \false, &$found = \null)
        {
        }
        /**
         * Retrieves multiple values from the cache in one call.
         *
         * @since 5.5.0
         *
         * @param array  $keys  Array of keys under which the cache contents are stored.
         * @param string $group Optional. Where the cache contents are grouped. Default 'default'.
         * @param bool   $force Optional. Whether to force an update of the local cache
         *                      from the persistent cache. Default false.
         * @return array Array of return values, grouped by key. Each value is either
         *               the cache contents on success, or false on failure.
         */
        public function get_multiple($keys, $group = 'default', $force = \false)
        {
        }
        /**
         * Removes the contents of the cache key in the group.
         *
         * If the cache key does not exist in the group, then nothing will happen.
         *
         * @since 2.0.0
         *
         * @param int|string $key        What the contents in the cache are called.
         * @param string     $group      Optional. Where the cache contents are grouped. Default 'default'.
         * @param bool       $deprecated Optional. Unused. Default false.
         * @return bool True on success, false if the contents were not deleted.
         * @phpstan-param false $deprecated
         */
        public function delete($key, $group = 'default', $deprecated = \false)
        {
        }
        /**
         * Deletes multiple values from the cache in one call.
         *
         * @since 6.0.0
         *
         * @param array  $keys  Array of keys to be deleted.
         * @param string $group Optional. Where the cache contents are grouped. Default empty.
         * @return bool[] Array of return values, grouped by key. Each value is either
         *                true on success, or false if the contents were not deleted.
         */
        public function delete_multiple(array $keys, $group = '')
        {
        }
        /**
         * Increments numeric cache item's value.
         *
         * @since 3.3.0
         *
         * @param int|string $key    The cache key to increment.
         * @param int        $offset Optional. The amount by which to increment the item's value.
         *                           Default 1.
         * @param string     $group  Optional. The group the key is in. Default 'default'.
         * @return int|false The item's new value on success, false on failure.
         */
        public function incr($key, $offset = 1, $group = 'default')
        {
        }
        /**
         * Decrements numeric cache item's value.
         *
         * @since 3.3.0
         *
         * @param int|string $key    The cache key to decrement.
         * @param int        $offset Optional. The amount by which to decrement the item's value.
         *                           Default 1.
         * @param string     $group  Optional. The group the key is in. Default 'default'.
         * @return int|false The item's new value on success, false on failure.
         */
        public function decr($key, $offset = 1, $group = 'default')
        {
        }
        /**
         * Clears the object cache of all data.
         *
         * @since 2.0.0
         *
         * @return true Always returns true.
         */
        public function flush()
        {
        }
        /**
         * Removes all cache items in a group.
         *
         * @since 6.1.0
         *
         * @param string $group Name of group to remove from cache.
         * @return true Always returns true.
         */
        public function flush_group($group)
        {
        }
        /**
         * Sets the list of global cache groups.
         *
         * @since 3.0.0
         *
         * @param string|string[] $groups List of groups that are global.
         */
        public function add_global_groups($groups)
        {
        }
        /**
         * Switches the internal blog ID.
         *
         * This changes the blog ID used to create keys in blog specific groups.
         *
         * @since 3.5.0
         *
         * @param int $blog_id Blog ID.
         */
        public function switch_to_blog($blog_id)
        {
        }
        /**
         * Resets cache keys.
         *
         * @since 3.0.0
         *
         * @deprecated 3.5.0 Use WP_Object_Cache::switch_to_blog()
         * @see switch_to_blog()
         */
        public function reset()
        {
        }
        /**
         * Echoes the stats of the caching.
         *
         * Gives the cache hits, and cache misses. Also prints every cached group,
         * key and the data.
         *
         * @since 2.0.0
         */
        public function stats()
        {
        }
    }
    /**
     * Core class used for interacting with post types.
     *
     * @since 4.6.0
     *
     * @see register_post_type()
     */
    #[\AllowDynamicProperties]
    final class WP_Post_Type
    {
        /**
         * Post type key.
         *
         * @since 4.6.0
         * @var string $name
         */
        public $name;
        /**
         * Name of the post type shown in the menu. Usually plural.
         *
         * @since 4.6.0
         * @var string $label
         */
        public $label;
        /**
         * Labels object for this post type.
         *
         * If not set, post labels are inherited for non-hierarchical types
         * and page labels for hierarchical ones.
         *
         * @see get_post_type_labels()
         *
         * @since 4.6.0
         * @var stdClass $labels
         */
        public $labels;
        /**
         * A short descriptive summary of what the post type is.
         *
         * Default empty.
         *
         * @since 4.6.0
         * @var string $description
         */
        public $description = '';
        /**
         * Whether a post type is intended for use publicly either via the admin interface or by front-end users.
         *
         * While the default settings of $exclude_from_search, $publicly_queryable, $show_ui, and $show_in_nav_menus
         * are inherited from public, each does not rely on this relationship and controls a very specific intention.
         *
         * Default false.
         *
         * @since 4.6.0
         * @var bool $public
         */
        public $public = \false;
        /**
         * Whether the post type is hierarchical (e.g. page).
         *
         * Default false.
         *
         * @since 4.6.0
         * @var bool $hierarchical
         */
        public $hierarchical = \false;
        /**
         * Whether to exclude posts with this post type from front end search
         * results.
         *
         * Default is the opposite value of $public.
         *
         * @since 4.6.0
         * @var bool $exclude_from_search
         */
        public $exclude_from_search = \null;
        /**
         * Whether queries can be performed on the front end for the post type as part of `parse_request()`.
         *
         * Endpoints would include:
         *
         * - `?post_type={post_type_key}`
         * - `?{post_type_key}={single_post_slug}`
         * - `?{post_type_query_var}={single_post_slug}`
         *
         * Default is the value of $public.
         *
         * @since 4.6.0
         * @var bool $publicly_queryable
         */
        public $publicly_queryable = \null;
        /**
         * Whether this post type is embeddable.
         *
         * Default is the value of $public.
         *
         * @since 6.8.0
         * @var bool $embeddable
         */
        public $embeddable = \null;
        /**
         * Whether to generate and allow a UI for managing this post type in the admin.
         *
         * Default is the value of $public.
         *
         * @since 4.6.0
         * @var bool $show_ui
         */
        public $show_ui = \null;
        /**
         * Where to show the post type in the admin menu.
         *
         * To work, $show_ui must be true. If true, the post type is shown in its own top level menu. If false, no menu is
         * shown. If a string of an existing top level menu ('tools.php' or 'edit.php?post_type=page', for example), the
         * post type will be placed as a sub-menu of that.
         *
         * Default is the value of $show_ui.
         *
         * @since 4.6.0
         * @var bool|string $show_in_menu
         */
        public $show_in_menu = \null;
        /**
         * Makes this post type available for selection in navigation menus.
         *
         * Default is the value $public.
         *
         * @since 4.6.0
         * @var bool $show_in_nav_menus
         */
        public $show_in_nav_menus = \null;
        /**
         * Makes this post type available via the admin bar.
         *
         * Default is the value of $show_in_menu.
         *
         * @since 4.6.0
         * @var bool $show_in_admin_bar
         */
        public $show_in_admin_bar = \null;
        /**
         * The position in the menu order the post type should appear.
         *
         * To work, $show_in_menu must be true. Default null (at the bottom).
         *
         * @since 4.6.0
         * @var int $menu_position
         */
        public $menu_position = \null;
        /**
         * The URL or reference to the icon to be used for this menu.
         *
         * Pass a base64-encoded SVG using a data URI, which will be colored to match the color scheme.
         * This should begin with 'data:image/svg+xml;base64,'. Pass the name of a Dashicons helper class
         * to use a font icon, e.g. 'dashicons-chart-pie'. Pass 'none' to leave div.wp-menu-image empty
         * so an icon can be added via CSS.
         *
         * Defaults to use the posts icon.
         *
         * @since 4.6.0
         * @var string $menu_icon
         */
        public $menu_icon = \null;
        /**
         * The string to use to build the read, edit, and delete capabilities.
         *
         * May be passed as an array to allow for alternative plurals when using
         * this argument as a base to construct the capabilities, e.g.
         * array( 'story', 'stories' ). Default 'post'.
         *
         * @since 4.6.0
         * @var string $capability_type
         */
        public $capability_type = 'post';
        /**
         * Whether to use the internal default meta capability handling.
         *
         * Default false.
         *
         * @since 4.6.0
         * @var bool $map_meta_cap
         */
        public $map_meta_cap = \false;
        /**
         * Provide a callback function that sets up the meta boxes for the edit form.
         *
         * Do `remove_meta_box()` and `add_meta_box()` calls in the callback. Default null.
         *
         * @since 4.6.0
         * @var callable $register_meta_box_cb
         */
        public $register_meta_box_cb = \null;
        /**
         * An array of taxonomy identifiers that will be registered for the post type.
         *
         * Taxonomies can be registered later with `register_taxonomy()` or `register_taxonomy_for_object_type()`.
         *
         * Default empty array.
         *
         * @since 4.6.0
         * @var string[] $taxonomies
         */
        public $taxonomies = array();
        /**
         * Whether there should be post type archives, or if a string, the archive slug to use.
         *
         * Will generate the proper rewrite rules if $rewrite is enabled. Default false.
         *
         * @since 4.6.0
         * @var bool|string $has_archive
         */
        public $has_archive = \false;
        /**
         * Sets the query_var key for this post type.
         *
         * Defaults to $post_type key. If false, a post type cannot be loaded at `?{query_var}={post_slug}`.
         * If specified as a string, the query `?{query_var_string}={post_slug}` will be valid.
         *
         * @since 4.6.0
         * @var string|bool $query_var
         */
        public $query_var;
        /**
         * Whether to allow this post type to be exported.
         *
         * Default true.
         *
         * @since 4.6.0
         * @var bool $can_export
         */
        public $can_export = \true;
        /**
         * Whether to delete posts of this type when deleting a user.
         *
         * - If true, posts of this type belonging to the user will be moved to Trash when the user is deleted.
         * - If false, posts of this type belonging to the user will *not* be trashed or deleted.
         * - If not set (the default), posts are trashed if post type supports the 'author' feature.
         *   Otherwise posts are not trashed or deleted.
         *
         * Default null.
         *
         * @since 4.6.0
         * @var bool $delete_with_user
         */
        public $delete_with_user = \null;
        /**
         * Array of blocks to use as the default initial state for an editor session.
         *
         * Each item should be an array containing block name and optional attributes.
         *
         * Default empty array.
         *
         * @link https://developer.wordpress.org/block-editor/developers/block-api/block-templates/
         *
         * @since 5.0.0
         * @var array[] $template
         */
        public $template = array();
        /**
         * Whether the block template should be locked if $template is set.
         *
         * - If set to 'all', the user is unable to insert new blocks, move existing blocks
         *   and delete blocks.
         * - If set to 'insert', the user is able to move existing blocks but is unable to insert
         *   new blocks and delete blocks.
         * - If set to 'contentOnly', the user is only able to edit the content of existing blocks.
         *
         * Default false.
         *
         * @link https://developer.wordpress.org/block-editor/developers/block-api/block-templates/
         *
         * @since 5.0.0
         * @var string|false $template_lock
         */
        public $template_lock = \false;
        /**
         * Whether this post type is a native or "built-in" post_type.
         *
         * Default false.
         *
         * @since 4.6.0
         * @var bool $_builtin
         */
        public $_builtin = \false;
        /**
         * URL segment to use for edit link of this post type.
         *
         * Default 'post.php?post=%d'.
         *
         * @since 4.6.0
         * @var string $_edit_link
         */
        public $_edit_link = 'post.php?post=%d';
        /**
         * Post type capabilities.
         *
         * @since 4.6.0
         * @var stdClass $cap
         */
        public $cap;
        /**
         * Triggers the handling of rewrites for this post type.
         *
         * Defaults to true, using $post_type as slug.
         *
         * @since 4.6.0
         * @var array|false $rewrite
         */
        public $rewrite;
        /**
         * The features supported by the post type.
         *
         * @since 4.6.0
         * @var array|bool $supports
         */
        public $supports;
        /**
         * Whether this post type should appear in the REST API.
         *
         * Default false. If true, standard endpoints will be registered with
         * respect to $rest_base and $rest_controller_class.
         *
         * @since 4.7.4
         * @var bool $show_in_rest
         */
        public $show_in_rest;
        /**
         * The base path for this post type's REST API endpoints.
         *
         * @since 4.7.4
         * @var string|bool $rest_base
         */
        public $rest_base;
        /**
         * The namespace for this post type's REST API endpoints.
         *
         * @since 5.9.0
         * @var string|bool $rest_namespace
         */
        public $rest_namespace;
        /**
         * The controller for this post type's REST API endpoints.
         *
         * Custom controllers must extend WP_REST_Controller.
         *
         * @since 4.7.4
         * @var string|bool $rest_controller_class
         */
        public $rest_controller_class;
        /**
         * The controller instance for this post type's REST API endpoints.
         *
         * Lazily computed. Should be accessed using {@see WP_Post_Type::get_rest_controller()}.
         *
         * @since 5.3.0
         * @var WP_REST_Controller $rest_controller
         */
        public $rest_controller;
        /**
         * The controller for this post type's revisions REST API endpoints.
         *
         * Custom controllers must extend WP_REST_Controller.
         *
         * @since 6.4.0
         * @var string|bool $revisions_rest_controller_class
         */
        public $revisions_rest_controller_class;
        /**
         * The controller instance for this post type's revisions REST API endpoints.
         *
         * Lazily computed. Should be accessed using {@see WP_Post_Type::get_revisions_rest_controller()}.
         *
         * @since 6.4.0
         * @var WP_REST_Controller $revisions_rest_controller
         */
        public $revisions_rest_controller;
        /**
         * The controller for this post type's autosave REST API endpoints.
         *
         * Custom controllers must extend WP_REST_Controller.
         *
         * @since 6.4.0
         * @var string|bool $autosave_rest_controller_class
         */
        public $autosave_rest_controller_class;
        /**
         * The controller instance for this post type's autosave REST API endpoints.
         *
         * Lazily computed. Should be accessed using {@see WP_Post_Type::get_autosave_rest_controller()}.
         *
         * @since 6.4.0
         * @var WP_REST_Controller $autosave_rest_controller
         */
        public $autosave_rest_controller;
        /**
         * A flag to register the post type REST API controller after its associated autosave / revisions controllers, instead of before. Registration order affects route matching priority.
         *
         * @since 6.4.0
         * @var bool $late_route_registration
         */
        public $late_route_registration;
        /**
         * Constructor.
         *
         * See the register_post_type() function for accepted arguments for `$args`.
         *
         * Will populate object properties from the provided arguments and assign other
         * default properties based on that information.
         *
         * @since 4.6.0
         *
         * @see register_post_type()
         *
         * @param string       $post_type Post type key.
         * @param array|string $args      Optional. Array or string of arguments for registering a post type.
         *                                See register_post_type() for information on accepted arguments.
         *                                Default empty array.
         * @phpstan-param array{
         *   label?: string,
         *   labels?: string[],
         *   description?: string,
         *   public?: bool,
         *   hierarchical?: bool,
         *   exclude_from_search?: bool,
         *   publicly_queryable?: bool,
         *   show_ui?: bool,
         *   show_in_menu?: bool|string,
         *   show_in_nav_menus?: bool,
         *   show_in_admin_bar?: bool,
         *   show_in_rest?: bool,
         *   rest_base?: string,
         *   rest_namespace?: string,
         *   rest_controller_class?: string,
         *   autosave_rest_controller_class?: string|bool,
         *   revisions_rest_controller_class?: string|bool,
         *   late_route_registration?: bool,
         *   menu_position?: int,
         *   menu_icon?: string,
         *   capability_type?: string|array,
         *   capabilities?: string[],
         *   map_meta_cap?: bool,
         *   supports?: array|false,
         *   register_meta_box_cb?: callable,
         *   taxonomies?: string[],
         *   has_archive?: bool|string,
         *   rewrite?: bool|array{
         *     slug?: string,
         *     with_front?: bool,
         *     feeds?: bool,
         *     pages?: bool,
         *     ep_mask?: int,
         *   },
         *   query_var?: string|bool,
         *   can_export?: bool,
         *   delete_with_user?: bool,
         *   template?: array,
         *   template_lock?: string|false,
         *   _builtin?: bool,
         *   _edit_link?: string,
         * } $args See register_post_type()
         */
        public function __construct($post_type, $args = array())
        {
        }
        /**
         * Sets post type properties.
         *
         * See the register_post_type() function for accepted arguments for `$args`.
         *
         * @since 4.6.0
         *
         * @param array|string $args Array or string of arguments for registering a post type.
         */
        public function set_props($args)
        {
        }
        /**
         * Sets the features support for the post type.
         *
         * @since 4.6.0
         */
        public function add_supports()
        {
        }
        /**
         * Adds the necessary rewrite rules for the post type.
         *
         * @since 4.6.0
         *
         * @global WP_Rewrite $wp_rewrite WordPress rewrite component.
         * @global WP         $wp         Current WordPress environment instance.
         */
        public function add_rewrite_rules()
        {
        }
        /**
         * Registers the post type meta box if a custom callback was specified.
         *
         * @since 4.6.0
         */
        public function register_meta_boxes()
        {
        }
        /**
         * Adds the future post hook action for the post type.
         *
         * @since 4.6.0
         */
        public function add_hooks()
        {
        }
        /**
         * Registers the taxonomies for the post type.
         *
         * @since 4.6.0
         */
        public function register_taxonomies()
        {
        }
        /**
         * Removes the features support for the post type.
         *
         * @since 4.6.0
         *
         * @global array $_wp_post_type_features Post type features.
         */
        public function remove_supports()
        {
        }
        /**
         * Removes any rewrite rules, permastructs, and rules for the post type.
         *
         * @since 4.6.0
         *
         * @global WP_Rewrite $wp_rewrite          WordPress rewrite component.
         * @global WP         $wp                  Current WordPress environment instance.
         * @global array      $post_type_meta_caps Used to remove meta capabilities.
         */
        public function remove_rewrite_rules()
        {
        }
        /**
         * Unregisters the post type meta box if a custom callback was specified.
         *
         * @since 4.6.0
         */
        public function unregister_meta_boxes()
        {
        }
        /**
         * Removes the post type from all taxonomies.
         *
         * @since 4.6.0
         */
        public function unregister_taxonomies()
        {
        }
        /**
         * Removes the future post hook action for the post type.
         *
         * @since 4.6.0
         */
        public function remove_hooks()
        {
        }
        /**
         * Gets the REST API controller for this post type.
         *
         * Will only instantiate the controller class once per request.
         *
         * @since 5.3.0
         *
         * @return WP_REST_Controller|null The controller instance, or null if the post type
         *                                 is set not to show in rest.
         */
        public function get_rest_controller()
        {
        }
        /**
         * Gets the REST API revisions controller for this post type.
         *
         * Will only instantiate the controller class once per request.
         *
         * @since 6.4.0
         *
         * @return WP_REST_Controller|null The controller instance, or null if the post type
         *                                 is set not to show in rest.
         */
        public function get_revisions_rest_controller()
        {
        }
        /**
         * Gets the REST API autosave controller for this post type.
         *
         * Will only instantiate the controller class once per request.
         *
         * @since 6.4.0
         *
         * @return WP_REST_Controller|null The controller instance, or null if the post type
         *                                 is set not to show in rest.
         */
        public function get_autosave_rest_controller()
        {
        }
        /**
         * Returns the default labels for post types.
         *
         * @since 6.0.0
         *
         * @return (string|null)[][] The default labels for post types.
         */
        public static function get_default_labels()
        {
        }
        /**
         * Resets the cache for the default labels.
         *
         * @since 6.0.0
         */
        public static function reset_default_labels()
        {
        }
    }
    /**
     * Core class used to implement the WP_Post object.
     *
     * @since 3.5.0
     *
     * @property string $page_template
     *
     * @property-read list<non-negative-int> $ancestors
     * @property-read list<non-negative-int> $post_category
     * @property-read list<non-empty-string> $tags_input
     *
     * @phpstan-type Data_Array array{
     *     ID: non-negative-int,
     *     post_author: numeric-string|'',
     *     post_date: string,
     *     post_date_gmt: string,
     *     post_content: string,
     *     post_title: string,
     *     post_excerpt: string,
     *     post_status: non-empty-string,
     *     comment_status: non-empty-string,
     *     ping_status: non-empty-string,
     *     post_password: string,
     *     post_name: string,
     *     to_ping: string,
     *     pinged: string,
     *     post_modified: string,
     *     post_modified_gmt: string,
     *     post_content_filtered: string,
     *     post_parent: non-negative-int,
     *     guid: string,
     *     menu_order: int,
     *     post_type: non-empty-string,
     *     post_mime_type: string,
     *     comment_count: numeric-string,
     *     filter: 'raw'|'edit'|'db'|'display'|'attribute'|'js'|'sample'|null,
     *     ancestors: list<non-negative-int>,
     *     page_template: string,
     *     post_category: list<non-negative-int>,
     *     tags_input: list<non-empty-string>,
     *     ...
     * }
     */
    #[\AllowDynamicProperties]
    final class WP_Post
    {
        /**
         * Post ID.
         *
         * @since 3.5.0
         * @var int
         * @phpstan-var non-negative-int
         */
        public $ID;
        /**
         * ID of post author.
         *
         * A numeric string, for compatibility reasons. May be an empty string for a
         * default post that has not yet been assigned an author.
         *
         * @since 3.5.0
         * @var string
         * @phpstan-var numeric-string|''
         */
        public $post_author = '0';
        /**
         * The post's local publication time.
         *
         * @since 3.5.0
         * @var string
         */
        public $post_date = '0000-00-00 00:00:00';
        /**
         * The post's GMT publication time.
         *
         * @since 3.5.0
         * @var string
         */
        public $post_date_gmt = '0000-00-00 00:00:00';
        /**
         * The post's content.
         *
         * @since 3.5.0
         * @var string
         */
        public $post_content = '';
        /**
         * The post's title.
         *
         * @since 3.5.0
         * @var string
         */
        public $post_title = '';
        /**
         * The post's excerpt.
         *
         * @since 3.5.0
         * @var string
         */
        public $post_excerpt = '';
        /**
         * The post's status.
         *
         * @since 3.5.0
         * @var string
         * @phpstan-var non-empty-string
         */
        public $post_status = 'publish';
        /**
         * Whether comments are allowed.
         *
         * @since 3.5.0
         * @var string
         * @phpstan-var non-empty-string
         */
        public $comment_status = 'open';
        /**
         * Whether pings are allowed.
         *
         * @since 3.5.0
         * @var string
         * @phpstan-var non-empty-string
         */
        public $ping_status = 'open';
        /**
         * The post's password in plain text.
         *
         * @since 3.5.0
         * @var string
         */
        public $post_password = '';
        /**
         * The post's slug.
         *
         * @since 3.5.0
         * @var string
         */
        public $post_name = '';
        /**
         * URLs queued to be pinged.
         *
         * @since 3.5.0
         * @var string
         */
        public $to_ping = '';
        /**
         * URLs that have been pinged.
         *
         * @since 3.5.0
         * @var string
         */
        public $pinged = '';
        /**
         * The post's local modified time.
         *
         * @since 3.5.0
         * @var string
         */
        public $post_modified = '0000-00-00 00:00:00';
        /**
         * The post's GMT modified time.
         *
         * @since 3.5.0
         * @var string
         */
        public $post_modified_gmt = '0000-00-00 00:00:00';
        /**
         * A utility DB field for post content.
         *
         * @since 3.5.0
         * @var string
         */
        public $post_content_filtered = '';
        /**
         * ID of a post's parent post.
         *
         * @since 3.5.0
         * @var int
         * @phpstan-var non-negative-int
         */
        public $post_parent = 0;
        /**
         * The unique identifier for a post, not necessarily a URL, used as the feed GUID.
         *
         * @since 3.5.0
         * @var string
         */
        public $guid = '';
        /**
         * A field used for ordering posts.
         *
         * @since 3.5.0
         * @var int
         */
        public $menu_order = 0;
        /**
         * The post's type, like post or page.
         *
         * @since 3.5.0
         * @var string
         * @phpstan-var non-empty-string
         */
        public $post_type = 'post';
        /**
         * An attachment's mime type.
         *
         * @since 3.5.0
         * @var string
         */
        public $post_mime_type = '';
        /**
         * Cached comment count.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 3.5.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $comment_count = '0';
        /**
         * Stores the post object's sanitization level.
         *
         * Does not correspond to a DB field.
         *
         * The 'sample' value is set exclusively by {@see get_sample_permalink()} and is read during permalink previewing.
         *
         * @since 3.5.0
         * @var string|null
         * @phpstan-var 'raw'|'edit'|'db'|'display'|'attribute'|'js'|'sample'|null
         */
        public $filter;
        /**
         * Retrieve WP_Post instance.
         *
         * @since 3.5.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param int $post_id Post ID.
         * @return WP_Post|false Post object, false otherwise.
         *
         * @phpstan-param int|numeric-string $post_id
         */
        public static function get_instance($post_id)
        {
        }
        /**
         * Constructor.
         *
         * @since 3.5.0
         *
         * @param object $post Post object.
         */
        public function __construct($post)
        {
        }
        /**
         * Isset-er.
         *
         * @since 3.5.0
         *
         * @param string $key Property to check if set.
         * @return bool
         */
        public function __isset($key)
        {
        }
        /**
         * Getter.
         *
         * @since 3.5.0
         *
         * @param string $key Key to get.
         * @return mixed
         */
        public function __get($key)
        {
        }
        /**
         * Applies the provided context filter for the current post.
         *
         * If the requested filter was already applied, then it returns without any changes.
         *
         * If the 'raw' filter is supplied, then a new instance of the post is obtained and this method _may_ return false
         * in case the underlying post was deleted.
         *
         * @since 3.5.0
         *
         * @param string $filter Filter.
         * @return WP_Post|false
         *
         * @phpstan-param 'raw'|'edit'|'db'|'display'|'attribute'|'js' $filter
         * @phpstan-return (
         *     $filter is 'raw' ? WP_Post|false : WP_Post
         * )
         */
        public function filter($filter)
        {
        }
        /**
         * Convert object to array.
         *
         * @since 3.5.0
         *
         * @return array<string, mixed> Object as array.
         *
         * @phpstan-return Data_Array
         */
        public function to_array()
        {
        }
    }
    /**
     * The WordPress Query class.
     *
     * @link https://developer.wordpress.org/reference/classes/wp_query/
     *
     * @since 1.5.0
     * @since 4.5.0 Removed the `$comments_popup` property.
     * @phpstan-property-read bool $query_vars_changed
     * @phpstan-property-read bool|string $query_vars_hash
     * @phpstan-method void init_query_flags()
     */
    #[\AllowDynamicProperties]
    class WP_Query
    {
        /**
         * Query vars set by the user.
         *
         * @since 1.5.0
         * @var ?array
         */
        public $query;
        /**
         * Query vars, after parsing.
         *
         * @since 1.5.0
         * @var array
         */
        public $query_vars = array();
        /**
         * Taxonomy query, as passed to get_tax_sql().
         *
         * @since 3.1.0
         * @var WP_Tax_Query|null A taxonomy query instance.
         */
        public $tax_query;
        /**
         * Metadata query container.
         *
         * @since 3.2.0
         * @var WP_Meta_Query A meta query instance.
         */
        public $meta_query = \false;
        /**
         * Date query container.
         *
         * @since 3.7.0
         * @var WP_Date_Query A date query instance.
         */
        public $date_query = \false;
        /**
         * Holds the data for a single object that is queried.
         *
         * Holds the contents of a post, page, category, attachment.
         *
         * @since 1.5.0
         * @var WP_Term|WP_Post_Type|WP_Post|WP_User|null
         */
        public $queried_object;
        /**
         * The ID of the queried object.
         *
         * @since 1.5.0
         * @var ?int
         */
        public $queried_object_id;
        /**
         * SQL for the database query.
         *
         * @since 2.0.1
         * @var ?string
         */
        public $request;
        /**
         * Array of post objects or post IDs.
         *
         * @since 1.5.0
         * @var WP_Post[]|int[]|null
         */
        public $posts;
        /**
         * The number of posts for the current query.
         *
         * @since 1.5.0
         * @var int
         */
        public $post_count = 0;
        /**
         * Index of the current item in the loop.
         *
         * @since 1.5.0
         * @var int
         */
        public $current_post = -1;
        /**
         * Whether the caller is before the loop.
         *
         * @since 6.3.0
         * @var bool
         */
        public $before_loop = \true;
        /**
         * Whether the loop has started and the caller is in the loop.
         *
         * @since 2.0.0
         * @var bool
         */
        public $in_the_loop = \false;
        /**
         * The current post.
         *
         * This property does not get populated when the `fields` argument is set to
         * `ids` or `id=>parent`.
         *
         * @since 1.5.0
         * @var WP_Post|null
         */
        public $post;
        /**
         * The list of comments for current post.
         *
         * @since 2.2.0
         * @var ?WP_Comment[]
         */
        public $comments;
        /**
         * The number of comments for the posts.
         *
         * @since 2.2.0
         * @var int
         */
        public $comment_count = 0;
        /**
         * The index of the comment in the comment loop.
         *
         * @since 2.2.0
         * @var int
         */
        public $current_comment = -1;
        /**
         * Current comment object.
         *
         * @since 2.2.0
         * @var ?WP_Comment
         */
        public $comment;
        /**
         * The number of found posts for the current query.
         *
         * If limit clause was not used, equals $post_count.
         *
         * @since 2.1.0
         * @var int
         */
        public $found_posts = 0;
        /**
         * The number of pages.
         *
         * @since 2.1.0
         * @var int
         */
        public $max_num_pages = 0;
        /**
         * The number of comment pages.
         *
         * @since 2.7.0
         * @var int
         */
        public $max_num_comment_pages = 0;
        /**
         * Signifies whether the current query is for a single post.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_single = \false;
        /**
         * Signifies whether the current query is for a preview.
         *
         * @since 2.0.0
         * @var bool
         */
        public $is_preview = \false;
        /**
         * Signifies whether the current query is for a page.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_page = \false;
        /**
         * Signifies whether the current query is for an archive.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_archive = \false;
        /**
         * Signifies whether the current query is for a date archive.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_date = \false;
        /**
         * Signifies whether the current query is for a year archive.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_year = \false;
        /**
         * Signifies whether the current query is for a month archive.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_month = \false;
        /**
         * Signifies whether the current query is for a day archive.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_day = \false;
        /**
         * Signifies whether the current query is for a specific time.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_time = \false;
        /**
         * Signifies whether the current query is for an author archive.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_author = \false;
        /**
         * Signifies whether the current query is for a category archive.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_category = \false;
        /**
         * Signifies whether the current query is for a tag archive.
         *
         * @since 2.3.0
         * @var bool
         */
        public $is_tag = \false;
        /**
         * Signifies whether the current query is for a taxonomy archive.
         *
         * @since 2.5.0
         * @var bool
         */
        public $is_tax = \false;
        /**
         * Signifies whether the current query is for a search.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_search = \false;
        /**
         * Signifies whether the current query is for a feed.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_feed = \false;
        /**
         * Signifies whether the current query is for a comment feed.
         *
         * @since 2.2.0
         * @var bool
         */
        public $is_comment_feed = \false;
        /**
         * Signifies whether the current query is for trackback endpoint call.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_trackback = \false;
        /**
         * Signifies whether the current query is for the site homepage.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_home = \false;
        /**
         * Signifies whether the current query is for the Privacy Policy page.
         *
         * @since 5.2.0
         * @var bool
         */
        public $is_privacy_policy = \false;
        /**
         * Signifies whether the current query couldn't find anything.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_404 = \false;
        /**
         * Signifies whether the current query is for an embed.
         *
         * @since 4.4.0
         * @var bool
         */
        public $is_embed = \false;
        /**
         * Signifies whether the current query is for a paged result and not for the first page.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_paged = \false;
        /**
         * Signifies whether the current query is for an administrative interface page.
         *
         * @since 1.5.0
         * @var bool
         */
        public $is_admin = \false;
        /**
         * Signifies whether the current query is for an attachment page.
         *
         * @since 2.0.0
         * @var bool
         */
        public $is_attachment = \false;
        /**
         * Signifies whether the current query is for an existing single post of any post type
         * (post, attachment, page, custom post types).
         *
         * @since 2.1.0
         * @var bool
         */
        public $is_singular = \false;
        /**
         * Signifies whether the current query is for the robots.txt file.
         *
         * @since 2.1.0
         * @var bool
         */
        public $is_robots = \false;
        /**
         * Signifies whether the current query is for the favicon.ico file.
         *
         * @since 5.4.0
         * @var bool
         */
        public $is_favicon = \false;
        /**
         * Signifies whether the current query is for a sitemap.
         *
         * @since 7.1.0
         */
        public bool $is_sitemap = \false;
        /**
         * Signifies whether the current query is for the page_for_posts page.
         *
         * Basically, the homepage if the option isn't set for the static homepage.
         *
         * @since 2.1.0
         * @var bool
         */
        public $is_posts_page = \false;
        /**
         * Signifies whether the current query is for a post type archive.
         *
         * @since 3.1.0
         * @var bool
         */
        public $is_post_type_archive = \false;
        /**
         * Set if post thumbnails are cached
         *
         * @since 3.2.0
         * @var bool
         */
        public $thumbnails_cached = \false;
        /**
         * Controls whether an attachment query should include filenames or not.
         *
         * @since 6.0.3
         * @var bool
         */
        protected $allow_query_attachment_by_filename = \false;
        /**
         * Initiates object properties and sets default values.
         *
         * @since 1.5.0
         */
        public function init()
        {
        }
        /**
         * Reparses the query vars.
         *
         * @since 1.5.0
         */
        public function parse_query_vars()
        {
        }
        /**
         * Fills in the query variables, which do not exist within the parameter.
         *
         * @since 2.1.0
         * @since 4.5.0 Removed the `comments_popup` public query variable.
         *
         * @param array $query_vars Defined query variables.
         * @return array Complete query variables with undefined ones filled in empty.
         */
        public function fill_query_vars($query_vars)
        {
        }
        /**
         * Parses a query string and sets query type booleans.
         *
         * @since 1.5.0
         * @since 4.2.0 Introduced the ability to order by specific clauses of a `$meta_query`, by passing the clause's
         *              array key to `$orderby`.
         * @since 4.4.0 Introduced `$post_name__in` and `$title` parameters. `$s` was updated to support excluded
         *              search terms, by prepending a hyphen.
         * @since 4.5.0 Removed the `$comments_popup` parameter.
         *              Introduced the `$comment_status` and `$ping_status` parameters.
         *              Introduced `RAND(x)` syntax for `$orderby`, which allows an integer seed value to random sorts.
         * @since 4.6.0 Added 'post_name__in' support for `$orderby`. Introduced the `$lazy_load_term_meta` argument.
         * @since 4.9.0 Introduced the `$comment_count` parameter.
         * @since 5.1.0 Introduced the `$meta_compare_key` parameter.
         * @since 5.3.0 Introduced the `$meta_type_key` parameter.
         * @since 6.1.0 Introduced the `$update_menu_item_cache` parameter.
         * @since 6.2.0 Introduced the `$search_columns` parameter.
         *
         * @param string|array $query {
         *     Optional. Array or string of Query parameters.
         *
         *     @type int             $attachment_id          Attachment post ID. Used for 'attachment' post_type.
         *     @type int|string      $author                 Author ID, or comma-separated list of IDs.
         *     @type string          $author_name            User 'user_nicename'.
         *     @type int[]           $author__in             An array of author IDs to query from.
         *     @type int[]           $author__not_in         An array of author IDs not to query from.
         *     @type bool            $cache_results          Whether to cache post information. Default true.
         *     @type int|string      $cat                    Category ID or comma-separated list of IDs (this or any children).
         *     @type int[]           $category__and          An array of category IDs (AND in).
         *     @type int[]           $category__in           An array of category IDs (OR in, no children).
         *     @type int[]           $category__not_in       An array of category IDs (NOT in).
         *     @type string          $category_name          Use category slug (not name, this or any children).
         *     @type array|int       $comment_count          Filter results by comment count. Provide an integer to match
         *                                                   comment count exactly. Provide an array with integer 'value'
         *                                                   and 'compare' operator ('=', '!=', '>', '>=', '<', '<=' ) to
         *                                                   compare against comment_count in a specific way.
         *     @type string          $comment_status         Comment status.
         *     @type int             $comments_per_page      The number of comments to return per page.
         *                                                   Default 'comments_per_page' option.
         *     @type array           $date_query             An associative array of WP_Date_Query arguments.
         *                                                   See WP_Date_Query::__construct().
         *     @type int             $day                    Day of the month. Default empty. Accepts numbers 1-31.
         *     @type bool            $exact                  Whether to search by exact keyword. Default false.
         *     @type string          $fields                 Post fields to query for. Accepts:
         *                                                   - '' Returns an array of complete post objects (`WP_Post[]`).
         *                                                   - 'ids' Returns an array of post IDs (`int[]`).
         *                                                   - 'id=>parent' Returns an associative array of parent post IDs,
         *                                                     keyed by post ID (`int[]`).
         *                                                   Default ''.
         *     @type int             $hour                   Hour of the day. Default empty. Accepts numbers 0-23.
         *     @type int|bool        $ignore_sticky_posts    Whether to ignore sticky posts or not. Setting this to false
         *                                                   excludes stickies from 'post__in'. Accepts 1|true, 0|false.
         *                                                   Default false.
         *     @type int             $m                      Combination YearMonth. Accepts any four-digit year and month
         *                                                   numbers 01-12. Default empty.
         *     @type string|string[] $meta_key               Meta key or keys to filter by.
         *     @type string|string[] $meta_value             Meta value or values to filter by.
         *     @type string          $meta_compare           MySQL operator used for comparing the meta value.
         *                                                   See WP_Meta_Query::__construct() for accepted values and default value.
         *     @type string          $meta_compare_key       MySQL operator used for comparing the meta key.
         *                                                   See WP_Meta_Query::__construct() for accepted values and default value.
         *     @type string          $meta_type              MySQL data type that the meta_value column will be CAST to for comparisons.
         *                                                   See WP_Meta_Query::__construct() for accepted values and default value.
         *     @type string          $meta_type_key          MySQL data type that the meta_key column will be CAST to for comparisons.
         *                                                   See WP_Meta_Query::__construct() for accepted values and default value.
         *     @type array           $meta_query             An associative array of WP_Meta_Query arguments.
         *                                                   See WP_Meta_Query::__construct() for accepted values.
         *     @type int             $menu_order             The menu order of the posts.
         *     @type int             $minute                 Minute of the hour. Default empty. Accepts numbers 0-59.
         *     @type int             $monthnum               The two-digit month. Default empty. Accepts numbers 1-12.
         *     @type string          $name                   Post slug.
         *     @type bool            $nopaging               Show all posts (true) or paginate (false). Default false.
         *     @type bool            $no_found_rows          Whether to skip counting the total rows found. Enabling can improve
         *                                                   performance. Default false.
         *     @type int             $offset                 The number of posts to offset before retrieval.
         *     @type string          $order                  Designates ascending or descending order of posts. Default 'DESC'.
         *                                                   Accepts 'ASC', 'DESC'.
         *     @type string|array    $orderby                Sort retrieved posts by parameter. One or more options may be passed.
         *                                                   To use 'meta_value', or 'meta_value_num', 'meta_key=keyname' must be
         *                                                   also be defined. To sort by a specific `$meta_query` clause, use that
         *                                                   clause's array key. Accepts:
         *                                                   - 'none'
         *                                                   - 'name'
         *                                                   - 'author'
         *                                                   - 'date'
         *                                                   - 'title'
         *                                                   - 'modified'
         *                                                   - 'menu_order'
         *                                                   - 'parent'
         *                                                   - 'ID'
         *                                                   - 'rand'
         *                                                   - 'relevance'
         *                                                   - 'RAND(x)' (where 'x' is an integer seed value)
         *                                                   - 'comment_count'
         *                                                   - 'meta_value'
         *                                                   - 'meta_value_num'
         *                                                   - 'post__in'
         *                                                   - 'post_name__in'
         *                                                   - 'post_parent__in'
         *                                                   - The array keys of `$meta_query`.
         *                                                   Default is 'date', except when a search is being performed, when
         *                                                   the default is 'relevance'.
         *     @type int             $p                      Post ID.
         *     @type int             $page                   Show the number of posts that would show up on page X of a
         *                                                   static front page.
         *     @type int             $paged                  The number of the current page.
         *     @type int             $page_id                Page ID.
         *     @type string          $pagename               Page slug.
         *     @type string          $perm                   Show posts if user has the appropriate capability.
         *     @type string          $ping_status            Ping status.
         *     @type int[]           $post__in               An array of post IDs to retrieve, sticky posts will be included.
         *     @type int[]           $post__not_in           An array of post IDs not to retrieve. Note: a string of comma-
         *                                                   separated IDs will NOT work.
         *     @type string          $post_mime_type         The mime type of the post. Used for 'attachment' post_type.
         *     @type string[]        $post_name__in          An array of post slugs that results must match.
         *     @type int             $post_parent            Page ID to retrieve child pages for. Use 0 to only retrieve
         *                                                   top-level pages.
         *     @type int[]           $post_parent__in        An array containing parent page IDs to query child pages from.
         *     @type int[]           $post_parent__not_in    An array containing parent page IDs not to query child pages from.
         *     @type string|string[] $post_type              A post type slug (string) or array of post type slugs.
         *                                                   Default 'any' if using 'tax_query'.
         *     @type string|string[] $post_status            A post status (string) or array of post statuses.
         *     @type int             $posts_per_page         The number of posts to query for. Use -1 to request all posts.
         *     @type int             $posts_per_archive_page The number of posts to query for by archive page. Overrides
         *                                                   'posts_per_page' when is_archive(), or is_search() are true.
         *     @type string          $s                      Search keyword(s). Prepending a term with a hyphen will
         *                                                   exclude posts matching that term. Eg, 'pillow -sofa' will
         *                                                   return posts containing 'pillow' but not 'sofa'. The
         *                                                   character used for exclusion can be modified using the
         *                                                   the 'wp_query_search_exclusion_prefix' filter.
         *     @type string[]        $search_columns         Array of column names to be searched. Accepts 'post_title',
         *                                                   'post_excerpt' and 'post_content'. Default empty array.
         *     @type int             $second                 Second of the minute. Default empty. Accepts numbers 0-59.
         *     @type bool            $sentence               Whether to search by phrase. Default false.
         *     @type bool            $suppress_filters       Whether to suppress filters. Default false.
         *     @type string          $tag                    Tag slug. Comma-separated (either), Plus-separated (all).
         *     @type int[]           $tag__and               An array of tag IDs (AND in).
         *     @type int[]           $tag__in                An array of tag IDs (OR in).
         *     @type int[]           $tag__not_in            An array of tag IDs (NOT in).
         *     @type int             $tag_id                 Tag id or comma-separated list of IDs.
         *     @type string[]        $tag_slug__and          An array of tag slugs (AND in).
         *     @type string[]        $tag_slug__in           An array of tag slugs (OR in). unless 'ignore_sticky_posts' is
         *                                                   true. Note: a string of comma-separated IDs will NOT work.
         *     @type array           $tax_query              An associative array of WP_Tax_Query arguments.
         *                                                   See WP_Tax_Query::__construct().
         *     @type string          $title                  Post title.
         *     @type bool            $update_post_meta_cache Whether to update the post meta cache. Default true.
         *     @type bool            $update_post_term_cache Whether to update the post term cache. Default true.
         *     @type bool            $update_menu_item_cache Whether to update the menu item cache. Default false.
         *     @type bool            $lazy_load_term_meta    Whether to lazy-load term meta. Setting to false will
         *                                                   disable cache priming for term meta, so that each
         *                                                   get_term_meta() call will hit the database.
         *                                                   Defaults to the value of `$update_post_term_cache`.
         *     @type int             $w                      The week number of the year. Default empty. Accepts numbers 1-53.
         *     @type int             $year                   The four-digit year. Default empty. Accepts any four-digit year.
         * }
         * @phpstan-param array{
         *   attachment_id?: int,
         *   author?: int|string,
         *   author_name?: string,
         *   author__in?: int[],
         *   author__not_in?: int[],
         *   cache_results?: bool,
         *   cat?: int|string,
         *   category__and?: int[],
         *   category__in?: int[],
         *   category__not_in?: int[],
         *   category_name?: string,
         *   comment_count?: array|int,
         *   comment_status?: string,
         *   comments_per_page?: int,
         *   date_query?: array,
         *   day?: int,
         *   exact?: bool,
         *   fields?: string,
         *   hour?: int,
         *   ignore_sticky_posts?: int|bool,
         *   m?: int,
         *   meta_key?: string|string[],
         *   meta_value?: string|string[],
         *   meta_compare?: string,
         *   meta_compare_key?: string,
         *   meta_type?: string,
         *   meta_type_key?: string,
         *   meta_query?: array,
         *   menu_order?: int,
         *   minute?: int,
         *   monthnum?: int,
         *   name?: string,
         *   nopaging?: bool,
         *   no_found_rows?: bool,
         *   offset?: int,
         *   order?: string,
         *   orderby?: string|array,
         *   p?: int,
         *   page?: int,
         *   paged?: int,
         *   page_id?: int,
         *   pagename?: string,
         *   perm?: string,
         *   ping_status?: string,
         *   post__in?: int[],
         *   post__not_in?: int[],
         *   post_mime_type?: string,
         *   post_name__in?: string[],
         *   post_parent?: int,
         *   post_parent__in?: int[],
         *   post_parent__not_in?: int[],
         *   post_type?: string|string[],
         *   post_status?: string|string[],
         *   posts_per_page?: int,
         *   posts_per_archive_page?: int,
         *   s?: string,
         *   search_columns?: string[],
         *   second?: int,
         *   sentence?: bool,
         *   suppress_filters?: bool,
         *   tag?: string,
         *   tag__and?: int[],
         *   tag__in?: int[],
         *   tag__not_in?: int[],
         *   tag_id?: int,
         *   tag_slug__and?: string[],
         *   tag_slug__in?: string[],
         *   tax_query?: array,
         *   title?: string,
         *   update_post_meta_cache?: bool,
         *   update_post_term_cache?: bool,
         *   update_menu_item_cache?: bool,
         *   lazy_load_term_meta?: bool,
         *   w?: int,
         *   year?: int,
         * } $query
         */
        public function parse_query($query = '')
        {
        }
        /**
         * Parses various taxonomy related query vars.
         *
         * For BC, this method is not marked as protected. See [28987].
         *
         * @since 3.1.0
         *
         * @param array $query_vars The query variables. Passed by reference.
         */
        public function parse_tax_query(&$query_vars)
        {
        }
        /**
         * Generates SQL for the WHERE clause based on passed search terms.
         *
         * @since 3.7.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param array $query_vars Query variables.
         * @return string WHERE clause.
         */
        protected function parse_search(&$query_vars)
        {
        }
        /**
         * Checks if the terms are suitable for searching.
         *
         * Uses an array of stopwords (terms) that are excluded from the separate
         * term matching when searching for posts. The list of English stopwords is
         * the approximate search engines list, and is translatable.
         *
         * @since 3.7.0
         *
         * @param string[] $terms Array of terms to check.
         * @return string[] Terms that are not stopwords.
         */
        protected function parse_search_terms($terms)
        {
        }
        /**
         * Retrieves stopwords used when parsing search terms.
         *
         * @since 3.7.0
         *
         * @return string[] Stopwords.
         */
        protected function get_search_stopwords()
        {
        }
        /**
         * Generates SQL for the ORDER BY condition based on passed search terms.
         *
         * @since 3.7.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param array $query_vars Query variables.
         * @return string ORDER BY clause.
         */
        protected function parse_search_order(&$query_vars)
        {
        }
        /**
         * Converts the given orderby alias (if allowed) to a properly-prefixed value.
         *
         * @since 4.0.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param string $orderby Alias for the field to order by.
         * @return string|false Table-prefixed value to used in the ORDER clause. False otherwise.
         */
        protected function parse_orderby($orderby)
        {
        }
        /**
         * Parse an 'order' query variable and cast it to ASC or DESC as necessary.
         *
         * @since 4.0.0
         *
         * @param string $order The 'order' query variable.
         * @return string The sanitized 'order' query variable.
         */
        protected function parse_order($order)
        {
        }
        /**
         * Sets the 404 property and saves whether query is feed.
         *
         * @since 2.0.0
         */
        public function set_404()
        {
        }
        /**
         * Retrieves the value of a query variable.
         *
         * @since 1.5.0
         * @since 3.9.0 The `$default_value` argument was introduced.
         *
         * @param string $query_var     Query variable key.
         * @param mixed  $default_value Optional. Value to return if the query variable is not set.
         *                              Default empty string.
         * @return mixed Contents of the query variable.
         */
        public function get($query_var, $default_value = '')
        {
        }
        /**
         * Sets the value of a query variable.
         *
         * @since 1.5.0
         *
         * @param string $query_var Query variable key.
         * @param mixed  $value     Query variable value.
         */
        public function set($query_var, $value)
        {
        }
        /**
         * Retrieves an array of posts based on query variables.
         *
         * There are a few filters and actions that can be used to modify the post
         * database query.
         *
         * @since 1.5.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @return WP_Post[]|int[] Array of post objects or post IDs.
         */
        public function get_posts()
        {
        }
        /**
         * Sets up the next post and iterate current post index.
         *
         * @since 1.5.0
         *
         * @return WP_Post Next post.
         */
        public function next_post()
        {
        }
        /**
         * Sets up the current post.
         *
         * Retrieves the next post, sets up the post, sets the 'in the loop'
         * property to true.
         *
         * @since 1.5.0
         *
         * @global WP_Post $post Global post object.
         */
        public function the_post()
        {
        }
        /**
         * Determines whether there are more posts available in the loop.
         *
         * Calls the {@see 'loop_end'} action when the loop is complete.
         *
         * @since 1.5.0
         *
         * @return bool True if posts are available, false if end of the loop.
         * @phpstan-impure
         */
        public function have_posts()
        {
        }
        /**
         * Rewinds the posts and resets post index.
         *
         * @since 1.5.0
         */
        public function rewind_posts()
        {
        }
        /**
         * Iterates current comment index and returns WP_Comment object.
         *
         * @since 2.2.0
         *
         * @return WP_Comment Comment object.
         */
        public function next_comment()
        {
        }
        /**
         * Sets up the current comment.
         *
         * @since 2.2.0
         *
         * @global WP_Comment $comment Global comment object.
         */
        public function the_comment()
        {
        }
        /**
         * Determines whether there are more comments available.
         *
         * Automatically rewinds comments when finished.
         *
         * @since 2.2.0
         *
         * @return bool True if comments are available, false if no more comments.
         */
        public function have_comments()
        {
        }
        /**
         * Rewinds the comments, resets the comment index and comment to first.
         *
         * @since 2.2.0
         */
        public function rewind_comments()
        {
        }
        /**
         * Sets up the WordPress query by parsing query string.
         *
         * @since 1.5.0
         *
         * @see WP_Query::parse_query() for all available arguments.
         *
         * @param string|array $query URL query string or array of query arguments.
         * @return WP_Post[]|int[] Array of post objects or post IDs.
         *
         * @phpstan-return (
         *     $query is array{ fields: 'ids', ... } ? int[] : WP_Post[]
         * )
         * @phpstan-return ($query is array ? ($query is array{fields: 'id=>parent'|'ids', ...} ? array<int, int> : array<int, \WP_Post>) : array<int, int>|array<int, \WP_Post>)
         */
        public function query($query)
        {
        }
        /**
         * Retrieves the currently queried object.
         *
         * If queried object is not set, then the queried object will be set from
         * the category, tag, taxonomy, posts page, single post, page, or author
         * query variable. After it is set up, it will be returned.
         *
         * @since 1.5.0
         *
         * @return WP_Term|WP_Post_Type|WP_Post|WP_User|null The queried object.
         */
        public function get_queried_object()
        {
        }
        /**
         * Retrieves the ID of the currently queried object.
         *
         * @since 1.5.0
         *
         * @return int
         */
        public function get_queried_object_id()
        {
        }
        /**
         * Constructor.
         *
         * Sets up the WordPress query, if parameter is not empty.
         *
         * @since 1.5.0
         *
         * @see WP_Query::parse_query() for all available arguments.
         *
         * @param string|array $query URL query string or array of vars.
         */
        public function __construct($query = '')
        {
        }
        /**
         * Makes private properties readable for backward compatibility.
         *
         * @since 4.0.0
         *
         * @param string $name Property to get.
         * @return mixed Property.
         */
        public function __get($name)
        {
        }
        /**
         * Makes private properties checkable for backward compatibility.
         *
         * @since 4.0.0
         *
         * @param string $name Property to check if set.
         * @return bool Whether the property is set.
         */
        public function __isset($name)
        {
        }
        /**
         * Makes private/protected methods readable for backward compatibility.
         *
         * @since 4.0.0
         *
         * @param string $name      Method to call.
         * @param array  $arguments Arguments to pass when calling.
         * @return mixed|false Return value of the callback, false otherwise.
         */
        public function __call($name, $arguments)
        {
        }
        /**
         * Determines whether the query is for an existing archive page.
         *
         * Archive pages include category, tag, author, date, custom post type,
         * and custom taxonomy based archives.
         *
         * @since 3.1.0
         *
         * @see WP_Query::is_category()
         * @see WP_Query::is_tag()
         * @see WP_Query::is_author()
         * @see WP_Query::is_date()
         * @see WP_Query::is_post_type_archive()
         * @see WP_Query::is_tax()
         *
         * @return bool Whether the query is for an existing archive page.
         */
        public function is_archive()
        {
        }
        /**
         * Determines whether the query is for an existing post type archive page.
         *
         * @since 3.1.0
         *
         * @param string|string[] $post_types Optional. Post type or array of posts types
         *                                    to check against. Default empty.
         * @return bool Whether the query is for an existing post type archive page.
         */
        public function is_post_type_archive($post_types = '')
        {
        }
        /**
         * Determines whether the query is for an existing attachment page.
         *
         * @since 3.1.0
         *
         * @param int|string|int[]|string[] $attachment Optional. Attachment ID, title, slug, or array of such
         *                                              to check against. Default empty.
         * @return bool Whether the query is for an existing attachment page.
         */
        public function is_attachment($attachment = '')
        {
        }
        /**
         * Determines whether the query is for an existing author archive page.
         *
         * If the $author parameter is specified, this function will additionally
         * check if the query is for one of the authors specified.
         *
         * @since 3.1.0
         *
         * @param int|string|int[]|string[] $author Optional. User ID, nickname, nicename, or array of such
         *                                          to check against. Default empty.
         * @return bool Whether the query is for an existing author archive page.
         */
        public function is_author($author = '')
        {
        }
        /**
         * Determines whether the query is for an existing category archive page.
         *
         * If the $category parameter is specified, this function will additionally
         * check if the query is for one of the categories specified.
         *
         * @since 3.1.0
         *
         * @param int|string|int[]|string[] $category Optional. Category ID, name, slug, or array of such
         *                                            to check against. Default empty.
         * @return bool Whether the query is for an existing category archive page.
         */
        public function is_category($category = '')
        {
        }
        /**
         * Determines whether the query is for an existing tag archive page.
         *
         * If the $tag parameter is specified, this function will additionally
         * check if the query is for one of the tags specified.
         *
         * @since 3.1.0
         *
         * @param int|string|int[]|string[] $tag Optional. Tag ID, name, slug, or array of such
         *                                       to check against. Default empty.
         * @return bool Whether the query is for an existing tag archive page.
         */
        public function is_tag($tag = '')
        {
        }
        /**
         * Determines whether the query is for an existing custom taxonomy archive page.
         *
         * If the $taxonomy parameter is specified, this function will additionally
         * check if the query is for that specific $taxonomy.
         *
         * If the $term parameter is specified in addition to the $taxonomy parameter,
         * this function will additionally check if the query is for one of the terms
         * specified.
         *
         * @since 3.1.0
         *
         * @global WP_Taxonomy[] $wp_taxonomies Registered taxonomies.
         *
         * @param string|string[]           $taxonomy Optional. Taxonomy slug or slugs to check against.
         *                                            Default empty.
         * @param int|string|int[]|string[] $term     Optional. Term ID, name, slug, or array of such
         *                                            to check against. Default empty.
         * @return bool Whether the query is for an existing custom taxonomy archive page.
         *              True for custom taxonomy archive pages, false for built-in taxonomies
         *              (category and tag archives).
         */
        public function is_tax($taxonomy = '', $term = '')
        {
        }
        /**
         * Determines whether the current URL is within the comments popup window.
         *
         * @since 3.1.0
         * @deprecated 4.5.0
         *
         * @return false Always returns false.
         */
        public function is_comments_popup()
        {
        }
        /**
         * Determines whether the query is for an existing date archive.
         *
         * @since 3.1.0
         *
         * @return bool Whether the query is for an existing date archive.
         */
        public function is_date()
        {
        }
        /**
         * Determines whether the query is for an existing day archive.
         *
         * @since 3.1.0
         *
         * @return bool Whether the query is for an existing day archive.
         */
        public function is_day()
        {
        }
        /**
         * Determines whether the query is for a feed.
         *
         * @since 3.1.0
         *
         * @param string|string[] $feeds Optional. Feed type or array of feed types
         *                                         to check against. Default empty.
         * @return bool Whether the query is for a feed.
         */
        public function is_feed($feeds = '')
        {
        }
        /**
         * Determines whether the query is for a comments feed.
         *
         * @since 3.1.0
         *
         * @return bool Whether the query is for a comments feed.
         */
        public function is_comment_feed()
        {
        }
        /**
         * Determines whether the query is for the front page of the site.
         *
         * This is for what is displayed at your site's main URL.
         *
         * Depends on the site's "Front page displays" Reading Settings 'show_on_front' and 'page_on_front'.
         *
         * If you set a static page for the front page of your site, this function will return
         * true when viewing that page.
         *
         * Otherwise the same as {@see WP_Query::is_home()}.
         *
         * @since 3.1.0
         *
         * @return bool Whether the query is for the front page of the site.
         */
        public function is_front_page()
        {
        }
        /**
         * Determines whether the query is for the blog homepage.
         *
         * This is the page which shows the time based blog content of your site.
         *
         * Depends on the site's "Front page displays" Reading Settings 'show_on_front' and 'page_for_posts'.
         *
         * If you set a static page for the front page of your site, this function will return
         * true only on the page you set as the "Posts page".
         *
         * @since 3.1.0
         *
         * @see WP_Query::is_front_page()
         *
         * @return bool Whether the query is for the blog homepage.
         */
        public function is_home()
        {
        }
        /**
         * Determines whether the query is for the Privacy Policy page.
         *
         * This is the page which shows the Privacy Policy content of your site.
         *
         * Depends on the site's "Change your Privacy Policy page" Privacy Settings 'wp_page_for_privacy_policy'.
         *
         * This function will return true only on the page you set as the "Privacy Policy page".
         *
         * @since 5.2.0
         *
         * @return bool Whether the query is for the Privacy Policy page.
         */
        public function is_privacy_policy()
        {
        }
        /**
         * Determines whether the query is for an existing month archive.
         *
         * @since 3.1.0
         *
         * @return bool Whether the query is for an existing month archive.
         */
        public function is_month()
        {
        }
        /**
         * Determines whether the query is for an existing single page.
         *
         * If the $page parameter is specified, this function will additionally
         * check if the query is for one of the pages specified.
         *
         * @since 3.1.0
         *
         * @see WP_Query::is_single()
         * @see WP_Query::is_singular()
         *
         * @param int|string|int[]|string[] $page Optional. Page ID, title, slug, path, or array of such
         *                                        to check against. Default empty.
         * @return bool Whether the query is for an existing single page.
         */
        public function is_page($page = '')
        {
        }
        /**
         * Determines whether the query is for a paged result and not for the first page.
         *
         * @since 3.1.0
         *
         * @return bool Whether the query is for a paged result.
         */
        public function is_paged()
        {
        }
        /**
         * Determines whether the query is for a post or page preview.
         *
         * @since 3.1.0
         *
         * @return bool Whether the query is for a post or page preview.
         */
        public function is_preview()
        {
        }
        /**
         * Determines whether the query is for the robots.txt file.
         *
         * @since 3.1.0
         *
         * @return bool Whether the query is for the robots.txt file.
         */
        public function is_robots()
        {
        }
        /**
         * Determines whether the query is for the favicon.ico file.
         *
         * @since 5.4.0
         *
         * @return bool Whether the query is for the favicon.ico file.
         */
        public function is_favicon()
        {
        }
        /**
         * Determines whether the query is for a sitemap.
         *
         * @since 7.1.0
         *
         * @return bool Whether the query is for a sitemap.
         */
        public function is_sitemap(): bool
        {
        }
        /**
         * Determines whether the query is for a search.
         *
         * @since 3.1.0
         *
         * @return bool Whether the query is for a search.
         */
        public function is_search()
        {
        }
        /**
         * Determines whether the query is for an existing single post.
         *
         * Works for any post type excluding pages.
         *
         * If the $post parameter is specified, this function will additionally
         * check if the query is for one of the Posts specified.
         *
         * @since 3.1.0
         *
         * @see WP_Query::is_page()
         * @see WP_Query::is_singular()
         *
         * @param int|string|int[]|string[] $post Optional. Post ID, title, slug, path, or array of such
         *                                        to check against. Default empty.
         * @return bool Whether the query is for an existing single post.
         */
        public function is_single($post = '')
        {
        }
        /**
         * Determines whether the query is for an existing single post of any post type
         * (post, attachment, page, custom post types).
         *
         * If the $post_types parameter is specified, this function will additionally
         * check if the query is for one of the Posts Types specified.
         *
         * @since 3.1.0
         *
         * @see WP_Query::is_page()
         * @see WP_Query::is_single()
         *
         * @param string|string[] $post_types Optional. Post type or array of post types
         *                                    to check against. Default empty.
         * @return bool Whether the query is for an existing single post
         *              or any of the given post types.
         */
        public function is_singular($post_types = '')
        {
        }
        /**
         * Determines whether the query is for a specific time.
         *
         * @since 3.1.0
         *
         * @return bool Whether the query is for a specific time.
         */
        public function is_time()
        {
        }
        /**
         * Determines whether the query is for a trackback endpoint call.
         *
         * @since 3.1.0
         *
         * @return bool Whether the query is for a trackback endpoint call.
         */
        public function is_trackback()
        {
        }
        /**
         * Determines whether the query is for an existing year archive.
         *
         * @since 3.1.0
         *
         * @return bool Whether the query is for an existing year archive.
         */
        public function is_year()
        {
        }
        /**
         * Determines whether the query is a 404 (returns no results).
         *
         * @since 3.1.0
         *
         * @return bool Whether the query is a 404 error.
         */
        public function is_404()
        {
        }
        /**
         * Determines whether the query is for an embedded post.
         *
         * @since 4.4.0
         *
         * @return bool Whether the query is for an embedded post.
         */
        public function is_embed()
        {
        }
        /**
         * Determines whether the query is the main query.
         *
         * @since 3.3.0
         *
         * @global WP_Query $wp_the_query WordPress Query object.
         *
         * @return bool Whether the query is the main query.
         */
        public function is_main_query()
        {
        }
        /**
         * Sets up global post data.
         *
         * @since 4.1.0
         * @since 4.4.0 Added the ability to pass a post ID to `$post`.
         *
         * @global int     $id
         * @global WP_User $authordata
         * @global string  $currentday
         * @global string  $currentmonth
         * @global int     $page
         * @global array   $pages
         * @global int     $multipage
         * @global int     $more
         * @global int     $numpages
         *
         * @param WP_Post|object|int $post WP_Post instance or Post ID/object.
         * @return bool True on success, false on failure.
         */
        public function setup_postdata($post)
        {
        }
        /**
         * Generates post data.
         *
         * @since 5.2.0
         *
         * @param WP_Post|object|int $post WP_Post instance or Post ID/object.
         * @return array|false Elements of post or false on failure.
         */
        public function generate_postdata($post)
        {
        }
        /**
         * Generates cache key.
         *
         * @since 6.1.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param array  $args Query arguments.
         * @param string $sql  SQL statement.
         * @return string Cache key.
         */
        protected function generate_cache_key(array $args, $sql)
        {
        }
        /**
         * After looping through a nested query, this function
         * restores the $post global to the current post in this query.
         *
         * @since 3.7.0
         *
         * @global WP_Post $post Global post object.
         */
        public function reset_postdata()
        {
        }
        /**
         * Lazyloads term meta for posts in the loop.
         *
         * @since 4.4.0
         * @deprecated 4.5.0 See wp_queue_posts_for_term_meta_lazyload().
         *
         * @param mixed $check
         * @param int   $term_id
         * @return mixed
         */
        public function lazyload_term_meta($check, $term_id)
        {
        }
        /**
         * Lazyloads comment meta for comments in the loop.
         *
         * @since 4.4.0
         * @deprecated 4.5.0 See wp_lazyload_comment_meta().
         *
         * @param mixed $check
         * @param int   $comment_id
         * @return mixed
         */
        public function lazyload_comment_meta($check, $comment_id)
        {
        }
    }
    /**
     * Core class used to implement a rewrite component API.
     *
     * The WordPress Rewrite class writes the rewrite module rules to the .htaccess
     * file. It also handles parsing the request to get the correct setup for the
     * WordPress Query class.
     *
     * The Rewrite along with WP class function as a front controller for WordPress.
     * You can add rules to trigger your page view and processing using this
     * component. The full functionality of a front controller does not exist,
     * meaning you can't define how the template files load based on the rewrite
     * rules.
     *
     * @since 1.5.0
     */
    #[\AllowDynamicProperties]
    class WP_Rewrite
    {
        /**
         * Permalink structure for posts.
         *
         * @since 1.5.0
         * @var string
         */
        public $permalink_structure;
        /**
         * Whether to add trailing slashes.
         *
         * @since 2.2.0
         * @var bool
         */
        public $use_trailing_slashes;
        /**
         * Base for the author permalink structure (example.com/$author_base/authorname).
         *
         * @since 1.5.0
         * @var string
         */
        public $author_base = 'author';
        /**
         * Permalink structure for author archives.
         *
         * @since 1.5.0
         * @var ?string
         */
        public $author_structure;
        /**
         * Permalink structure for date archives.
         *
         * @since 1.5.0
         * @var ?string
         */
        public $date_structure;
        /**
         * Permalink structure for pages.
         *
         * @since 1.5.0
         * @var ?string
         */
        public $page_structure;
        /**
         * Base of the search permalink structure (example.com/$search_base/query).
         *
         * @since 1.5.0
         * @var string
         */
        public $search_base = 'search';
        /**
         * Permalink structure for searches.
         *
         * @since 1.5.0
         * @var ?string
         */
        public $search_structure;
        /**
         * Comments permalink base.
         *
         * @since 1.5.0
         * @var string
         */
        public $comments_base = 'comments';
        /**
         * Pagination permalink base.
         *
         * @since 3.1.0
         * @var string
         */
        public $pagination_base = 'page';
        /**
         * Comments pagination permalink base.
         *
         * @since 4.2.0
         * @var string
         */
        public $comments_pagination_base = 'comment-page';
        /**
         * Feed permalink base.
         *
         * @since 1.5.0
         * @var string
         */
        public $feed_base = 'feed';
        /**
         * Comments feed permalink structure.
         *
         * @since 1.5.0
         * @var ?string
         */
        public $comment_feed_structure;
        /**
         * Feed request permalink structure.
         *
         * @since 1.5.0
         * @var ?string
         */
        public $feed_structure;
        /**
         * The static portion of the post permalink structure.
         *
         * If the permalink structure is "/archive/%post_id%" then the front
         * is "/archive/". If the permalink structure is "/%year%/%postname%/"
         * then the front is "/".
         *
         * @since 1.5.0
         * @var string
         *
         * @see WP_Rewrite::init()
         */
        public $front;
        /**
         * The prefix for all permalink structures.
         *
         * If PATHINFO/index permalinks are in use then the root is the value of
         * `WP_Rewrite::$index` with a trailing slash appended. Otherwise the root
         * will be empty.
         *
         * @since 1.5.0
         * @var string
         *
         * @see WP_Rewrite::init()
         * @see WP_Rewrite::using_index_permalinks()
         */
        public $root = '';
        /**
         * The name of the index file which is the entry point to all requests.
         *
         * @since 1.5.0
         * @var string
         */
        public $index = 'index.php';
        /**
         * Variable name to use for regex matches in the rewritten query.
         *
         * @since 1.5.0
         * @var string
         */
        public $matches = '';
        /**
         * Rewrite rules to match against the request to find the redirect or query.
         *
         * @since 1.5.0
         * @var string[]
         */
        public $rules;
        /**
         * Additional rules added external to the rewrite class.
         *
         * Those not generated by the class, see add_rewrite_rule().
         *
         * @since 2.1.0
         * @var string[]
         */
        public $extra_rules = array();
        /**
         * Additional rules that belong at the beginning to match first.
         *
         * Those not generated by the class, see add_rewrite_rule().
         *
         * @since 2.3.0
         * @var string[]
         */
        public $extra_rules_top = array();
        /**
         * Rules that don't redirect to WordPress' index.php.
         *
         * These rules are written to the mod_rewrite portion of the .htaccess,
         * and are added by add_external_rule().
         *
         * @since 2.1.0
         * @var string[]
         */
        public $non_wp_rules = array();
        /**
         * Extra permalink structures, e.g. categories, added by add_permastruct().
         *
         * @since 2.1.0
         * @var array[]
         */
        public $extra_permastructs = array();
        /**
         * Endpoints (like /trackback/) added by add_rewrite_endpoint().
         *
         * @since 2.1.0
         * @var array[]
         */
        public $endpoints;
        /**
         * Whether to write every mod_rewrite rule for WordPress into the .htaccess file.
         *
         * This is off by default, turning it on might print a lot of rewrite rules
         * to the .htaccess file.
         *
         * @since 2.0.0
         * @var bool
         *
         * @see WP_Rewrite::mod_rewrite_rules()
         */
        public $use_verbose_rules = \false;
        /**
         * Could post permalinks be confused with those of pages?
         *
         * If the first rewrite tag in the post permalink structure is one that could
         * also match a page name (e.g. %postname% or %author%) then this flag is
         * set to true. Prior to WordPress 3.3 this flag indicated that every page
         * would have a set of rules added to the top of the rewrite rules array.
         * Now it tells WP::parse_request() to check if a URL matching the page
         * permastruct is actually a page before accepting it.
         *
         * @since 2.5.0
         * @var bool
         *
         * @see WP_Rewrite::init()
         */
        public $use_verbose_page_rules = \true;
        /**
         * Rewrite tags that can be used in permalink structures.
         *
         * These are translated into the regular expressions stored in
         * `WP_Rewrite::$rewritereplace` and are rewritten to the query
         * variables listed in WP_Rewrite::$queryreplace.
         *
         * Additional tags can be added with add_rewrite_tag().
         *
         * @since 1.5.0
         * @var string[]
         */
        public $rewritecode = array('%year%', '%monthnum%', '%day%', '%hour%', '%minute%', '%second%', '%postname%', '%post_id%', '%author%', '%pagename%', '%search%');
        /**
         * Regular expressions to be substituted into rewrite rules in place
         * of rewrite tags, see WP_Rewrite::$rewritecode.
         *
         * @since 1.5.0
         * @var string[]
         */
        public $rewritereplace = array('([0-9]{4})', '([0-9]{1,2})', '([0-9]{1,2})', '([0-9]{1,2})', '([0-9]{1,2})', '([0-9]{1,2})', '([^/]+)', '([0-9]+)', '([^/]+)', '([^/]+?)', '(.+)');
        /**
         * Query variables that rewrite tags map to, see WP_Rewrite::$rewritecode.
         *
         * @since 1.5.0
         * @var string[]
         */
        public $queryreplace = array('year=', 'monthnum=', 'day=', 'hour=', 'minute=', 'second=', 'name=', 'p=', 'author_name=', 'pagename=', 's=');
        /**
         * Supported default feeds.
         *
         * @since 1.5.0
         * @var string[]
         */
        public $feeds = array('feed', 'rdf', 'rss', 'rss2', 'atom');
        /**
         * Determines whether permalinks are being used.
         *
         * This can be either rewrite module or permalink in the HTTP query string.
         *
         * @since 1.5.0
         *
         * @return bool True, if permalinks are enabled.
         */
        public function using_permalinks()
        {
        }
        /**
         * Determines whether permalinks are being used and rewrite module is not enabled.
         *
         * Means that permalink links are enabled and index.php is in the URL.
         *
         * @since 1.5.0
         *
         * @return bool Whether permalink links are enabled and index.php is in the URL.
         */
        public function using_index_permalinks()
        {
        }
        /**
         * Determines whether permalinks are being used and rewrite module is enabled.
         *
         * Using permalinks and index.php is not in the URL.
         *
         * @since 1.5.0
         *
         * @return bool Whether permalink links are enabled and index.php is NOT in the URL.
         */
        public function using_mod_rewrite_permalinks()
        {
        }
        /**
         * Indexes for matches for usage in preg_*() functions.
         *
         * The format of the string is, with empty matches property value, '$NUM'.
         * The 'NUM' will be replaced with the value in the $number parameter. With
         * the matches property not empty, the value of the returned string will
         * contain that value of the matches property. The format then will be
         * '$MATCHES[NUM]', with MATCHES as the value in the property and NUM the
         * value of the $number parameter.
         *
         * @since 1.5.0
         *
         * @param int $number Index number.
         * @return string
         */
        public function preg_index($number)
        {
        }
        /**
         * Retrieves all pages and attachments for pages URIs.
         *
         * The attachments are for those that have pages as parents and will be
         * retrieved.
         *
         * @since 2.5.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @return array Array of page URIs as first element and attachment URIs as second element.
         */
        public function page_uri_index()
        {
        }
        /**
         * Retrieves all of the rewrite rules for pages.
         *
         * @since 1.5.0
         *
         * @return string[] Page rewrite rules.
         */
        public function page_rewrite_rules()
        {
        }
        /**
         * Retrieves date permalink structure, with year, month, and day.
         *
         * The permalink structure for the date, if not set already depends on the
         * permalink structure. It can be one of three formats. The first is year,
         * month, day; the second is day, month, year; and the last format is month,
         * day, year. These are matched against the permalink structure for which
         * one is used. If none matches, then the default will be used, which is
         * year, month, day.
         *
         * Prevents post ID and date permalinks from overlapping. In the case of
         * post_id, the date permalink will be prepended with front permalink with
         * 'date/' before the actual permalink to form the complete date permalink
         * structure.
         *
         * @since 1.5.0
         *
         * @return string|false Date permalink structure on success, false on failure.
         */
        public function get_date_permastruct()
        {
        }
        /**
         * Retrieves the year permalink structure without month and day.
         *
         * Gets the date permalink structure and strips out the month and day
         * permalink structures.
         *
         * @since 1.5.0
         *
         * @return string|false Year permalink structure on success, false on failure.
         */
        public function get_year_permastruct()
        {
        }
        /**
         * Retrieves the month permalink structure without day and with year.
         *
         * Gets the date permalink structure and strips out the day permalink
         * structures. Keeps the year permalink structure.
         *
         * @since 1.5.0
         *
         * @return string|false Year/Month permalink structure on success, false on failure.
         */
        public function get_month_permastruct()
        {
        }
        /**
         * Retrieves the day permalink structure with month and year.
         *
         * Keeps date permalink structure with all year, month, and day.
         *
         * @since 1.5.0
         *
         * @return string|false Year/Month/Day permalink structure on success, false on failure.
         */
        public function get_day_permastruct()
        {
        }
        /**
         * Retrieves the permalink structure for categories.
         *
         * If the category_base property has no value, then the category structure
         * will have the front property value, followed by 'category', and finally
         * '%category%'. If it does, then the root property will be used, along with
         * the category_base property value.
         *
         * @since 1.5.0
         *
         * @return string|false Category permalink structure on success, false on failure.
         */
        public function get_category_permastruct()
        {
        }
        /**
         * Retrieves the permalink structure for tags.
         *
         * If the tag_base property has no value, then the tag structure will have
         * the front property value, followed by 'tag', and finally '%tag%'. If it
         * does, then the root property will be used, along with the tag_base
         * property value.
         *
         * @since 2.3.0
         *
         * @return string|false Tag permalink structure on success, false on failure.
         */
        public function get_tag_permastruct()
        {
        }
        /**
         * Retrieves an extra permalink structure by name.
         *
         * @since 2.5.0
         *
         * @param string $name Permalink structure name.
         * @return string|false Permalink structure string on success, false on failure.
         */
        public function get_extra_permastruct($name)
        {
        }
        /**
         * Retrieves the author permalink structure.
         *
         * The permalink structure is front property, author base, and finally
         * '/%author%'. Will set the author_structure property and then return it
         * without attempting to set the value again.
         *
         * @since 1.5.0
         *
         * @return string|false Author permalink structure on success, false on failure.
         */
        public function get_author_permastruct()
        {
        }
        /**
         * Retrieves the search permalink structure.
         *
         * The permalink structure is root property, search base, and finally
         * '/%search%'. Will set the search_structure property and then return it
         * without attempting to set the value again.
         *
         * @since 1.5.0
         *
         * @return string|false Search permalink structure on success, false on failure.
         */
        public function get_search_permastruct()
        {
        }
        /**
         * Retrieves the page permalink structure.
         *
         * The permalink structure is root property, and '%pagename%'. Will set the
         * page_structure property and then return it without attempting to set the
         * value again.
         *
         * @since 1.5.0
         *
         * @return string|false Page permalink structure on success, false on failure.
         */
        public function get_page_permastruct()
        {
        }
        /**
         * Retrieves the feed permalink structure.
         *
         * The permalink structure is root property, feed base, and finally
         * '/%feed%'. Will set the feed_structure property and then return it
         * without attempting to set the value again.
         *
         * @since 1.5.0
         *
         * @return string|false Feed permalink structure on success, false on failure.
         */
        public function get_feed_permastruct()
        {
        }
        /**
         * Retrieves the comment feed permalink structure.
         *
         * The permalink structure is root property, comment base property, feed
         * base and finally '/%feed%'. Will set the comment_feed_structure property
         * and then return it without attempting to set the value again.
         *
         * @since 1.5.0
         *
         * @return string|false Comment feed permalink structure on success, false on failure.
         */
        public function get_comment_feed_permastruct()
        {
        }
        /**
         * Adds or updates existing rewrite tags (e.g. %postname%).
         *
         * If the tag already exists, replace the existing pattern and query for
         * that tag, otherwise add the new tag.
         *
         * @since 1.5.0
         *
         * @see WP_Rewrite::$rewritecode
         * @see WP_Rewrite::$rewritereplace
         * @see WP_Rewrite::$queryreplace
         *
         * @param string $tag   Name of the rewrite tag to add or update.
         * @param string $regex Regular expression to substitute the tag for in rewrite rules.
         * @param string $query String to append to the rewritten query. Must end in '='.
         */
        public function add_rewrite_tag($tag, $regex, $query)
        {
        }
        /**
         * Removes an existing rewrite tag.
         *
         * @since 4.5.0
         *
         * @see WP_Rewrite::$rewritecode
         * @see WP_Rewrite::$rewritereplace
         * @see WP_Rewrite::$queryreplace
         *
         * @param string $tag Name of the rewrite tag to remove.
         */
        public function remove_rewrite_tag($tag)
        {
        }
        /**
         * Generates rewrite rules from a permalink structure.
         *
         * The main WP_Rewrite function for building the rewrite rule list. The
         * contents of the function is a mix of black magic and regular expressions,
         * so best just ignore the contents and move to the parameters.
         *
         * @since 1.5.0
         *
         * @param string $permalink_structure The permalink structure.
         * @param int    $ep_mask             Optional. Endpoint mask defining what endpoints are added to the structure.
         *                                    Accepts a mask of:
         *                                    - `EP_ALL`
         *                                    - `EP_NONE`
         *                                    - `EP_ALL_ARCHIVES`
         *                                    - `EP_ATTACHMENT`
         *                                    - `EP_AUTHORS`
         *                                    - `EP_CATEGORIES`
         *                                    - `EP_COMMENTS`
         *                                    - `EP_DATE`
         *                                    - `EP_DAY`
         *                                    - `EP_MONTH`
         *                                    - `EP_PAGES`
         *                                    - `EP_PERMALINK`
         *                                    - `EP_ROOT`
         *                                    - `EP_SEARCH`
         *                                    - `EP_TAGS`
         *                                    - `EP_YEAR`
         *                                    Default `EP_NONE`.
         * @param bool   $paged               Optional. Whether archive pagination rules should be added for the structure.
         *                                    Default true.
         * @param bool   $feed                Optional. Whether feed rewrite rules should be added for the structure.
         *                                    Default true.
         * @param bool   $forcomments         Optional. Whether the feed rules should be a query for a comments feed.
         *                                    Default false.
         * @param bool   $walk_dirs           Optional. Whether the 'directories' making up the structure should be walked
         *                                    over and rewrite rules built for each in-turn. Default true.
         * @param bool   $endpoints           Optional. Whether endpoints should be applied to the generated rewrite rules.
         *                                    Default true.
         * @return string[] Array of rewrite rules keyed by their regex pattern.
         */
        public function generate_rewrite_rules($permalink_structure, $ep_mask = \EP_NONE, $paged = \true, $feed = \true, $forcomments = \false, $walk_dirs = \true, $endpoints = \true)
        {
        }
        /**
         * Generates rewrite rules with permalink structure and walking directory only.
         *
         * Shorten version of WP_Rewrite::generate_rewrite_rules() that allows for shorter
         * list of parameters. See the method for longer description of what generating
         * rewrite rules does.
         *
         * @since 1.5.0
         *
         * @see WP_Rewrite::generate_rewrite_rules() See for long description and rest of parameters.
         *
         * @param string $permalink_structure The permalink structure to generate rules.
         * @param bool   $walk_dirs           Optional. Whether to create list of directories to walk over.
         *                                    Default false.
         * @return array An array of rewrite rules keyed by their regex pattern.
         */
        public function generate_rewrite_rule($permalink_structure, $walk_dirs = \false)
        {
        }
        /**
         * Constructs rewrite matches and queries from permalink structure.
         *
         * Runs the action {@see 'generate_rewrite_rules'} with the parameter that is an
         * reference to the current WP_Rewrite instance to further manipulate the
         * permalink structures and rewrite rules. Runs the {@see 'rewrite_rules_array'}
         * filter on the full rewrite rule array.
         *
         * There are two ways to manipulate the rewrite rules, one by hooking into
         * the {@see 'generate_rewrite_rules'} action and gaining full control of the
         * object or just manipulating the rewrite rule array before it is passed
         * from the function.
         *
         * @since 1.5.0
         *
         * @return string[] An associative array of matches and queries.
         */
        public function rewrite_rules()
        {
        }
        /**
         * Retrieves the rewrite rules.
         *
         * The difference between this method and WP_Rewrite::rewrite_rules() is that
         * this method stores the rewrite rules in the 'rewrite_rules' option and retrieves
         * it. This prevents having to process all of the permalinks to get the rewrite rules
         * in the form of caching.
         *
         * @since 1.5.0
         *
         * @return string[] Array of rewrite rules keyed by their regex pattern.
         */
        public function wp_rewrite_rules()
        {
        }
        /**
         * Retrieves mod_rewrite-formatted rewrite rules to write to .htaccess.
         *
         * Does not actually write to the .htaccess file, but creates the rules for
         * the process that will.
         *
         * Will add the non_wp_rules property rules to the .htaccess file before
         * the WordPress rewrite rules one.
         *
         * @since 1.5.0
         *
         * @return string
         */
        public function mod_rewrite_rules()
        {
        }
        /**
         * Retrieves IIS7 URL Rewrite formatted rewrite rules to write to web.config file.
         *
         * Does not actually write to the web.config file, but creates the rules for
         * the process that will.
         *
         * @since 2.8.0
         *
         * @param bool $add_parent_tags Optional. Whether to add parent tags to the rewrite rule sets.
         *                              Default false.
         * @return string IIS7 URL rewrite rule sets.
         */
        public function iis7_url_rewrite_rules($add_parent_tags = \false)
        {
        }
        /**
         * Adds a rewrite rule that transforms a URL structure to a set of query vars.
         *
         * Any value in the $after parameter that isn't 'bottom' will result in the rule
         * being placed at the top of the rewrite rules.
         *
         * @since 2.1.0
         * @since 4.4.0 Array support was added to the `$query` parameter.
         *
         * @param string       $regex Regular expression to match request against.
         * @param string|array $query The corresponding query vars for this rewrite rule.
         * @param string       $after Optional. Priority of the new rule. Accepts 'top'
         *                            or 'bottom'. Default 'bottom'.
         * @phpstan-param 'top'|'bottom' $after
         */
        public function add_rule($regex, $query, $after = 'bottom')
        {
        }
        /**
         * Adds a rewrite rule that doesn't correspond to index.php.
         *
         * @since 2.1.0
         *
         * @param string $regex Regular expression to match request against.
         * @param string $query The corresponding query vars for this rewrite rule.
         */
        public function add_external_rule($regex, $query)
        {
        }
        /**
         * Adds an endpoint, like /trackback/.
         *
         * @since 2.1.0
         * @since 3.9.0 $query_var parameter added.
         * @since 4.3.0 Added support for skipping query var registration by passing `false` to `$query_var`.
         *
         * @see add_rewrite_endpoint() for full documentation.
         * @global WP $wp Current WordPress environment instance.
         *
         * @param string      $name      Name of the endpoint.
         * @param int         $places    Endpoint mask describing the places the endpoint should be added.
         *                               Accepts a mask of:
         *                               - `EP_ALL`
         *                               - `EP_NONE`
         *                               - `EP_ALL_ARCHIVES`
         *                               - `EP_ATTACHMENT`
         *                               - `EP_AUTHORS`
         *                               - `EP_CATEGORIES`
         *                               - `EP_COMMENTS`
         *                               - `EP_DATE`
         *                               - `EP_DAY`
         *                               - `EP_MONTH`
         *                               - `EP_PAGES`
         *                               - `EP_PERMALINK`
         *                               - `EP_ROOT`
         *                               - `EP_SEARCH`
         *                               - `EP_TAGS`
         *                               - `EP_YEAR`
         * @param string|bool $query_var Optional. Name of the corresponding query variable. Pass `false` to
         *                               skip registering a query_var for this endpoint. Defaults to the
         *                               value of `$name`.
         */
        public function add_endpoint($name, $places, $query_var = \true)
        {
        }
        /**
         * Adds a new permalink structure.
         *
         * A permalink structure (permastruct) is an abstract definition of a set of rewrite rules;
         * it is an easy way of expressing a set of regular expressions that rewrite to a set of
         * query strings. The new permastruct is added to the WP_Rewrite::$extra_permastructs array.
         *
         * When the rewrite rules are built by WP_Rewrite::rewrite_rules(), all of these extra
         * permastructs are passed to WP_Rewrite::generate_rewrite_rules() which transforms them
         * into the regular expressions that many love to hate.
         *
         * The `$args` parameter gives you control over how WP_Rewrite::generate_rewrite_rules()
         * works on the new permastruct.
         *
         * @since 2.5.0
         *
         * @param string $name   Name for permalink structure.
         * @param string $struct Permalink structure (e.g. category/%category%)
         * @param array  $args   {
         *     Optional. Arguments for building rewrite rules based on the permalink structure.
         *     Default empty array.
         *
         *     @type bool $with_front  Whether the structure should be prepended with `WP_Rewrite::$front`.
         *                             Default true.
         *     @type int  $ep_mask     The endpoint mask defining which endpoints are added to the structure.
         *                             Accepts a mask of:
         *                             - `EP_ALL`
         *                             - `EP_NONE`
         *                             - `EP_ALL_ARCHIVES`
         *                             - `EP_ATTACHMENT`
         *                             - `EP_AUTHORS`
         *                             - `EP_CATEGORIES`
         *                             - `EP_COMMENTS`
         *                             - `EP_DATE`
         *                             - `EP_DAY`
         *                             - `EP_MONTH`
         *                             - `EP_PAGES`
         *                             - `EP_PERMALINK`
         *                             - `EP_ROOT`
         *                             - `EP_SEARCH`
         *                             - `EP_TAGS`
         *                             - `EP_YEAR`
         *                             Default `EP_NONE`.
         *     @type bool $paged       Whether archive pagination rules should be added for the structure.
         *                             Default true.
         *     @type bool $feed        Whether feed rewrite rules should be added for the structure. Default true.
         *     @type bool $forcomments Whether the feed rules should be a query for a comments feed. Default false.
         *     @type bool $walk_dirs   Whether the 'directories' making up the structure should be walked over
         *                             and rewrite rules built for each in-turn. Default true.
         *     @type bool $endpoints   Whether endpoints should be applied to the generated rules. Default true.
         * }
         * @phpstan-param array{
         *   with_front?: bool,
         *   ep_mask?: int,
         *   paged?: bool,
         *   feed?: bool,
         *   forcomments?: bool,
         *   walk_dirs?: bool,
         *   endpoints?: bool,
         * } $args
         */
        public function add_permastruct($name, $struct, $args = array())
        {
        }
        /**
         * Removes a permalink structure.
         *
         * @since 4.5.0
         *
         * @param string $name Name for permalink structure.
         */
        public function remove_permastruct($name)
        {
        }
        /**
         * Removes rewrite rules and then recreate rewrite rules.
         *
         * Calls WP_Rewrite::wp_rewrite_rules() after removing the 'rewrite_rules' option.
         * If the function named 'save_mod_rewrite_rules' exists, it will be called.
         *
         * @since 2.0.1
         *
         * @param bool $hard Whether to update .htaccess (hard flush) or just update rewrite_rules option (soft flush). Default is true (hard).
         * @phpstan-return void
         */
        public function flush_rules($hard = \true)
        {
        }
        /**
         * Sets up the object's properties.
         *
         * The 'use_verbose_page_rules' object property will be set to true if the
         * permalink structure begins with one of the following: '%postname%', '%category%',
         * '%tag%', or '%author%'.
         *
         * @since 1.5.0
         */
        public function init()
        {
        }
        /**
         * Sets the main permalink structure for the site.
         *
         * Will update the 'permalink_structure' option, if there is a difference
         * between the current permalink structure and the parameter value. Calls
         * WP_Rewrite::init() after the option is updated.
         *
         * Fires the {@see 'permalink_structure_changed'} action once the init call has
         * processed passing the old and new values
         *
         * @since 1.5.0
         *
         * @param string $permalink_structure Permalink structure.
         */
        public function set_permalink_structure($permalink_structure)
        {
        }
        /**
         * Sets the category base for the category permalink.
         *
         * Will update the 'category_base' option, if there is a difference between
         * the current category base and the parameter value. Calls WP_Rewrite::init()
         * after the option is updated.
         *
         * @since 1.5.0
         *
         * @param string $category_base Category permalink structure base.
         */
        public function set_category_base($category_base)
        {
        }
        /**
         * Sets the tag base for the tag permalink.
         *
         * Will update the 'tag_base' option, if there is a difference between the
         * current tag base and the parameter value. Calls WP_Rewrite::init() after
         * the option is updated.
         *
         * @since 2.3.0
         *
         * @param string $tag_base Tag permalink structure base.
         */
        public function set_tag_base($tag_base)
        {
        }
        /**
         * Constructor - Calls init(), which runs setup.
         *
         * @since 1.5.0
         */
        public function __construct()
        {
        }
    }
    /**
     * Core class used to register scripts.
     *
     * @since 2.1.0
     *
     * @see WP_Dependencies
     */
    class WP_Scripts extends \WP_Dependencies
    {
        /**
         * Base URL for scripts.
         *
         * Full URL with trailing slash.
         *
         * @since 2.6.0
         * @see wp_default_scripts()
         * @var string|null
         */
        public $base_url;
        /**
         * URL of the content directory.
         *
         * @since 2.8.0
         * @see wp_default_scripts()
         * @var string|null
         */
        public $content_url;
        /**
         * Default version string for scripts.
         *
         * @since 2.6.0
         * @see wp_default_scripts()
         * @var string|null
         */
        public $default_version;
        /**
         * Holds handles of scripts which are enqueued in footer.
         *
         * @since 2.8.0
         * @var string[]
         */
        public $in_footer = array();
        /**
         * Holds a list of script handles which will be concatenated.
         *
         * @since 2.8.0
         * @var string
         */
        public $concat = '';
        /**
         * Holds a string which contains script handles and their version.
         *
         * @since 2.8.0
         * @deprecated 3.4.0
         * @var string
         */
        public $concat_version = '';
        /**
         * Whether to perform concatenation.
         *
         * @since 2.8.0
         * @var bool
         */
        public $do_concat = \false;
        /**
         * Holds HTML markup of scripts and additional data if concatenation
         * is enabled.
         *
         * @since 2.8.0
         * @var string
         */
        public $print_html = '';
        /**
         * Holds inline code if concatenation is enabled.
         *
         * @since 2.8.0
         * @var string
         */
        public $print_code = '';
        /**
         * Holds a list of script handles which are not in the default directory
         * if concatenation is enabled.
         *
         * Unused in core.
         *
         * @since 2.8.0
         * @var string
         */
        public $ext_handles = '';
        /**
         * Holds a string which contains handles and versions of scripts which
         * are not in the default directory if concatenation is enabled.
         *
         * Unused in core.
         *
         * @since 2.8.0
         * @var string
         */
        public $ext_version = '';
        /**
         * List of default directories.
         *
         * @since 2.8.0
         * @see wp_default_scripts()
         * @var string[]|null
         */
        public $default_dirs;
        /**
         * Constructor.
         *
         * @since 2.6.0
         */
        public function __construct()
        {
        }
        /**
         * Initialize the class.
         *
         * @since 3.4.0
         */
        public function init()
        {
        }
        /**
         * Prints scripts.
         *
         * Prints the scripts passed to it or the print queue. Also prints all necessary dependencies.
         *
         * @since 2.1.0
         * @since 2.8.0 Added the `$group` parameter.
         *
         * @param string|string[]|false $handles Optional. Scripts to be printed: queue (false),
         *                                       single script (string), or multiple scripts (array of strings).
         *                                       Default false.
         * @param int|false             $group   Optional. Group level: level (int), no groups (false).
         *                                       Default false.
         * @return string[] Handles of scripts that have been printed.
         */
        public function print_scripts($handles = \false, $group = \false)
        {
        }
        /**
         * Prints extra scripts of a registered script.
         *
         * @since 2.1.0
         * @since 2.8.0 Added the `$display` parameter.
         * @deprecated 3.3.0
         *
         * @see print_extra_script()
         *
         * @param string $handle  The script's registered handle.
         * @param bool   $display Optional. Whether to print the extra script
         *                        instead of just returning it. Default true.
         * @return bool|string|null Null if no data exists, extra scripts if `$display` is true,
         *                          true otherwise.
         */
        public function print_scripts_l10n($handle, $display = \true)
        {
        }
        /**
         * Prints extra scripts of a registered script.
         *
         * @since 3.3.0
         *
         * @param string $handle  The script's registered handle.
         * @param bool   $display Optional. Whether to print the extra script
         *                        instead of just returning it. Default true.
         * @return bool|string|null Null if no data exists, extra scripts if `$display` is true,
         *                          true otherwise.
         */
        public function print_extra_script($handle, $display = \true)
        {
        }
        /**
         * Processes a script dependency.
         *
         * @since 2.6.0
         * @since 2.8.0 Added the `$group` parameter.
         *
         * @see WP_Dependencies::do_item()
         *
         * @param string    $handle The script's registered handle.
         * @param int|false $group  Optional. Group level: level (int), no groups (false).
         *                          Default false.
         * @return bool True on success, false on failure.
         */
        public function do_item($handle, $group = \false)
        {
        }
        /**
         * Adds extra code to a registered script.
         *
         * @since 4.5.0
         *
         * @param string $handle   Name of the script to add the inline script to.
         *                         Must be lowercase.
         * @param string $data     String containing the JavaScript to be added.
         * @param string $position Optional. Whether to add the inline script
         *                         before the handle or after. Default 'after'.
         * @return bool True on success, false on failure.
         */
        public function add_inline_script($handle, $data, $position = 'after')
        {
        }
        /**
         * Prints inline scripts registered for a specific handle.
         *
         * @since 4.5.0
         * @deprecated 6.3.0 Use methods get_inline_script_tag() or get_inline_script_data() instead.
         *
         * @param string $handle   Name of the script to print inline scripts for.
         *                         Must be lowercase.
         * @param string $position Optional. Whether to add the inline script
         *                         before the handle or after. Default 'after'.
         * @param bool   $display  Optional. Whether to print the script tag
         *                         instead of just returning the script data. Default true.
         * @return string|false Script data on success, false otherwise.
         */
        public function print_inline_script($handle, $position = 'after', $display = \true)
        {
        }
        /**
         * Gets data for inline scripts registered for a specific handle.
         *
         * @since 6.3.0
         *
         * @param string $handle   Name of the script to get data for.
         *                         Must be lowercase.
         * @param string $position Optional. Whether to add the inline script
         *                         before the handle or after. Default 'after'.
         * @return string Inline script, which may be empty string.
         */
        public function get_inline_script_data($handle, $position = 'after')
        {
        }
        /**
         * Gets tags for inline scripts registered for a specific handle.
         *
         * @since 6.3.0
         *
         * @param string $handle   Name of the script to get associated inline script tag for.
         *                         Must be lowercase.
         * @param string $position Optional. Whether to get tag for inline
         *                         scripts in the before or after position. Default 'after'.
         * @return string Inline script, which may be empty string.
         */
        public function get_inline_script_tag($handle, $position = 'after')
        {
        }
        /**
         * Localizes a script, only if the script has already been added.
         *
         * @since 2.1.0
         *
         * @param string               $handle      Name of the script to attach data to.
         * @param string               $object_name Name of the variable that will contain the data.
         * @param array<string, mixed> $l10n        Array of data to localize.
         * @return bool True on success, false on failure.
         */
        public function localize($handle, $object_name, $l10n)
        {
        }
        /**
         * Sets handle group.
         *
         * @since 2.8.0
         *
         * @see WP_Dependencies::set_group()
         *
         * @param string    $handle    Name of the item. Should be unique.
         * @param bool      $recursion Internal flag that calling function was called recursively.
         * @param int|false $group     Optional. Group level: level (int), no groups (false).
         *                             Default false.
         * @return bool Not already in the group or a lower group.
         */
        public function set_group($handle, $recursion, $group = \false)
        {
        }
        /**
         * Sets a translation textdomain.
         *
         * @since 5.0.0
         * @since 5.1.0 The `$domain` parameter was made optional.
         *
         * @param string $handle Name of the script to register a translation domain to.
         * @param string $domain Optional. Text domain. Default 'default'.
         * @param string $path   Optional. The full file path to the directory containing translation files.
         * @return bool True if the text domain was registered, false if not.
         */
        public function set_translations($handle, $domain = 'default', $path = '')
        {
        }
        /**
         * Prints translations set for a specific handle.
         *
         * @since 5.0.0
         *
         * @param string $handle  Name of the script to add the inline script to.
         *                        Must be lowercase.
         * @param bool   $display Optional. Whether to print the script
         *                        instead of just returning it. Default true.
         * @return string|false Script on success, false otherwise.
         */
        public function print_translations($handle, $display = \true)
        {
        }
        /**
         * Determines script dependencies.
         *
         * @since 2.1.0
         *
         * @see WP_Dependencies::all_deps()
         *
         * @param string|string[] $handles   Item handle (string) or item handles (array of strings).
         * @param bool            $recursion Optional. Internal flag that function is calling itself.
         *                                   Default false.
         * @param int|false       $group     Optional. Group level: level (int), no groups (false).
         *                                   Default false.
         * @return bool True on success, false on failure.
         */
        public function all_deps($handles, $recursion = \false, $group = \false)
        {
        }
        /**
         * Processes items and dependencies for the head group.
         *
         * @since 2.8.0
         *
         * @see WP_Dependencies::do_items()
         *
         * @return string[] Handles of items that have been processed.
         */
        public function do_head_items()
        {
        }
        /**
         * Processes items and dependencies for the footer group.
         *
         * @since 2.8.0
         *
         * @see WP_Dependencies::do_items()
         *
         * @return string[] Handles of items that have been processed.
         */
        public function do_footer_items()
        {
        }
        /**
         * Whether a handle's source is in a default directory.
         *
         * @since 2.8.0
         *
         * @param string $src The source of the enqueued script.
         * @return bool True if found, false if not.
         */
        public function in_default_dir($src)
        {
        }
        /**
         * This overrides the add_data method from WP_Dependencies, to support normalizing of $args.
         *
         * @since 6.3.0
         *
         * @param string $handle Name of the item. Should be unique.
         * @param string $key    The data key.
         * @param mixed  $value  The data value.
         * @return bool True on success, false on failure.
         */
        public function add_data($handle, $key, $value)
        {
        }
        /**
         * Resets class properties.
         *
         * @since 2.8.0
         */
        public function reset()
        {
        }
        /**
         * Gets a script-specific dependency warning message.
         *
         * @since 6.9.1
         *
         * @param string   $handle                     Script handle with missing dependencies.
         * @param string[] $missing_dependency_handles Missing dependency handles.
         * @return string Formatted, localized warning message.
         */
        protected function get_dependency_warning_message($handle, $missing_dependency_handles)
        {
        }
    }
    /**
     * Core class used for querying sites.
     *
     * @since 4.6.0
     *
     * @see WP_Site_Query::__construct() for accepted arguments.
     */
    #[\AllowDynamicProperties]
    class WP_Site_Query
    {
        /**
         * SQL for database query.
         *
         * @since 4.6.0
         * @var string
         */
        public $request;
        /**
         * SQL query clauses.
         *
         * @since 4.6.0
         * @var array
         */
        protected $sql_clauses = array('select' => '', 'from' => '', 'where' => array(), 'groupby' => '', 'orderby' => '', 'limits' => '');
        /**
         * Metadata query container.
         *
         * @since 5.1.0
         * @var WP_Meta_Query
         */
        public $meta_query = \false;
        /**
         * Metadata query clauses.
         *
         * @since 5.1.0
         * @var array
         */
        protected $meta_query_clauses;
        /**
         * Date query container.
         *
         * @since 4.6.0
         * @var WP_Date_Query A date query instance.
         */
        public $date_query = \false;
        /**
         * Query vars set by the user.
         *
         * @since 4.6.0
         * @var array
         */
        public $query_vars;
        /**
         * Default values for query vars.
         *
         * @since 4.6.0
         * @var array
         */
        public $query_var_defaults;
        /**
         * List of sites located by the query.
         *
         * @since 4.6.0
         * @var array
         */
        public $sites;
        /**
         * The amount of found sites for the current query.
         *
         * @since 4.6.0
         * @var int
         */
        public $found_sites = 0;
        /**
         * The number of pages.
         *
         * @since 4.6.0
         * @var int
         */
        public $max_num_pages = 0;
        /**
         * Sets up the site query, based on the query vars passed.
         *
         * @since 4.6.0
         * @since 4.8.0 Introduced the 'lang_id', 'lang__in', and 'lang__not_in' parameters.
         * @since 5.1.0 Introduced the 'update_site_meta_cache', 'meta_query', 'meta_key',
         *              'meta_compare_key', 'meta_value', 'meta_type', and 'meta_compare' parameters.
         * @since 5.3.0 Introduced the 'meta_type_key' parameter.
         *
         * @param string|array $query {
         *     Optional. Array or query string of site query parameters. Default empty.
         *
         *     @type int[]           $site__in               Array of site IDs to include. Default empty.
         *     @type int[]           $site__not_in           Array of site IDs to exclude. Default empty.
         *     @type bool            $count                  Whether to return a site count (true) or array of site objects.
         *                                                   Default false.
         *     @type array           $date_query             Date query clauses to limit sites by. See WP_Date_Query.
         *                                                   Default null.
         *     @type string          $fields                 Site fields to return. Accepts 'ids' (returns an array of site IDs)
         *                                                   or empty (returns an array of complete site objects). Default empty.
         *     @type int             $ID                     A site ID to only return that site. Default empty.
         *     @type int             $number                 Maximum number of sites to retrieve. Default 100.
         *     @type int             $offset                 Number of sites to offset the query. Used to build LIMIT clause.
         *                                                   Default 0.
         *     @type bool            $no_found_rows          Whether to disable the `SQL_CALC_FOUND_ROWS` query. Default true.
         *     @type string|array    $orderby                Site status or array of statuses. Accepts:
         *                                                   - 'id'
         *                                                   - 'domain'
         *                                                   - 'path'
         *                                                   - 'network_id'
         *                                                   - 'last_updated'
         *                                                   - 'registered'
         *                                                   - 'domain_length'
         *                                                   - 'path_length'
         *                                                   - 'site__in'
         *                                                   - 'network__in'
         *                                                   - 'deleted'
         *                                                   - 'mature'
         *                                                   - 'spam'
         *                                                   - 'archived'
         *                                                   - 'public'
         *                                                   - false, an empty array, or 'none' to disable `ORDER BY` clause.
         *                                                   Default 'id'.
         *     @type string          $order                  How to order retrieved sites. Accepts 'ASC', 'DESC'. Default 'ASC'.
         *     @type int             $network_id             Limit results to those affiliated with a given network ID. If 0,
         *                                                   include all networks. Default 0.
         *     @type int[]           $network__in            Array of network IDs to include affiliated sites for. Default empty.
         *     @type int[]           $network__not_in        Array of network IDs to exclude affiliated sites for. Default empty.
         *     @type string          $domain                 Limit results to those affiliated with a given domain. Default empty.
         *     @type string[]        $domain__in             Array of domains to include affiliated sites for. Default empty.
         *     @type string[]        $domain__not_in         Array of domains to exclude affiliated sites for. Default empty.
         *     @type string          $path                   Limit results to those affiliated with a given path. Default empty.
         *     @type string[]        $path__in               Array of paths to include affiliated sites for. Default empty.
         *     @type string[]        $path__not_in           Array of paths to exclude affiliated sites for. Default empty.
         *     @type int             $public                 Limit results to public sites. Accepts 1 or 0. Default empty.
         *     @type int             $archived               Limit results to archived sites. Accepts 1 or 0. Default empty.
         *     @type int             $mature                 Limit results to mature sites. Accepts 1 or 0. Default empty.
         *     @type int             $spam                   Limit results to spam sites. Accepts 1 or 0. Default empty.
         *     @type int             $deleted                Limit results to deleted sites. Accepts 1 or 0. Default empty.
         *     @type int             $lang_id                Limit results to a language ID. Default empty.
         *     @type string[]        $lang__in               Array of language IDs to include affiliated sites for. Default empty.
         *     @type string[]        $lang__not_in           Array of language IDs to exclude affiliated sites for. Default empty.
         *     @type string          $search                 Search term(s) to retrieve matching sites for. Default empty.
         *     @type string[]        $search_columns         Array of column names to be searched. Accepts 'domain' and 'path'.
         *                                                   Default empty array.
         *     @type bool            $update_site_cache      Whether to prime the cache for found sites. Default true.
         *     @type bool            $update_site_meta_cache Whether to prime the metadata cache for found sites. Default true.
         *     @type string|string[] $meta_key               Meta key or keys to filter by.
         *     @type string|string[] $meta_value             Meta value or values to filter by.
         *     @type string          $meta_compare           MySQL operator used for comparing the meta value.
         *                                                   See WP_Meta_Query::__construct() for accepted values and default value.
         *     @type string          $meta_compare_key       MySQL operator used for comparing the meta key.
         *                                                   See WP_Meta_Query::__construct() for accepted values and default value.
         *     @type string          $meta_type              MySQL data type that the meta_value column will be CAST to for comparisons.
         *                                                   See WP_Meta_Query::__construct() for accepted values and default value.
         *     @type string          $meta_type_key          MySQL data type that the meta_key column will be CAST to for comparisons.
         *                                                   See WP_Meta_Query::__construct() for accepted values and default value.
         *     @type array           $meta_query             An associative array of WP_Meta_Query arguments.
         *                                                   See WP_Meta_Query::__construct() for accepted values.
         * }
         * @phpstan-param array{
         *   site__in?: int[],
         *   site__not_in?: int[],
         *   count?: bool,
         *   date_query?: array,
         *   fields?: string,
         *   ID?: int,
         *   number?: int,
         *   offset?: int,
         *   no_found_rows?: bool,
         *   orderby?: string|array,
         *   order?: string,
         *   network_id?: int,
         *   network__in?: int[],
         *   network__not_in?: int[],
         *   domain?: string,
         *   domain__in?: string[],
         *   domain__not_in?: string[],
         *   path?: string,
         *   path__in?: string[],
         *   path__not_in?: string[],
         *   public?: int,
         *   archived?: int,
         *   mature?: int,
         *   spam?: int,
         *   deleted?: int,
         *   lang_id?: int,
         *   lang__in?: string[],
         *   lang__not_in?: string[],
         *   search?: string,
         *   search_columns?: string[],
         *   update_site_cache?: bool,
         *   update_site_meta_cache?: bool,
         *   meta_key?: string|string[],
         *   meta_value?: string|string[],
         *   meta_compare?: string,
         *   meta_compare_key?: string,
         *   meta_type?: string,
         *   meta_type_key?: string,
         *   meta_query?: array,
         * } $query
         */
        public function __construct($query = '')
        {
        }
        /**
         * Parses arguments passed to the site query with default query parameters.
         *
         * @since 4.6.0
         *
         * @see WP_Site_Query::__construct()
         *
         * @param string|array $query Array or string of WP_Site_Query arguments. See WP_Site_Query::__construct().
         * @phpstan-param array{
         *   site__in?: int[],
         *   site__not_in?: int[],
         *   count?: bool,
         *   date_query?: array,
         *   fields?: string,
         *   ID?: int,
         *   number?: int,
         *   offset?: int,
         *   no_found_rows?: bool,
         *   orderby?: string|array,
         *   order?: string,
         *   network_id?: int,
         *   network__in?: int[],
         *   network__not_in?: int[],
         *   domain?: string,
         *   domain__in?: string[],
         *   domain__not_in?: string[],
         *   path?: string,
         *   path__in?: string[],
         *   path__not_in?: string[],
         *   public?: int,
         *   archived?: int,
         *   mature?: int,
         *   spam?: int,
         *   deleted?: int,
         *   lang_id?: int,
         *   lang__in?: string[],
         *   lang__not_in?: string[],
         *   search?: string,
         *   search_columns?: string[],
         *   update_site_cache?: bool,
         *   update_site_meta_cache?: bool,
         *   meta_key?: string|string[],
         *   meta_value?: string|string[],
         *   meta_compare?: string,
         *   meta_compare_key?: string,
         *   meta_type?: string,
         *   meta_type_key?: string,
         *   meta_query?: array,
         * } $query See WP_Site_Query::__construct()
         */
        public function parse_query($query = '')
        {
        }
        /**
         * Sets up the WordPress query for retrieving sites.
         *
         * @since 4.6.0
         *
         * @param string|array $query Array or URL query string of parameters.
         * @return WP_Site[]|int[]|int List of WP_Site objects, a list of site IDs when 'fields' is set to 'ids',
         *                             or the number of sites when 'count' is passed as a query var.
         */
        public function query($query)
        {
        }
        /**
         * Retrieves a list of sites matching the query vars.
         *
         * @since 4.6.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @return WP_Site[]|int[]|int List of WP_Site objects, a list of site IDs when 'fields' is set to 'ids',
         *                             or the number of sites when 'count' is passed as a query var.
         */
        public function get_sites()
        {
        }
        /**
         * Used internally to get a list of site IDs matching the query vars.
         *
         * @since 4.6.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @return int|array A single count of site IDs if a count query. An array of site IDs if a full query.
         */
        protected function get_site_ids()
        {
        }
        /**
         * Used internally to generate an SQL string for searching across multiple columns.
         *
         * @since 4.6.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param string   $search  Search string.
         * @param string[] $columns Array of columns to search.
         * @return string Search SQL.
         */
        protected function get_search_sql($search, $columns)
        {
        }
        /**
         * Parses and sanitizes 'orderby' keys passed to the site query.
         *
         * @since 4.6.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param string $orderby Alias for the field to order by.
         * @return string|false Value to used in the ORDER clause. False otherwise.
         */
        protected function parse_orderby($orderby)
        {
        }
        /**
         * Parses an 'order' query variable and cast it to 'ASC' or 'DESC' as necessary.
         *
         * @since 4.6.0
         *
         * @param string $order The 'order' query variable.
         * @return string The sanitized 'order' query variable.
         */
        protected function parse_order($order)
        {
        }
    }
    /**
     * Core class used for interacting with a multisite site.
     *
     * This class is used during load to populate the `$current_blog` global and
     * setup the current site.
     *
     * @since 4.5.0
     *
     * @property int              $id
     * @property int              $network_id
     * @property string           $blogname
     * @property string           $siteurl
     * @property int|string|false $post_count
     * @property string           $home
     *
     * @phpstan-property int|numeric-string|false $post_count
     */
    #[\AllowDynamicProperties]
    final class WP_Site
    {
        /**
         * Site ID.
         *
         * Named "blog" vs. "site" for legacy reasons.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 4.5.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $blog_id;
        /**
         * Domain of the site.
         *
         * @since 4.5.0
         * @var string
         */
        public $domain = '';
        /**
         * Path of the site.
         *
         * @since 4.5.0
         * @var string
         */
        public $path = '';
        /**
         * The ID of the site's parent network.
         *
         * Named "site" vs. "network" for legacy reasons. An individual site's "site" is
         * its network.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 4.5.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $site_id = '0';
        /**
         * The date and time on which the site was created or registered.
         *
         * @since 4.5.0
         * @var string Date in MySQL's datetime format.
         */
        public $registered = '0000-00-00 00:00:00';
        /**
         * The date and time on which site settings were last updated.
         *
         * @since 4.5.0
         * @var string Date in MySQL's datetime format.
         */
        public $last_updated = '0000-00-00 00:00:00';
        /**
         * Whether the site should be treated as public.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 4.5.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $public = '1';
        /**
         * Whether the site should be treated as archived.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 4.5.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $archived = '0';
        /**
         * Whether the site should be treated as mature.
         *
         * Handling for this does not exist throughout WordPress core, but custom
         * implementations exist that require the property to be present.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 4.5.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $mature = '0';
        /**
         * Whether the site should be treated as spam.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 4.5.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $spam = '0';
        /**
         * Whether the site should be treated as flagged for deletion.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 4.5.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $deleted = '0';
        /**
         * The language pack associated with this site.
         *
         * A numeric string, for compatibility reasons.
         *
         * @since 4.5.0
         * @var string
         * @phpstan-var numeric-string
         */
        public $lang_id = '0';
        /**
         * Retrieves a site from the database by its ID.
         *
         * @since 4.5.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param int $site_id The ID of the site to retrieve.
         * @return WP_Site|false The site's object if found. False if not.
         */
        public static function get_instance($site_id)
        {
        }
        /**
         * Creates a new WP_Site object.
         *
         * Will populate object properties from the object provided and assign other
         * default properties based on that information.
         *
         * @since 4.5.0
         *
         * @param object $site A site object.
         */
        public function __construct($site)
        {
        }
        /**
         * Converts an object to array.
         *
         * @since 4.6.0
         *
         * @return array Object as array.
         */
        public function to_array()
        {
        }
        /**
         * Getter.
         *
         * Allows current multisite naming conventions when getting properties.
         * Allows access to extended site properties.
         *
         * @since 4.6.0
         *
         * @param string $key Property to get.
         * @return mixed Value of the property. Null if not available.
         */
        public function __get($key)
        {
        }
        /**
         * Isset-er.
         *
         * Allows current multisite naming conventions when checking for properties.
         * Checks for extended site properties.
         *
         * @since 4.6.0
         *
         * @param string $key Property to check if set.
         * @return bool Whether the property is set.
         */
        public function __isset($key)
        {
        }
        /**
         * Setter.
         *
         * Allows current multisite naming conventions while setting properties.
         *
         * @since 4.6.0
         *
         * @param string $key   Property to set.
         * @param mixed  $value Value to assign to the property.
         */
        public function __set($key, $value)
        {
        }
    }
    /**
     * Core class used to register styles.
     *
     * @since 2.6.0
     *
     * @see WP_Dependencies
     */
    class WP_Styles extends \WP_Dependencies
    {
        /**
         * Base URL for styles.
         *
         * Full URL with trailing slash.
         *
         * @since 2.6.0
         * @see wp_default_styles()
         * @var string|null
         */
        public $base_url;
        /**
         * URL of the content directory.
         *
         * @since 2.8.0
         * @see wp_default_styles()
         * @var string|null
         */
        public $content_url;
        /**
         * Default version string for stylesheets.
         *
         * @since 2.6.0
         * @see wp_default_styles()
         * @var string|null
         */
        public $default_version;
        /**
         * The current text direction.
         *
         * @since 2.6.0
         * @see wp_default_styles()
         * @var string
         */
        public $text_direction = 'ltr';
        /**
         * Holds a list of style handles which will be concatenated.
         *
         * @since 2.8.0
         * @var string
         */
        public $concat = '';
        /**
         * Holds a string which contains style handles and their version.
         *
         * @since 2.8.0
         * @deprecated 3.4.0
         * @var string
         */
        public $concat_version = '';
        /**
         * Whether to perform concatenation.
         *
         * @since 2.8.0
         * @var bool
         */
        public $do_concat = \false;
        /**
         * Holds HTML markup of styles and additional data if concatenation
         * is enabled.
         *
         * @since 2.8.0
         * @var string
         */
        public $print_html = '';
        /**
         * Holds inline styles if concatenation is enabled.
         *
         * @since 3.3.0
         * @var string
         */
        public $print_code = '';
        /**
         * List of default directories.
         *
         * @since 2.8.0
         * @see wp_default_styles()
         * @var string[]|null
         */
        public $default_dirs;
        /**
         * Constructor.
         *
         * @since 2.6.0
         */
        public function __construct()
        {
        }
        /**
         * Processes a style dependency.
         *
         * @since 2.6.0
         * @since 5.5.0 Added the `$group` parameter.
         *
         * @see WP_Dependencies::do_item()
         *
         * @param string    $handle The style's registered handle.
         * @param int|false $group  Optional. Group level: level (int), no groups (false).
         *                          Default false.
         * @return bool True on success, false on failure.
         */
        public function do_item($handle, $group = \false)
        {
        }
        /**
         * Adds extra CSS styles to a registered stylesheet.
         *
         * @since 3.3.0
         *
         * @param string $handle The style's registered handle.
         * @param string $code   String containing the CSS styles to be added.
         * @return bool True on success, false on failure.
         */
        public function add_inline_style($handle, $code)
        {
        }
        /**
         * Prints extra CSS styles of a registered stylesheet.
         *
         * @since 3.3.0
         *
         * @param string $handle  The style's registered handle.
         * @param bool   $display Optional. Whether to print the inline style
         *                        instead of just returning it. Default true.
         * @return string|bool False if no data exists, inline styles if `$display` is true,
         *                     true otherwise.
         */
        public function print_inline_style($handle, $display = \true)
        {
        }
        /**
         * Overrides the add_data method from WP_Dependencies, to allow unsetting dependencies for conditional styles.
         *
         * @since 6.9.0
         *
         * @param string $handle Name of the item. Should be unique.
         * @param string $key    The data key.
         * @param mixed  $value  The data value.
         * @return bool True on success, false on failure.
         */
        public function add_data($handle, $key, $value)
        {
        }
        /**
         * Determines style dependencies.
         *
         * @since 2.6.0
         *
         * @see WP_Dependencies::all_deps()
         *
         * @param string|string[] $handles   Item handle (string) or item handles (array of strings).
         * @param bool            $recursion Optional. Internal flag that function is calling itself.
         *                                   Default false.
         * @param int|false       $group     Optional. Group level: level (int), no groups (false).
         *                                   Default false.
         * @return bool True on success, false on failure.
         */
        public function all_deps($handles, $recursion = \false, $group = \false)
        {
        }
        /**
         * Generates an enqueued style's fully-qualified URL.
         *
         * @since 2.6.0
         *
         * @param string            $src    The source of the enqueued style.
         * @param string|false|null $ver    The version of the enqueued style.
         * @param string            $handle The style's registered handle.
         * @return string Style's fully-qualified URL.
         */
        public function _css_href($src, $ver, $handle)
        {
        }
        /**
         * Whether a handle's source is in a default directory.
         *
         * @since 2.8.0
         *
         * @param string $src The source of the enqueued style.
         * @return bool True if found, false if not.
         */
        public function in_default_dir($src)
        {
        }
        /**
         * Processes items and dependencies for the footer group.
         *
         * HTML 5 allows styles in the body, grab late enqueued items and output them in the footer.
         *
         * @since 3.3.0
         *
         * @see WP_Dependencies::do_items()
         *
         * @return string[] Handles of items that have been processed.
         */
        public function do_footer_items()
        {
        }
        /**
         * Resets class properties.
         *
         * @since 3.3.0
         */
        public function reset()
        {
        }
        /**
         * Gets a style-specific dependency warning message.
         *
         * @since 6.9.1
         *
         * @param string   $handle                     Style handle with missing dependencies.
         * @param string[] $missing_dependency_handles Missing dependency handles.
         * @return string Formatted, localized warning message.
         */
        protected function get_dependency_warning_message($handle, $missing_dependency_handles)
        {
        }
    }
    /**
     * Core class used for interacting with taxonomies.
     *
     * @since 4.7.0
     */
    #[\AllowDynamicProperties]
    final class WP_Taxonomy
    {
        /**
         * Taxonomy key.
         *
         * @since 4.7.0
         * @var string
         */
        public $name;
        /**
         * Name of the taxonomy shown in the menu. Usually plural.
         *
         * @since 4.7.0
         * @var string
         */
        public $label;
        /**
         * Labels object for this taxonomy.
         *
         * If not set, tag labels are inherited for non-hierarchical types
         * and category labels for hierarchical ones.
         *
         * @see get_taxonomy_labels()
         *
         * @since 4.7.0
         * @var stdClass
         */
        public $labels;
        /**
         * A short descriptive summary of what the taxonomy is for.
         *
         * @since 4.7.0
         * @var string
         */
        public $description = '';
        /**
         * Whether a taxonomy is intended for use publicly either via the admin interface or by front-end users.
         *
         * @since 4.7.0
         * @var bool
         */
        public $public = \true;
        /**
         * Whether the taxonomy is publicly queryable.
         *
         * @since 4.7.0
         * @var bool
         */
        public $publicly_queryable = \true;
        /**
         * Whether the taxonomy is hierarchical.
         *
         * @since 4.7.0
         * @var bool
         */
        public $hierarchical = \false;
        /**
         * Whether to generate and allow a UI for managing terms in this taxonomy in the admin.
         *
         * @since 4.7.0
         * @var bool
         */
        public $show_ui = \true;
        /**
         * Whether to show the taxonomy in the admin menu.
         *
         * If true, the taxonomy is shown as a submenu of the object type menu. If false, no menu is shown.
         *
         * @since 4.7.0
         * @var bool
         */
        public $show_in_menu = \true;
        /**
         * Whether the taxonomy is available for selection in navigation menus.
         *
         * @since 4.7.0
         * @var bool
         */
        public $show_in_nav_menus = \true;
        /**
         * Whether to list the taxonomy in the tag cloud widget controls.
         *
         * @since 4.7.0
         * @var bool
         */
        public $show_tagcloud = \true;
        /**
         * Whether to show the taxonomy in the quick/bulk edit panel.
         *
         * @since 4.7.0
         * @var bool
         */
        public $show_in_quick_edit = \true;
        /**
         * Whether to display a column for the taxonomy on its post type listing screens.
         *
         * @since 4.7.0
         * @var bool
         */
        public $show_admin_column = \false;
        /**
         * The callback function for the meta box display.
         *
         * @since 4.7.0
         * @var bool|callable
         */
        public $meta_box_cb = \null;
        /**
         * The callback function for sanitizing taxonomy data saved from a meta box.
         *
         * @since 5.1.0
         * @var callable
         */
        public $meta_box_sanitize_cb = \null;
        /**
         * An array of object types this taxonomy is registered for.
         *
         * @since 4.7.0
         * @var string[]
         */
        public $object_type = \null;
        /**
         * Capabilities for this taxonomy.
         *
         * @since 4.7.0
         * @var stdClass
         */
        public $cap;
        /**
         * Rewrites information for this taxonomy.
         *
         * @since 4.7.0
         * @var array|false
         */
        public $rewrite;
        /**
         * Query var string for this taxonomy.
         *
         * @since 4.7.0
         * @var string|false
         */
        public $query_var;
        /**
         * Function that will be called when the count is updated.
         *
         * @since 4.7.0
         * @var callable
         */
        public $update_count_callback;
        /**
         * Whether this taxonomy should appear in the REST API.
         *
         * Default false. If true, standard endpoints will be registered with
         * respect to $rest_base and $rest_controller_class.
         *
         * @since 4.7.4
         * @var bool $show_in_rest
         */
        public $show_in_rest;
        /**
         * The base path for this taxonomy's REST API endpoints.
         *
         * @since 4.7.4
         * @var string|bool $rest_base
         */
        public $rest_base;
        /**
         * The namespace for this taxonomy's REST API endpoints.
         *
         * @since 5.9.0
         * @var string|bool $rest_namespace
         */
        public $rest_namespace;
        /**
         * The controller for this taxonomy's REST API endpoints.
         *
         * Custom controllers must extend WP_REST_Controller.
         *
         * @since 4.7.4
         * @var string|bool $rest_controller_class
         */
        public $rest_controller_class;
        /**
         * The controller instance for this taxonomy's REST API endpoints.
         *
         * Lazily computed. Should be accessed using {@see WP_Taxonomy::get_rest_controller()}.
         *
         * @since 5.5.0
         * @var WP_REST_Controller $rest_controller
         */
        public $rest_controller;
        /**
         * The default term name for this taxonomy. If you pass an array you have
         * to set 'name' and optionally 'slug' and 'description'.
         *
         * @since 5.5.0
         * @var array|string
         */
        public $default_term;
        /**
         * Whether terms in this taxonomy should be sorted in the order they are provided to `wp_set_object_terms()`.
         *
         * Use this in combination with `'orderby' => 'term_order'` when fetching terms.
         *
         * @since 2.5.0
         * @var bool|null
         */
        public $sort = \null;
        /**
         * Array of arguments to automatically use inside `wp_get_object_terms()` for this taxonomy.
         *
         * @since 2.6.0
         * @var array|null
         */
        public $args = \null;
        /**
         * Whether it is a built-in taxonomy.
         *
         * @since 4.7.0
         * @var bool
         */
        public $_builtin;
        /**
         * Constructor.
         *
         * See the register_taxonomy() function for accepted arguments for `$args`.
         *
         * @since 4.7.0
         *
         * @param string       $taxonomy    Taxonomy key, must not exceed 32 characters.
         * @param array|string $object_type Name of the object type for the taxonomy object.
         * @param array|string $args        Optional. Array or query string of arguments for registering a taxonomy.
         *                                  See register_taxonomy() for information on accepted arguments.
         *                                  Default empty array.
         * @phpstan-param array{
         *   labels?: string[],
         *   description?: string,
         *   public?: bool,
         *   publicly_queryable?: bool,
         *   hierarchical?: bool,
         *   show_ui?: bool,
         *   show_in_menu?: bool,
         *   show_in_nav_menus?: bool,
         *   show_in_rest?: bool,
         *   rest_base?: string,
         *   rest_namespace?: string,
         *   rest_controller_class?: string,
         *   show_tagcloud?: bool,
         *   show_in_quick_edit?: bool,
         *   show_admin_column?: bool,
         *   meta_box_cb?: bool|callable,
         *   meta_box_sanitize_cb?: callable,
         *   capabilities?: array{
         *     manage_terms?: string,
         *     edit_terms?: string,
         *     delete_terms?: string,
         *     assign_terms?: string,
         *   },
         *   rewrite?: bool|array{
         *     slug?: string,
         *     with_front?: bool,
         *     hierarchical?: bool,
         *     ep_mask?: int,
         *   },
         *   query_var?: string|bool,
         *   update_count_callback?: callable,
         *   default_term?: string|array{
         *     name?: string,
         *     slug?: string,
         *     description?: string,
         *   },
         *   sort?: bool,
         *   args?: array,
         *   _builtin?: bool,
         * } $args See register_taxonomy()
         */
        public function __construct($taxonomy, $object_type, $args = array())
        {
        }
        /**
         * Sets taxonomy properties.
         *
         * See the register_taxonomy() function for accepted arguments for `$args`.
         *
         * @since 4.7.0
         *
         * @param string|string[] $object_type Name or array of names of the object types for the taxonomy.
         * @param array|string    $args        Array or query string of arguments for registering a taxonomy.
         */
        public function set_props($object_type, $args)
        {
        }
        /**
         * Adds the necessary rewrite rules for the taxonomy.
         *
         * @since 4.7.0
         *
         * @global WP $wp Current WordPress environment instance.
         */
        public function add_rewrite_rules()
        {
        }
        /**
         * Removes any rewrite rules, permastructs, and rules for the taxonomy.
         *
         * @since 4.7.0
         *
         * @global WP $wp Current WordPress environment instance.
         */
        public function remove_rewrite_rules()
        {
        }
        /**
         * Registers the ajax callback for the meta box.
         *
         * @since 4.7.0
         */
        public function add_hooks()
        {
        }
        /**
         * Removes the ajax callback for the meta box.
         *
         * @since 4.7.0
         */
        public function remove_hooks()
        {
        }
        /**
         * Gets the REST API controller for this taxonomy.
         *
         * Will only instantiate the controller class once per request.
         *
         * @since 5.5.0
         *
         * @return WP_REST_Controller|null The controller instance, or null if the taxonomy
         *                                 is set not to show in rest.
         */
        public function get_rest_controller()
        {
        }
        /**
         * Returns the default labels for taxonomies.
         *
         * @since 6.0.0
         *
         * @return (string|null)[][] The default labels for taxonomies.
         */
        public static function get_default_labels()
        {
        }
        /**
         * Resets the cache for the default labels.
         *
         * @since 6.0.0
         */
        public static function reset_default_labels()
        {
        }
    }
    /**
     * Class used for querying terms.
     *
     * @since 4.6.0
     *
     * @see WP_Term_Query::__construct() for accepted arguments.
     */
    #[\AllowDynamicProperties]
    class WP_Term_Query
    {
        /**
         * SQL string used to perform database query.
         *
         * @since 4.6.0
         * @var string
         */
        public $request;
        /**
         * Metadata query container.
         *
         * @since 4.6.0
         * @var WP_Meta_Query A meta query instance.
         */
        public $meta_query = \false;
        /**
         * Metadata query clauses.
         *
         * @since 4.6.0
         * @var array
         */
        protected $meta_query_clauses;
        /**
         * SQL query clauses.
         *
         * @since 4.6.0
         * @var array
         */
        protected $sql_clauses = array('select' => '', 'from' => '', 'where' => array(), 'orderby' => '', 'limits' => '');
        /**
         * Query vars set by the user.
         *
         * @since 4.6.0
         * @var array
         */
        public $query_vars;
        /**
         * Default values for query vars.
         *
         * @since 4.6.0
         * @var array
         */
        public $query_var_defaults;
        /**
         * List of terms located by the query.
         *
         * @since 4.6.0
         * @var array
         */
        public $terms;
        /**
         * Constructor.
         *
         * Sets up the term query, based on the query vars passed.
         *
         * @since 4.6.0
         * @since 4.6.0 Introduced 'term_taxonomy_id' parameter.
         * @since 4.7.0 Introduced 'object_ids' parameter.
         * @since 4.9.0 Added 'slug__in' support for 'orderby'.
         * @since 5.1.0 Introduced the 'meta_compare_key' parameter.
         * @since 5.3.0 Introduced the 'meta_type_key' parameter.
         * @since 6.4.0 Introduced the 'cache_results' parameter.
         *
         * @param string|array $query {
         *     Optional. Array or query string of term query parameters. Default empty.
         *
         *     @type string|string[] $taxonomy               Taxonomy name, or array of taxonomy names, to which results
         *                                                   should be limited.
         *     @type int|int[]       $object_ids             Object ID, or array of object IDs. Results will be
         *                                                   limited to terms associated with these objects.
         *     @type string          $orderby                Field(s) to order terms by. Accepts:
         *                                                   - Term fields ('name', 'slug', 'term_group', 'term_id', 'id',
         *                                                     'description', 'parent', 'term_order'). Unless `$object_ids`
         *                                                     is not empty, 'term_order' is treated the same as 'term_id'.
         *                                                   - 'count' to use the number of objects associated with the term.
         *                                                   - 'include' to match the 'order' of the `$include` param.
         *                                                   - 'slug__in' to match the 'order' of the `$slug` param.
         *                                                   - 'meta_value'
         *                                                   - 'meta_value_num'.
         *                                                   - The value of `$meta_key`.
         *                                                   - The array keys of `$meta_query`.
         *                                                   - 'none' to omit the ORDER BY clause.
         *                                                   Default 'name'.
         *     @type string          $order                  Whether to order terms in ascending or descending order.
         *                                                   Accepts 'ASC' (ascending) or 'DESC' (descending).
         *                                                   Default 'ASC'.
         *     @type bool|int        $hide_empty             Whether to hide terms not assigned to any posts. Accepts
         *                                                   1|true or 0|false. Default 1|true.
         *     @type int[]|string    $include                Array or comma/space-separated string of term IDs to include.
         *                                                   Default empty array.
         *     @type int[]|string    $exclude                Array or comma/space-separated string of term IDs to exclude.
         *                                                   If `$include` is non-empty, `$exclude` is ignored.
         *                                                   Default empty array.
         *     @type int[]|string    $exclude_tree           Array or comma/space-separated string of term IDs to exclude
         *                                                   along with all of their descendant terms. If `$include` is
         *                                                   non-empty, `$exclude_tree` is ignored. Default empty array.
         *     @type int|string      $number                 Maximum number of terms to return. Accepts ''|0 (all) or any
         *                                                   positive number. Default ''|0 (all). Note that `$number` may
         *                                                   not return accurate results when coupled with `$object_ids`.
         *                                                   See #41796 for details.
         *     @type int             $offset                 The number by which to offset the terms query. Default empty.
         *     @type string          $fields                 Term fields to query for. Accepts:
         *                                                   - 'all' Returns an array of complete term objects (`WP_Term[]`).
         *                                                   - 'all_with_object_id' Returns an array of term objects
         *                                                     with the 'object_id' param (`WP_Term[]`). Works only
         *                                                     when the `$object_ids` parameter is populated.
         *                                                   - 'ids' Returns an array of term IDs (`int[]`).
         *                                                   - 'tt_ids' Returns an array of term taxonomy IDs (`int[]`).
         *                                                   - 'names' Returns an array of term names (`string[]`).
         *                                                   - 'slugs' Returns an array of term slugs (`string[]`).
         *                                                   - 'count' Returns the number of matching terms (`int`).
         *                                                   - 'id=>parent' Returns an associative array of parent term IDs,
         *                                                      keyed by term ID (`int[]`).
         *                                                   - 'id=>name' Returns an associative array of term names,
         *                                                      keyed by term ID (`string[]`).
         *                                                   - 'id=>slug' Returns an associative array of term slugs,
         *                                                      keyed by term ID (`string[]`).
         *                                                   Default 'all'.
         *     @type string|string[] $name                   Name or array of names to return term(s) for.
         *                                                   Default empty.
         *     @type string|string[] $slug                   Slug or array of slugs to return term(s) for.
         *                                                   Default empty.
         *     @type int|int[]       $term_taxonomy_id       Term taxonomy ID, or array of term taxonomy IDs,
         *                                                   to match when querying terms.
         *     @type bool            $hierarchical           Whether to include terms that have non-empty descendants
         *                                                   (even if `$hide_empty` is set to true). Default true.
         *     @type string          $search                 Search criteria to match terms. Will be SQL-formatted with
         *                                                   wildcards before and after. Default empty.
         *     @type string          $name__like             Retrieve terms with criteria by which a term is LIKE
         *                                                   `$name__like`. Default empty.
         *     @type string          $description__like      Retrieve terms where the description is LIKE
         *                                                   `$description__like`. Default empty.
         *     @type bool            $pad_counts             Whether to pad the quantity of a term's children in the
         *                                                   quantity of each term's "count" object variable.
         *                                                   Default false.
         *     @type string          $get                    Whether to return terms regardless of ancestry or whether the
         *                                                   terms are empty. Accepts 'all' or '' (disabled).
         *                                                   Default ''.
         *     @type int             $child_of               Term ID to retrieve child terms of. If multiple taxonomies
         *                                                   are passed, `$child_of` is ignored. Default 0.
         *     @type int             $parent                 Parent term ID to retrieve direct-child terms of.
         *                                                   Default empty.
         *     @type bool            $childless              True to limit results to terms that have no children.
         *                                                   This parameter has no effect on non-hierarchical taxonomies.
         *                                                   Default false.
         *     @type string          $cache_domain           Unique cache key to be produced when this query is stored in
         *                                                   an object cache. Default 'core'.
         *     @type bool            $cache_results          Whether to cache term information. Default true.
         *     @type bool            $update_term_meta_cache Whether to prime meta caches for matched terms. Default true.
         *     @type string|string[] $meta_key               Meta key or keys to filter by.
         *     @type string|string[] $meta_value             Meta value or values to filter by.
         *     @type string          $meta_compare           MySQL operator used for comparing the meta value.
         *                                                   See WP_Meta_Query::__construct() for accepted values and default value.
         *     @type string          $meta_compare_key       MySQL operator used for comparing the meta key.
         *                                                   See WP_Meta_Query::__construct() for accepted values and default value.
         *     @type string          $meta_type              MySQL data type that the meta_value column will be CAST to for comparisons.
         *                                                   See WP_Meta_Query::__construct() for accepted values and default value.
         *     @type string          $meta_type_key          MySQL data type that the meta_key column will be CAST to for comparisons.
         *                                                   See WP_Meta_Query::__construct() for accepted values and default value.
         *     @type array           $meta_query             An associative array of WP_Meta_Query arguments.
         *                                                   See WP_Meta_Query::__construct() for accepted values.
         * }
         * @phpstan-param array{
         *   taxonomy?: string|string[],
         *   object_ids?: int|int[],
         *   orderby?: string,
         *   order?: string,
         *   hide_empty?: bool|int,
         *   include?: int[]|string,
         *   exclude?: int[]|string,
         *   exclude_tree?: int[]|string,
         *   number?: int|string,
         *   offset?: int,
         *   fields?: string,
         *   name?: string|string[],
         *   slug?: string|string[],
         *   term_taxonomy_id?: int|int[],
         *   hierarchical?: bool,
         *   search?: string,
         *   name__like?: string,
         *   description__like?: string,
         *   pad_counts?: bool,
         *   get?: string,
         *   child_of?: int,
         *   parent?: int,
         *   childless?: bool,
         *   cache_domain?: string,
         *   cache_results?: bool,
         *   update_term_meta_cache?: bool,
         *   meta_key?: string|string[],
         *   meta_value?: string|string[],
         *   meta_compare?: string,
         *   meta_compare_key?: string,
         *   meta_type?: string,
         *   meta_type_key?: string,
         *   meta_query?: array,
         * } $query
         */
        public function __construct($query = '')
        {
        }
        /**
         * Parse arguments passed to the term query with default query parameters.
         *
         * @since 4.6.0
         *
         * @param string|array $query WP_Term_Query arguments. See WP_Term_Query::__construct() for accepted arguments.
         * @phpstan-param array{
         *   taxonomy?: string|string[],
         *   object_ids?: int|int[],
         *   orderby?: string,
         *   order?: string,
         *   hide_empty?: bool|int,
         *   include?: int[]|string,
         *   exclude?: int[]|string,
         *   exclude_tree?: int[]|string,
         *   number?: int|string,
         *   offset?: int,
         *   fields?: string,
         *   name?: string|string[],
         *   slug?: string|string[],
         *   term_taxonomy_id?: int|int[],
         *   hierarchical?: bool,
         *   search?: string,
         *   name__like?: string,
         *   description__like?: string,
         *   pad_counts?: bool,
         *   get?: string,
         *   child_of?: int,
         *   parent?: int,
         *   childless?: bool,
         *   cache_domain?: string,
         *   cache_results?: bool,
         *   update_term_meta_cache?: bool,
         *   meta_key?: string|string[],
         *   meta_value?: string|string[],
         *   meta_compare?: string,
         *   meta_compare_key?: string,
         *   meta_type?: string,
         *   meta_type_key?: string,
         *   meta_query?: array,
         * } $query See WP_Term_Query::__construct()
         */
        public function parse_query($query = '')
        {
        }
        /**
         * Sets up the query and retrieves the results.
         *
         * The return type varies depending on the value passed to `$args['fields']`. See
         * WP_Term_Query::get_terms() for details.
         *
         * @since 4.6.0
         *
         * @param string|array $query Array or URL query string of parameters.
         * @return WP_Term[]|int[]|string[]|string Array of terms, or number of terms as numeric string
         *                                         when 'count' is passed to `$args['fields']`.
         * @phpstan-return ($query is array ? ($query is array{fields: 'count', ...} ? 0|numeric-string : ($query is array{fields: 'names'|'slugs', ...} ? list<string> : ($query is array{fields: 'id=>name'|'id=>slug', ...} ? array<int, string> : ($query is array{fields: 'id=>parent', ...} ? array<int, int> : ($query is array{fields: 'ids'|'tt_ids', ...} ? list<int> : array<int, \WP_Term>))))) : 0|numeric-string|array<int, int|string|\WP_Term>)
         */
        public function query($query)
        {
        }
        /**
         * Retrieves the query results.
         *
         * The return type varies depending on the value passed to `$args['fields']`.
         *
         * The following will result in an array of `WP_Term` objects being returned:
         *
         *   - 'all'
         *   - 'all_with_object_id'
         *
         * The following will result in a numeric string being returned:
         *
         *   - 'count'
         *
         * The following will result in an array of text strings being returned:
         *
         *   - 'id=>name'
         *   - 'id=>slug'
         *   - 'names'
         *   - 'slugs'
         *
         * The following will result in an array of numeric strings being returned:
         *
         *   - 'id=>parent'
         *
         * The following will result in an array of integers being returned:
         *
         *   - 'ids'
         *   - 'tt_ids'
         *
         * @since 4.6.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @return WP_Term[]|int[]|string[]|string Array of terms, or number of terms as numeric string
         *                                         when 'count' is passed to `$args['fields']`.
         * @phpstan-return 0|numeric-string|array<int, int|string|\WP_Term>
         */
        public function get_terms()
        {
        }
        /**
         * Parse and sanitize 'orderby' keys passed to the term query.
         *
         * @since 4.6.0
         *
         * @param string $orderby_raw Alias for the field to order by.
         * @return string|false Value to used in the ORDER clause. False otherwise.
         */
        protected function parse_orderby($orderby_raw)
        {
        }
        /**
         * Format response depending on field requested.
         *
         * @since 6.0.0
         *
         * @param WP_Term[] $term_objects Array of term objects.
         * @param string    $_fields      Field to format.
         *
         * @return WP_Term[]|int[]|string[] Array of terms / strings / ints depending on field requested.
         */
        protected function format_terms($term_objects, $_fields)
        {
        }
        /**
         * Generate the ORDER BY clause for an 'orderby' param that is potentially related to a meta query.
         *
         * @since 4.6.0
         *
         * @param string $orderby_raw Raw 'orderby' value passed to WP_Term_Query.
         * @return string ORDER BY clause.
         */
        protected function parse_orderby_meta($orderby_raw)
        {
        }
        /**
         * Parse an 'order' query variable and cast it to ASC or DESC as necessary.
         *
         * @since 4.6.0
         *
         * @param string $order The 'order' query variable.
         * @return string The sanitized 'order' query variable.
         */
        protected function parse_order($order)
        {
        }
        /**
         * Used internally to generate a SQL string related to the 'search' parameter.
         *
         * @since 4.6.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param string $search Search string.
         * @return string Search SQL.
         */
        protected function get_search_sql($search)
        {
        }
        /**
         * Creates an array of term objects from an array of term IDs.
         *
         * Also discards invalid term objects.
         *
         * @since 4.9.8
         *
         * @param Object[]|int[] $terms List of objects or term ids.
         * @return WP_Term[] Array of `WP_Term` objects.
         */
        protected function populate_terms($terms)
        {
        }
        /**
         * Generate cache key.
         *
         * @since 6.2.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param array  $args WP_Term_Query arguments.
         * @param string $sql  SQL statement.
         *
         * @return string Cache key.
         */
        protected function generate_cache_key(array $args, $sql)
        {
        }
    }
    /**
     * Core class used to implement the WP_Term object.
     *
     * @since 4.4.0
     *
     * @property-read object $data Sanitized term data.
     */
    #[\AllowDynamicProperties]
    final class WP_Term
    {
        /**
         * Term ID.
         *
         * @since 4.4.0
         * @var int
         */
        public $term_id;
        /**
         * The term's name.
         *
         * @since 4.4.0
         * @var string
         */
        public $name = '';
        /**
         * The term's slug.
         *
         * @since 4.4.0
         * @var string
         */
        public $slug = '';
        /**
         * The term's term_group.
         *
         * @since 4.4.0
         * @var int
         */
        public $term_group = '';
        /**
         * Term Taxonomy ID.
         *
         * @since 4.4.0
         * @var int
         */
        public $term_taxonomy_id = 0;
        /**
         * The term's taxonomy name.
         *
         * @since 4.4.0
         * @var string
         */
        public $taxonomy = '';
        /**
         * The term's description.
         *
         * @since 4.4.0
         * @var string
         */
        public $description = '';
        /**
         * ID of a term's parent term.
         *
         * @since 4.4.0
         * @var int
         */
        public $parent = 0;
        /**
         * Cached object count for this term.
         *
         * @since 4.4.0
         * @var int
         */
        public $count = 0;
        /**
         * Stores the term object's sanitization level.
         *
         * Does not correspond to a database field.
         *
         * @since 4.4.0
         * @var string
         */
        public $filter = 'raw';
        /**
         * Retrieve WP_Term instance.
         *
         * @since 4.4.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param int    $term_id  Term ID.
         * @param string $taxonomy Optional. Limit matched terms to those matching `$taxonomy`. Only used for
         *                         disambiguating potentially shared terms.
         * @return WP_Term|WP_Error|false Term object, if found. WP_Error if `$term_id` is shared between taxonomies and
         *                                there's insufficient data to distinguish which term is intended.
         *                                False for other failures.
         */
        public static function get_instance($term_id, $taxonomy = \null)
        {
        }
        /**
         * Constructor.
         *
         * @since 4.4.0
         *
         * @param object $term Term object.
         */
        public function __construct($term)
        {
        }
        /**
         * Sanitizes term fields, according to the filter type provided.
         *
         * @since 4.4.0
         *
         * @param string $filter Filter context. Accepts 'edit', 'db', 'display', 'attribute', 'js', 'rss', or 'raw'.
         * @phpstan-param 'edit'|'db'|'display'|'attribute'|'js'|'rss'|'raw' $filter
         */
        public function filter($filter)
        {
        }
        /**
         * Converts an object to array.
         *
         * @since 4.4.0
         *
         * @return array Object as array.
         */
        public function to_array()
        {
        }
        /**
         * Getter.
         *
         * @since 4.4.0
         *
         * @param string $key Property to get.
         * @return mixed Property value.
         */
        public function __get($key)
        {
        }
    }
    /**
     * WP_Theme Class
     *
     * @package WordPress
     * @subpackage Theme
     * @since 3.4.0
     * @phpstan-type ThemeKey 'Name'|'Version'|'Status'|'Title'|'Author'|'Author Name'|'Author URI'|'Description'|'Template'|'Stylesheet'|'Template Files'|'Stylesheet Files'|'Template Dir'|'Stylesheet Dir'|'Screenshot'|'Tags'|'Theme Root'|'Theme Root URI'|'Parent Theme'
     * @phpstan-property-read string $name
     * @phpstan-property-read string $title
     * @phpstan-property-read string $version
     * @phpstan-property-read string $parent_theme
     * @phpstan-property-read string $template_dir
     * @phpstan-property-read string $stylesheet_dir
     * @phpstan-property-read string $template
     * @phpstan-property-read string $stylesheet
     * @phpstan-property-read string $screenshot
     * @phpstan-property-read string $description
     * @phpstan-property-read string $author
     * @phpstan-property-read list<string> $tags
     * @phpstan-property-read string $theme_root
     * @phpstan-property-read string $theme_root_uri
     */
    #[\AllowDynamicProperties]
    final class WP_Theme implements \ArrayAccess
    {
        /**
         * Whether the theme has been marked as updateable.
         *
         * @since 4.4.0
         * @var bool
         *
         * @see WP_MS_Themes_List_Table
         */
        public $update = \false;
        /**
         * Constructor for WP_Theme.
         *
         * @since 3.4.0
         *
         * @global string[] $wp_theme_directories
         *
         * @param string        $theme_dir  Directory of the theme within the theme_root.
         * @param string        $theme_root Theme root.
         * @param WP_Theme|null $_child If this theme is a parent theme, the child may be passed for validation purposes.
         * @phpstan-return void
         */
        public function __construct($theme_dir, $theme_root, $_child = \null)
        {
        }
        /**
         * When converting the object to a string, the theme name is returned.
         *
         * @since 3.4.0
         *
         * @return string Theme name, ready for display (translated)
         */
        public function __toString()
        {
        }
        /**
         * __isset() magic method for properties formerly returned by current_theme_info()
         *
         * @since 3.4.0
         *
         * @param string $offset Property to check if set.
         * @return bool Whether the given property is set.
         */
        public function __isset($offset)
        {
        }
        /**
         * __get() magic method for properties formerly returned by current_theme_info()
         *
         * @since 3.4.0
         *
         * @param string $offset Property to get.
         * @return mixed Property value.
         */
        public function __get($offset)
        {
        }
        /**
         * Method to implement ArrayAccess for keys formerly returned by get_themes()
         *
         * @since 3.4.0
         *
         * @param mixed $offset
         * @param mixed $value
         */
        #[\ReturnTypeWillChange]
        public function offsetSet($offset, $value)
        {
        }
        /**
         * Method to implement ArrayAccess for keys formerly returned by get_themes()
         *
         * @since 3.4.0
         *
         * @param mixed $offset
         */
        #[\ReturnTypeWillChange]
        public function offsetUnset($offset)
        {
        }
        /**
         * Method to implement ArrayAccess for keys formerly returned by get_themes()
         *
         * @since 3.4.0
         *
         * @param mixed $offset
         * @return bool
         * @phpstan-return ($offset is ThemeKey ? true : false)
         */
        #[\ReturnTypeWillChange]
        public function offsetExists($offset)
        {
        }
        /**
         * Method to implement ArrayAccess for keys formerly returned by get_themes().
         *
         * Author, Author Name, Author URI, and Description did not previously return
         * translated data. We are doing so now as it is safe to do. However, as
         * Name and Title could have been used as the key for get_themes(), both remain
         * untranslated for back compatibility. This means that ['Name'] is not ideal,
         * and care should be taken to use `$theme::display( 'Name' )` to get a properly
         * translated header.
         *
         * @since 3.4.0
         *
         * @param mixed $offset
         * @return mixed
         * @phpstan-return ($offset is ThemeKey ? mixed : null)
         */
        #[\ReturnTypeWillChange]
        public function offsetGet($offset)
        {
        }
        /**
         * Returns errors property.
         *
         * @since 3.4.0
         *
         * @return WP_Error|false WP_Error if there are errors, or false.
         */
        public function errors()
        {
        }
        /**
         * Determines whether the theme exists.
         *
         * A theme with errors exists. A theme with the error of 'theme_not_found',
         * meaning that the theme's directory was not found, does not exist.
         *
         * @since 3.4.0
         *
         * @return bool Whether the theme exists.
         */
        public function exists()
        {
        }
        /**
         * Returns reference to the parent theme.
         *
         * @since 3.4.0
         *
         * @return WP_Theme|false Parent theme, or false if the active theme is not a child theme.
         */
        public function parent()
        {
        }
        /**
         * Perform reinitialization tasks.
         *
         * Prevents a callback from being injected during unserialization of an object.
         */
        public function __wakeup()
        {
        }
        /**
         * Clears the cache for the theme.
         *
         * @since 3.4.0
         */
        public function cache_delete()
        {
        }
        /**
         * Gets a raw, unformatted theme header.
         *
         * The header is sanitized, but is not translated, and is not marked up for display.
         * To get a theme header for display, use the display() method.
         *
         * Use the get_template() method, not the 'Template' header, for finding the template.
         * The 'Template' header is only good for what was written in the style.css, while
         * get_template() takes into account where WordPress actually located the theme and
         * whether it is actually valid.
         *
         * @since 3.4.0
         *
         * @param string $header Theme header. Name, Description, Author, Version, ThemeURI, AuthorURI, Status, Tags.
         * @return string|array|false String or array (for Tags header) on success, false on failure.
         *
         * @phpstan-return (
         *     $header is 'Tags'
         *         ? string[]|false
         *         : ( $header is 'Name'|'ThemeURI'|'Description'|'Author'|'AuthorURI'|'Version'|'Template'|'Status'|'TextDomain'|'DomainPath'|'RequiresWP'|'RequiresPHP'|'UpdateURI'
         *             ? string|false
         *             : false )
         * )
         */
        public function get($header)
        {
        }
        /**
         * Gets a theme header, formatted and translated for display.
         *
         * @since 3.4.0
         *
         * @param string $header    Theme header. Name, Description, Author, Version, ThemeURI, AuthorURI, Status, Tags.
         * @param bool   $markup    Optional. Whether to mark up the header. Defaults to true.
         * @param bool   $translate Optional. Whether to translate the header. Defaults to true.
         * @return string|array|false Processed header. An array for Tags if `$markup` is false, string otherwise.
         *                            False on failure.
         */
        public function display($header, $markup = \true, $translate = \true)
        {
        }
        /**
         * Returns the directory name of the theme's "stylesheet" files, inside the theme root.
         *
         * In the case of a child theme, this is directory name of the child theme.
         * Otherwise, get_stylesheet() is the same as get_template().
         *
         * @since 3.4.0
         *
         * @return string Stylesheet
         */
        public function get_stylesheet()
        {
        }
        /**
         * Returns the directory name of the theme's "template" files, inside the theme root.
         *
         * In the case of a child theme, this is the directory name of the parent theme.
         * Otherwise, the get_template() is the same as get_stylesheet().
         *
         * @since 3.4.0
         *
         * @return string Template
         */
        public function get_template()
        {
        }
        /**
         * Returns the absolute path to the directory of a theme's "stylesheet" files.
         *
         * In the case of a child theme, this is the absolute path to the directory
         * of the child theme's files.
         *
         * @since 3.4.0
         *
         * @return string Absolute path of the stylesheet directory.
         */
        public function get_stylesheet_directory()
        {
        }
        /**
         * Returns the absolute path to the directory of a theme's "template" files.
         *
         * In the case of a child theme, this is the absolute path to the directory
         * of the parent theme's files.
         *
         * @since 3.4.0
         *
         * @return string Absolute path of the template directory.
         */
        public function get_template_directory()
        {
        }
        /**
         * Returns the URL to the directory of a theme's "stylesheet" files.
         *
         * In the case of a child theme, this is the URL to the directory of the
         * child theme's files.
         *
         * @since 3.4.0
         *
         * @return string URL to the stylesheet directory.
         */
        public function get_stylesheet_directory_uri()
        {
        }
        /**
         * Returns the URL to the directory of a theme's "template" files.
         *
         * In the case of a child theme, this is the URL to the directory of the
         * parent theme's files.
         *
         * @since 3.4.0
         *
         * @return string URL to the template directory.
         */
        public function get_template_directory_uri()
        {
        }
        /**
         * Returns the absolute path to the directory of the theme root.
         *
         * This is typically the absolute path to wp-content/themes.
         *
         * @since 3.4.0
         *
         * @return string Theme root.
         */
        public function get_theme_root()
        {
        }
        /**
         * Returns the URL to the directory of the theme root.
         *
         * This is typically the absolute URL to wp-content/themes. This forms the basis
         * for all other URLs returned by WP_Theme, so we pass it to the public function
         * get_theme_root_uri() and allow it to run the {@see 'theme_root_uri'} filter.
         *
         * @since 3.4.0
         *
         * @return string Theme root URI.
         */
        public function get_theme_root_uri()
        {
        }
        /**
         * Returns the main screenshot file for the theme.
         *
         * The main screenshot is called screenshot.png. gif and jpg extensions are also allowed.
         *
         * Screenshots for a theme must be in the stylesheet directory. (In the case of child
         * themes, parent theme screenshots are not inherited.)
         *
         * @since 3.4.0
         *
         * @param string $uri Type of URL to return, either 'relative' or an absolute URI. Defaults to absolute URI.
         * @return string|false Screenshot file. False if the theme does not have a screenshot.
         */
        public function get_screenshot($uri = 'uri')
        {
        }
        /**
         * Returns files in the theme's directory.
         *
         * @since 3.4.0
         *
         * @param string[]|string $type          Optional. Array of extensions to find, string of a single extension,
         *                                       or null for all extensions. Default null.
         * @param int             $depth         Optional. How deep to search for files. Defaults to a flat scan (0 depth).
         *                                       -1 depth is infinite.
         * @param bool            $search_parent Optional. Whether to return parent files. Default false.
         * @return string[] Array of files, keyed by the path to the file relative to the theme's directory, with the values
         *                  being absolute paths.
         */
        public function get_files($type = \null, $depth = 0, $search_parent = \false)
        {
        }
        /**
         * Returns the theme's post templates.
         *
         * @since 4.7.0
         * @since 5.8.0 Include block templates.
         *
         * @return array[] Array of page template arrays, keyed by post type and filename,
         *                 with the value of the translated header name.
         */
        public function get_post_templates()
        {
        }
        /**
         * Returns the theme's post templates for a given post type.
         *
         * @since 3.4.0
         * @since 4.7.0 Added the `$post_type` parameter.
         *
         * @param WP_Post|null $post      Optional. The post being edited, provided for context.
         * @param string       $post_type Optional. Post type to get the templates for. Default 'page'.
         *                                If a post is provided, its post type is used.
         * @return string[] Array of template header names keyed by the template file name.
         */
        public function get_page_templates($post = \null, $post_type = 'page')
        {
        }
        /**
         * Loads the theme's textdomain.
         *
         * Translation files are not inherited from the parent theme. TODO: If this fails for the
         * child theme, it should probably try to load the parent theme's translations.
         *
         * @since 3.4.0
         *
         * @return bool True if the textdomain was successfully loaded or has already been loaded.
         *  False if no textdomain was specified in the file headers, or if the domain could not be loaded.
         */
        public function load_textdomain()
        {
        }
        /**
         * Determines whether the theme is allowed (multisite only).
         *
         * @since 3.4.0
         *
         * @param string $check   Optional. Whether to check only the 'network'-wide settings, the 'site'
         *                        settings, or 'both'. Defaults to 'both'.
         * @param int    $blog_id Optional. Ignored if only network-wide settings are checked. Defaults to current site.
         * @return bool Whether the theme is allowed for the network. Returns true in single-site.
         */
        public function is_allowed($check = 'both', $blog_id = \null)
        {
        }
        /**
         * Returns whether this theme is a block-based theme or not.
         *
         * @since 5.9.0
         *
         * @return bool
         */
        public function is_block_theme()
        {
        }
        /**
         * Retrieves the path of a file in the theme.
         *
         * Searches in the stylesheet directory before the template directory so themes
         * which inherit from a parent theme can just override one file.
         *
         * @since 5.9.0
         *
         * @param string $file Optional. File to search for in the stylesheet directory.
         * @return string The path of the file.
         */
        public function get_file_path($file = '')
        {
        }
        /**
         * Determines the latest WordPress default theme that is installed.
         *
         * This hits the filesystem.
         *
         * @since 4.4.0
         *
         * @return WP_Theme|false Object, or false if no theme is installed, which would be bad.
         */
        public static function get_core_default_theme()
        {
        }
        /**
         * Returns array of stylesheet names of themes allowed on the site or network.
         *
         * @since 3.4.0
         *
         * @param int $blog_id Optional. ID of the site. Defaults to the current site.
         * @return string[] Array of stylesheet names.
         */
        public static function get_allowed($blog_id = \null)
        {
        }
        /**
         * Returns array of stylesheet names of themes allowed on the network.
         *
         * @since 3.4.0
         *
         * @return string[] Array of stylesheet names.
         */
        public static function get_allowed_on_network()
        {
        }
        /**
         * Returns array of stylesheet names of themes allowed on the site.
         *
         * @since 3.4.0
         *
         * @param int $blog_id Optional. ID of the site. Defaults to the current site.
         * @return string[] Array of stylesheet names.
         */
        public static function get_allowed_on_site($blog_id = \null)
        {
        }
        /**
         * Returns the folder names of the block template directories.
         *
         * @since 6.4.0
         *
         * @return string[] {
         *     Folder names used by block themes.
         *
         *     @type string $wp_template      Theme-relative directory name for block templates.
         *     @type string $wp_template_part Theme-relative directory name for block template parts.
         * }
         * @phpstan-return array{
         *   wp_template: string,
         *   wp_template_part: string,
         * }
         */
        public function get_block_template_folders()
        {
        }
        /**
         * Gets block pattern data for a specified theme.
         * Each pattern is defined as a PHP file and defines
         * its metadata using plugin-style headers. The minimum required definition is:
         *
         *     /**
         *      * Title: My Pattern
         *      * Slug: my-theme/my-pattern
         *      *
         *
         * The output of the PHP source corresponds to the content of the pattern, e.g.:
         *
         *     <main><p><?php echo "Hello"; ?></p></main>
         *
         * If applicable, this will collect from both parent and child theme.
         *
         * Other settable fields include:
         *
         *     - Description
         *     - Viewport Width
         *     - Inserter         (yes/no)
         *     - Categories       (comma-separated values)
         *     - Keywords         (comma-separated values)
         *     - Block Types      (comma-separated values)
         *     - Post Types       (comma-separated values)
         *     - Template Types   (comma-separated values)
         *
         * @since 6.4.0
         *
         * @return array Block pattern data.
         */
        public function get_block_patterns()
        {
        }
        /**
         * Clears block pattern cache.
         *
         * @since 6.4.0
         * @since 6.6.0 Uses transients to cache regardless of site environment.
         */
        public function delete_pattern_cache()
        {
        }
        /**
         * Enables a theme for all sites on the current network.
         *
         * @since 4.6.0
         *
         * @param string|string[] $stylesheets Stylesheet name or array of stylesheet names.
         * @phpstan-return void
         */
        public static function network_enable_theme($stylesheets)
        {
        }
        /**
         * Disables a theme for all sites on the current network.
         *
         * @since 4.6.0
         *
         * @param string|string[] $stylesheets Stylesheet name or array of stylesheet names.
         * @phpstan-return void
         */
        public static function network_disable_theme($stylesheets)
        {
        }
        /**
         * Sorts themes by name.
         *
         * @since 3.4.0
         *
         * @param WP_Theme[] $themes Array of theme objects to sort (passed by reference).
         */
        public static function sort_by_name(&$themes)
        {
        }
    }
    /**
     * Core class used to implement the WP_User object.
     *
     * @since 2.0.0
     * @since 6.8.0 The `user_pass` property is now hashed using bcrypt by default instead of phpass.
     *              Existing passwords may still be hashed using phpass.
     *
     * @property string     $nickname
     * @property string     $description
     * @property string     $user_description
     * @property string     $first_name
     * @property string     $user_firstname
     * @property string     $last_name
     * @property string     $user_lastname
     * @property string     $user_login
     * @property string     $user_pass
     * @property string     $user_nicename
     * @property string     $user_email
     * @property string     $user_url
     * @property string     $user_registered
     * @property string     $user_activation_key
     * @property string     $user_status
     * @property int|string $user_level
     * @property string     $display_name
     * @property string     $spam
     * @property string     $deleted
     * @property string     $locale
     * @property string     $rich_editing
     * @property string     $syntax_highlighting
     * @property string     $use_ssl
     *
     * @phpstan-property numeric-string        $user_status
     * @phpstan-property int|numeric-string|'' $user_level
     */
    #[\AllowDynamicProperties]
    class WP_User
    {
        /**
         * User data container.
         *
         * @since 2.0.0
         * @var stdClass
         */
        public $data;
        /**
         * The user's ID.
         *
         * @since 2.1.0
         * @var int
         */
        public $ID = 0;
        /**
         * Capabilities that the individual user has been granted outside of those inherited from their role.
         *
         * @since 2.0.0
         * @var bool[] Array of key/value pairs where keys represent a capability name
         *             and boolean values represent whether the user has that capability.
         */
        public $caps = array();
        /**
         * User metadata option name.
         *
         * @since 2.0.0
         * @var string
         */
        public $cap_key;
        /**
         * The roles the user is part of.
         *
         * @since 2.0.0
         * @var string[]
         */
        public $roles = array();
        /**
         * All capabilities the user has, including individual and role based.
         *
         * @since 2.0.0
         * @var bool[] Array of key/value pairs where keys represent a capability name
         *             and boolean values represent whether the user has that capability.
         */
        public $allcaps = array();
        /**
         * The filter context applied to user data fields.
         *
         * @since 2.9.0
         * @var string
         */
        public $filter = \null;
        /**
         * Constructor.
         *
         * Retrieves the userdata and passes it to WP_User::init().
         *
         * @since 2.0.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param int|string|object $id      User's ID, a WP_User object, or a user object from the DB.
         * @param string            $name    Optional. User's username
         * @param int               $site_id Optional Site ID, defaults to current site.
         * @phpstan-return void
         */
        public function __construct($id = 0, $name = '', $site_id = 0)
        {
        }
        /**
         * Sets up object properties, including capabilities.
         *
         * @since 3.3.0
         *
         * @param object $data    User DB row object.
         * @param int    $site_id Optional. The site ID to initialize for.
         */
        public function init($data, $site_id = 0)
        {
        }
        /**
         * Returns only the main user fields.
         *
         * @since 3.3.0
         * @since 4.4.0 Added 'ID' as an alias of 'id' for the `$field` parameter.
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param string     $field The field to query against: Accepts 'id', 'ID', 'slug', 'email' or 'login'.
         * @param string|int $value The field value.
         * @return object|false Raw user object.
         * @phpstan-param 'id'|'ID'|'slug'|'email'|'login' $field
         */
        public static function get_data_by($field, $value)
        {
        }
        /**
         * Magic method for checking the existence of a certain custom field.
         *
         * @since 3.3.0
         *
         * @param string $key User meta key to check if set.
         * @return bool Whether the given user meta key is set.
         */
        public function __isset($key)
        {
        }
        /**
         * Magic method for accessing custom fields.
         *
         * @since 3.3.0
         *
         * @param string $key User meta key to retrieve.
         * @return mixed Value of the given user meta key (if set). If `$key` is 'id', the user ID.
         */
        public function __get($key)
        {
        }
        /**
         * Magic method for setting custom user fields.
         *
         * This method does not update custom fields in the database. It only stores
         * the value on the WP_User instance.
         *
         * @since 3.3.0
         *
         * @param string $key   User meta key.
         * @param mixed  $value User meta value.
         * @phpstan-return void
         */
        public function __set($key, $value)
        {
        }
        /**
         * Magic method for unsetting a certain custom field.
         *
         * @since 4.4.0
         *
         * @param string $key User meta key to unset.
         */
        public function __unset($key)
        {
        }
        /**
         * Determines whether the user exists in the database.
         *
         * @since 3.4.0
         *
         * @return bool True if user exists in the database, false if not.
         */
        public function exists()
        {
        }
        /**
         * Retrieves the value of a property or meta key.
         *
         * Retrieves from the users and usermeta table.
         *
         * @since 3.3.0
         *
         * @param string $key Property
         * @return mixed
         */
        public function get($key)
        {
        }
        /**
         * Determines whether a property or meta key is set.
         *
         * Consults the users and usermeta tables.
         *
         * @since 3.3.0
         *
         * @param string $key Property.
         * @return bool
         */
        public function has_prop($key)
        {
        }
        /**
         * Returns an array representation.
         *
         * @since 3.5.0
         *
         * @return array Array representation.
         */
        public function to_array()
        {
        }
        /**
         * Makes private/protected methods readable for backward compatibility.
         *
         * @since 4.3.0
         *
         * @param string $name      Method to call.
         * @param array  $arguments Arguments to pass when calling.
         * @return mixed|false Return value of the callback, false otherwise.
         */
        public function __call($name, $arguments)
        {
        }
        /**
         * Sets up capability object properties.
         *
         * Will set the value for the 'cap_key' property to current database table
         * prefix, followed by 'capabilities'. Will then check to see if the
         * property matching the 'cap_key' exists and is an array. If so, it will be
         * used.
         *
         * @since 2.1.0
         * @deprecated 4.9.0 Use WP_User::for_site()
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param string $cap_key Optional capability key
         */
        protected function _init_caps($cap_key = '')
        {
        }
        /**
         * Retrieves all of the capabilities of the user's roles, and merges them with
         * individual user capabilities.
         *
         * All of the capabilities of the user's roles are merged with the user's individual
         * capabilities. This means that the user can be denied specific capabilities that
         * their role might have, but the user is specifically denied.
         *
         * @since 2.0.0
         *
         * @return bool[] Array of key/value pairs where keys represent a capability name
         *                and boolean values represent whether the user has that capability.
         */
        public function get_role_caps()
        {
        }
        /**
         * Adds role to user.
         *
         * Updates the user's meta data option with capabilities and roles.
         *
         * @since 2.0.0
         *
         * @param string $role Role name.
         * @phpstan-return void
         */
        public function add_role($role)
        {
        }
        /**
         * Removes role from user.
         *
         * @since 2.0.0
         *
         * @param string $role Role name.
         * @phpstan-return void
         */
        public function remove_role($role)
        {
        }
        /**
         * Sets the role of the user.
         *
         * This will remove the previous roles of the user and assign the user the
         * new one. You can set the role to an empty string and it will remove all
         * of the roles from the user.
         *
         * @since 2.0.0
         *
         * @param string $role Role name.
         * @phpstan-return void
         */
        public function set_role($role)
        {
        }
        /**
         * Chooses the maximum level the user has.
         *
         * Will compare the level from the $item parameter against the $max
         * parameter. If the item is incorrect, then just the $max parameter value
         * will be returned.
         *
         * Used to get the max level based on the capabilities the user has. This
         * is also based on roles, so if the user is assigned the Administrator role
         * then the capability 'level_10' will exist and the user will get that
         * value.
         *
         * @since 2.0.0
         *
         * @param int    $max  Max level of user.
         * @param string $item Level capability name.
         * @return int Max Level.
         */
        public function level_reduction($max, $item)
        {
        }
        /**
         * Updates the maximum user level for the user.
         *
         * Updates the 'user_level' user metadata (includes prefix that is the
         * database table prefix) with the maximum user level. Gets the value from
         * the all of the capabilities that the user has.
         *
         * @since 2.0.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         */
        public function update_user_level_from_caps()
        {
        }
        /**
         * Adds capability and grant or deny access to capability.
         *
         * @since 2.0.0
         *
         * @param string $cap   Capability name.
         * @param bool   $grant Whether to grant capability to user.
         */
        public function add_cap($cap, $grant = \true)
        {
        }
        /**
         * Removes capability from user.
         *
         * @since 2.0.0
         *
         * @param string $cap Capability name.
         * @phpstan-return void
         */
        public function remove_cap($cap)
        {
        }
        /**
         * Removes all of the capabilities of the user.
         *
         * @since 2.1.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         */
        public function remove_all_caps()
        {
        }
        /**
         * Returns whether the user has the specified capability.
         *
         * This function also accepts an ID of an object to check against if the capability is a meta capability. Meta
         * capabilities such as `edit_post` and `edit_user` are capabilities used by the `map_meta_cap()` function to
         * map to primitive capabilities that a user or role has, such as `edit_posts` and `edit_others_posts`.
         *
         * Example usage:
         *
         *     $user->has_cap( 'edit_posts' );
         *     $user->has_cap( 'edit_post', $post->ID );
         *     $user->has_cap( 'edit_post_meta', $post->ID, $meta_key );
         *
         * While checking against a role in place of a capability is supported in part, this practice is discouraged as it
         * may produce unreliable results.
         *
         * @since 2.0.0
         * @since 5.3.0 Formalized the existing and already documented `...$args` parameter
         *              by adding it to the function signature.
         *
         * @see map_meta_cap()
         *
         * @param string $cap     Capability name.
         * @param mixed  ...$args Optional further parameters, typically starting with an object ID.
         * @return bool Whether the user has the given capability, or, if an object ID is passed, whether the user has
         *              the given capability for that object.
         */
        public function has_cap($cap, ...$args)
        {
        }
        /**
         * Converts numeric level to level capability name.
         *
         * Prepends 'level_' to level number.
         *
         * @since 2.0.0
         *
         * @param int $level Level number, 1 to 10.
         * @return string
         */
        public function translate_level_to_cap($level)
        {
        }
        /**
         * Sets the site to operate on. Defaults to the current site.
         *
         * @since 3.0.0
         * @deprecated 4.9.0 Use WP_User::for_site()
         *
         * @param int $blog_id Optional. Site ID, defaults to current site.
         */
        public function for_blog($blog_id = 0)
        {
        }
        /**
         * Sets the site to operate on. Defaults to the current site.
         *
         * @since 4.9.0
         *
         * @global wpdb $wpdb WordPress database abstraction object.
         *
         * @param int $site_id Site ID to initialize user capabilities for. Default is the current site.
         */
        public function for_site($site_id = 0)
        {
        }
        /**
         * Gets the ID of the site for which the user's capabilities are currently initialized.
         *
         * @since 4.9.0
         *
         * @return int Site ID.
         */
        public function get_site_id()
        {
        }
    }
    /**
     * Singleton that registers and instantiates WP_Widget classes.
     *
     * @since 2.8.0
     * @since 4.4.0 Moved to its own file from wp-includes/widgets.php
     */
    #[\AllowDynamicProperties]
    class WP_Widget_Factory
    {
        /**
         * Widgets array.
         *
         * Keyed by class name for a widget registered by name, and by a prefixed object ID for a widget
         * registered as an instance.
         *
         * @since 2.8.0
         * @var array<string, WP_Widget>
         * @phpstan-var array<non-decimal-int-string, WP_Widget>
         */
        public $widgets = array();
        /**
         * PHP5 constructor.
         *
         * @since 4.3.0
         */
        public function __construct()
        {
        }
        /**
         * PHP4 constructor.
         *
         * @since 2.8.0
         * @deprecated 4.3.0 Use __construct() instead.
         *
         * @see WP_Widget_Factory::__construct()
         */
        public function WP_Widget_Factory()
        {
        }
        /**
         * Registers a widget subclass.
         *
         * @since 2.8.0
         * @since 4.6.0 Updated the `$widget` parameter to also accept a WP_Widget instance object
         *              instead of simply a `WP_Widget` subclass name.
         * @since 7.1.1 The key for an instance is prefixed so that it is never cast to an integer.
         *
         * @param string|WP_Widget $widget Either the name of a `WP_Widget` subclass or an instance of a `WP_Widget` subclass.
         * @phpstan-param class-string<\WP_Widget>|\WP_Widget $widget
         */
        public function register($widget)
        {
        }
        /**
         * Un-registers a widget subclass.
         *
         * @since 2.8.0
         * @since 4.6.0 Updated the `$widget` parameter to also accept a WP_Widget instance object
         *              instead of simply a `WP_Widget` subclass name.
         * @since 7.1.1 The key for an instance is prefixed so that it is never cast to an integer.
         *
         * @param string|WP_Widget $widget Either the name of a `WP_Widget` subclass or an instance of a `WP_Widget` subclass.
         * @phpstan-param class-string<\WP_Widget>|\WP_Widget $widget
         */
        public function unregister($widget)
        {
        }
        /**
         * Serves as a utility method for adding widgets to the registered widgets global.
         *
         * @since 2.8.0
         *
         * @global array $wp_registered_widgets
         */
        public function _register_widgets()
        {
        }
        /**
         * Returns the registered WP_Widget object for the given widget type.
         *
         * @since 5.8.0
         *
         * @param string $id_base Widget type ID.
         * @return WP_Widget|null
         */
        public function get_widget_object($id_base)
        {
        }
        /**
         * Returns the registered key for the given widget type.
         *
         * @since 5.8.0
         *
         * @param string $id_base Widget type ID.
         * @return string
         */
        public function get_widget_key($id_base)
        {
        }
    }
    /**
     * Core base class extended to register widgets.
     *
     * This class must be extended for each widget, and WP_Widget::widget() must be overridden.
     *
     * If adding widget options, WP_Widget::update() and WP_Widget::form() should also be overridden.
     *
     * @since 2.8.0
     * @since 4.4.0 Moved to its own file from wp-includes/widgets.php
     * @phpstan-template T of array = array<string, mixed>
     */
    #[\AllowDynamicProperties]
    class WP_Widget
    {
        /**
         * Root ID for all widgets of this type.
         *
         * @since 2.8.0
         * @var mixed|string
         */
        public $id_base;
        /**
         * Name for this widget type.
         *
         * @since 2.8.0
         * @var string
         */
        public $name;
        /**
         * Option name for this widget type.
         *
         * @since 2.8.0
         * @var string
         */
        public $option_name;
        /**
         * Alt option name for this widget type.
         *
         * @since 2.8.0
         * @var string
         */
        public $alt_option_name;
        /**
         * Option array passed to wp_register_sidebar_widget().
         *
         * @since 2.8.0
         * @var array
         */
        public $widget_options;
        /**
         * Option array passed to wp_register_widget_control().
         *
         * @since 2.8.0
         * @var array
         */
        public $control_options;
        /**
         * Unique ID number of the current instance.
         *
         * @since 2.8.0
         * @var bool|int
         */
        public $number = \false;
        /**
         * Unique ID string of the current instance (id_base-number).
         *
         * @since 2.8.0
         * @var bool|string
         */
        public $id = \false;
        /**
         * Whether the widget data has been updated.
         *
         * Set to true when the data is updated after a POST submit - ensures it does
         * not happen twice.
         *
         * @since 2.8.0
         * @var bool
         */
        public $updated = \false;
        /**
         * Echoes the widget content.
         *
         * Subclasses should override this function to generate their widget code.
         *
         * @since 2.8.0
         *
         * @param array $args     Display arguments including 'before_title', 'after_title',
         *                        'before_widget', and 'after_widget'.
         * @param array $instance The settings for the particular instance of the widget.
         * @phpstan-param T $instance
         * @phpstan-param array{name:string,id:string,description:string,class:string,before_widget:string,after_widget:string,before_title:string,after_title:string,before_sidebar:string,after_sidebar:string,show_in_rest:boolean,widget_id:string,widget_name:string} $args
         */
        public function widget($args, $instance)
        {
        }
        /**
         * Updates a particular instance of a widget.
         *
         * This function should check that `$new_instance` is set correctly. The newly-calculated
         * value of `$instance` should be returned. If false is returned, the instance won't be
         * saved/updated.
         *
         * @since 2.8.0
         *
         * @param array $new_instance New settings for this instance as input by the user via
         *                            WP_Widget::form().
         * @param array $old_instance Old settings for this instance.
         * @return array Settings to save or bool false to cancel saving.
         * @phpstan-param T $new_instance
         * @phpstan-param T $old_instance
         */
        public function update($new_instance, $old_instance)
        {
        }
        /**
         * Outputs the settings update form.
         *
         * @since 2.8.0
         *
         * @param array $instance The settings for the particular instance of the widget.
         * @return string|null Default return is 'noform'. A subclass may opt to return null.
         * @phpstan-param T $instance
         */
        public function form($instance)
        {
        }
        /**
         * PHP5 constructor.
         *
         * @since 2.8.0
         *
         * @param string $id_base         Base ID for the widget, lowercase and unique. If left empty,
         *                                a portion of the widget's PHP class name will be used. Has to be unique.
         * @param string $name            Name for the widget displayed on the configuration page.
         * @param array  $widget_options  Optional. Widget options. See wp_register_sidebar_widget() for
         *                                information on accepted arguments. Default empty array.
         * @param array  $control_options Optional. Widget control options. See wp_register_widget_control() for
         *                                information on accepted arguments. Default empty array.
         * @phpstan-param array{
         *   classname?: string,
         *   description?: string,
         *   show_instance_in_rest?: bool,
         * } $widget_options See wp_register_sidebar_widget()
         * @phpstan-param array{
         *   height?: int,
         *   width?: int,
         *   id_base?: int|string,
         * } $control_options See wp_register_widget_control()
         */
        public function __construct($id_base, $name, $widget_options = array(), $control_options = array())
        {
        }
        /**
         * PHP4 constructor.
         *
         * @since 2.8.0
         * @deprecated 4.3.0 Use __construct() instead.
         *
         * @see WP_Widget::__construct()
         *
         * @param string $id_base         Base ID for the widget, lowercase and unique. If left empty,
         *                                a portion of the widget's PHP class name will be used. Has to be unique.
         * @param string $name            Name for the widget displayed on the configuration page.
         * @param array  $widget_options  Optional. Widget options. See wp_register_sidebar_widget() for
         *                                information on accepted arguments. Default empty array.
         * @param array  $control_options Optional. Widget control options. See wp_register_widget_control() for
         *                                information on accepted arguments. Default empty array.
         * @phpstan-param array{
         *   classname?: string,
         *   description?: string,
         *   show_instance_in_rest?: bool,
         * } $widget_options See wp_register_sidebar_widget()
         * @phpstan-param array{
         *   height?: int,
         *   width?: int,
         *   id_base?: int|string,
         * } $control_options See wp_register_widget_control()
         */
        public function WP_Widget($id_base, $name, $widget_options = array(), $control_options = array())
        {
        }
        /**
         * Constructs name attributes for use in form() fields
         *
         * This function should be used in form() methods to create name attributes for fields
         * to be saved by update()
         *
         * @since 2.8.0
         * @since 4.4.0 Array format field names are now accepted.
         *
         * @param string $field_name Field name.
         * @return string Name attribute for `$field_name`.
         * @phpstan-return non-falsy-string
         */
        public function get_field_name($field_name)
        {
        }
        /**
         * Constructs id attributes for use in WP_Widget::form() fields.
         *
         * This function should be used in form() methods to create id attributes
         * for fields to be saved by WP_Widget::update().
         *
         * @since 2.8.0
         * @since 4.4.0 Array format field IDs are now accepted.
         *
         * @param string $field_name Field name.
         * @return string ID attribute for `$field_name`.
         * @phpstan-return non-falsy-string
         */
        public function get_field_id($field_name)
        {
        }
        /**
         * Register all widget instances of this widget class.
         *
         * @since 2.8.0
         */
        public function _register()
        {
        }
        /**
         * Sets the internal order number for the widget instance.
         *
         * @since 2.8.0
         *
         * @param int $number The unique order number of this widget instance compared to other
         *                    instances of the same class.
         */
        public function _set($number)
        {
        }
        /**
         * Retrieves the widget display callback.
         *
         * @since 2.8.0
         *
         * @return callable Display callback.
         */
        public function _get_display_callback()
        {
        }
        /**
         * Retrieves the widget update callback.
         *
         * @since 2.8.0
         *
         * @return callable Update callback.
         */
        public function _get_update_callback()
        {
        }
        /**
         * Retrieves the form callback.
         *
         * @since 2.8.0
         *
         * @return callable Form callback.
         */
        public function _get_form_callback()
        {
        }
        /**
         * Determines whether the current request is inside the Customizer preview.
         *
         * If true -- the current request is inside the Customizer preview, then
         * the object cache gets suspended and widgets should check this to decide
         * whether they should store anything persistently to the object cache,
         * to transients, or anywhere else.
         *
         * @since 3.9.0
         *
         * @global WP_Customize_Manager $wp_customize
         *
         * @return bool True if within the Customizer preview, false if not.
         */
        public function is_preview()
        {
        }
        /**
         * Generates the actual widget content (Do NOT override).
         *
         * Finds the instance and calls WP_Widget::widget().
         *
         * @since 2.8.0
         *
         * @param array     $args        Display arguments. See WP_Widget::widget() for information
         *                               on accepted arguments.
         * @param int|array $widget_args {
         *     Optional. Internal order number of the widget instance, or array of multi-widget arguments.
         *     Default 1.
         *
         *     @type int $number Number increment used for multiples of the same widget.
         * }
         * @phpstan-param int|array{
         *   number?: int,
         * } $widget_args
         * @final
         * @phpstan-return void
         */
        public function display_callback($args, $widget_args = 1)
        {
        }
        /**
         * Handles changed settings (Do NOT override).
         *
         * @since 2.8.0
         *
         * @global array $wp_registered_widgets
         *
         * @param int $deprecated Not used.
         * @final
         * @phpstan-param 1 $deprecated
         * @phpstan-return void
         */
        public function update_callback($deprecated = 1)
        {
        }
        /**
         * Generates the widget control form (Do NOT override).
         *
         * @since 2.8.0
         *
         * @param int|array $widget_args {
         *     Optional. Internal order number of the widget instance, or array of multi-widget arguments.
         *     Default 1.
         *
         *     @type int $number Number increment used for multiples of the same widget.
         * }
         * @return string|null
         * @phpstan-param int|array{
         *   number?: int,
         * } $widget_args
         * @final
         */
        public function form_callback($widget_args = 1)
        {
        }
        /**
         * Registers an instance of the widget class.
         *
         * @since 2.8.0
         *
         * @param int $number Optional. The unique order number of this widget instance
         *                    compared to other instances of the same class. Default -1.
         */
        public function _register_one($number = -1)
        {
        }
        /**
         * Saves the settings for all instances of the widget class.
         *
         * @since 2.8.0
         *
         * @param array $settings Multi-dimensional array of widget instance settings.
         */
        public function save_settings($settings)
        {
        }
        /**
         * Retrieves the settings for all instances of the widget class.
         *
         * @since 2.8.0
         *
         * @return array Multi-dimensional array of widget instance settings.
         */
        public function get_settings()
        {
        }
    }
    /**
     * WordPress environment setup class.
     *
     * @package WordPress
     * @since 2.0.0
     */
    #[\AllowDynamicProperties]
    class WP
    {
        /**
         * Public query variables.
         *
         * Long list of public query variables.
         *
         * @since 2.0.0
         * @var string[]
         */
        public $public_query_vars = array('m', 'p', 'posts', 'w', 'cat', 'withcomments', 'withoutcomments', 's', 'search', 'exact', 'sentence', 'calendar', 'page', 'paged', 'more', 'tb', 'pb', 'author', 'order', 'orderby', 'year', 'monthnum', 'day', 'hour', 'minute', 'second', 'name', 'category_name', 'tag', 'feed', 'author_name', 'pagename', 'page_id', 'error', 'attachment', 'attachment_id', 'subpost', 'subpost_id', 'preview', 'robots', 'favicon', 'taxonomy', 'term', 'cpage', 'post_type', 'embed');
        /**
         * Private query variables.
         *
         * Long list of private query variables.
         *
         * @since 2.0.0
         * @var string[]
         */
        public $private_query_vars = array('offset', 'posts_per_page', 'posts_per_archive_page', 'showposts', 'nopaging', 'post_type', 'post_status', 'category__in', 'category__not_in', 'category__and', 'tag__in', 'tag__not_in', 'tag__and', 'tag_slug__in', 'tag_slug__and', 'tag_id', 'post_mime_type', 'perm', 'comments_per_page', 'post__in', 'post__not_in', 'post_parent', 'post_parent__in', 'post_parent__not_in', 'title', 'fields');
        /**
         * Extra query variables set by the user.
         *
         * @since 2.1.0
         * @var array
         */
        public $extra_query_vars = array();
        /**
         * Query variables for setting up the WordPress Query Loop.
         *
         * @since 2.0.0
         * @var array
         */
        public $query_vars = array();
        /**
         * String parsed to set the query variables.
         *
         * @since 2.0.0
         * @var string
         */
        public $query_string = '';
        /**
         * The request path, e.g. 2015/05/06.
         *
         * @since 2.0.0
         * @var string
         */
        public $request = '';
        /**
         * Rewrite rule the request matched.
         *
         * @since 2.0.0
         * @var string
         */
        public $matched_rule = '';
        /**
         * Rewrite query the request matched.
         *
         * @since 2.0.0
         * @var string
         */
        public $matched_query = '';
        /**
         * Whether already did the permalink.
         *
         * @since 2.0.0
         * @var bool
         */
        public $did_permalink = \false;
        /**
         * Adds a query variable to the list of public query variables.
         *
         * @since 2.1.0
         *
         * @param string $qv Query variable name.
         */
        public function add_query_var($qv)
        {
        }
        /**
         * Removes a query variable from a list of public query variables.
         *
         * @since 4.5.0
         *
         * @param string $name Query variable name.
         */
        public function remove_query_var($name)
        {
        }
        /**
         * Sets the value of a query variable.
         *
         * @since 2.3.0
         *
         * @param string $key   Query variable name.
         * @param mixed  $value Query variable value.
         */
        public function set_query_var($key, $value)
        {
        }
        /**
         * Parses the request to find the correct WordPress query.
         *
         * Sets up the query variables based on the request. There are also many
         * filters and actions that can be used to further manipulate the result.
         *
         * @since 2.0.0
         * @since 6.0.0 A return value was added.
         *
         * @global WP_Rewrite $wp_rewrite WordPress rewrite component.
         *
         * @param array|string $extra_query_vars Set the extra query variables.
         * @return bool Whether the request was parsed.
         */
        public function parse_request($extra_query_vars = '')
        {
        }
        /**
         * Sends additional HTTP headers for caching, content type, etc.
         *
         * Sets the Content-Type header. Sets the 'error' status (if passed) and optionally exits.
         * If showing a feed, it will also send Last-Modified, ETag, and 304 status if needed.
         *
         * @since 2.0.0
         * @since 4.4.0 `X-Pingback` header is added conditionally for single posts that allow pings.
         * @since 6.1.0 Runs after posts have been queried.
         *
         * @global WP_Query $wp_query WordPress Query object.
         */
        public function send_headers()
        {
        }
        /**
         * Sets the query string property based off of the query variable property.
         *
         * The {@see 'query_string'} filter is deprecated, but still works. Plugins should
         * use the {@see 'request'} filter instead.
         *
         * @since 2.0.0
         */
        public function build_query_string()
        {
        }
        /**
         * Set up the WordPress Globals.
         *
         * The query_vars property will be extracted to the GLOBALS. So care should
         * be taken when naming global variables that might interfere with the
         * WordPress environment.
         *
         * @since 2.0.0
         *
         * @global WP_Query     $wp_query     WordPress Query object.
         * @global string       $query_string Query string for the loop.
         * @global array        $posts        The found posts.
         * @global WP_Post|null $post         The current post, if available.
         * @global string       $request      The SQL statement for the request.
         * @global int          $more         Only set, if single page or post.
         * @global int          $single       If single page or post. Only set, if single page or post.
         * @global WP_User      $authordata   Only set, if author archive.
         */
        public function register_globals()
        {
        }
        /**
         * Set up the current user.
         *
         * @since 2.0.0
         */
        public function init()
        {
        }
        /**
         * Set up the Loop based on the query variables.
         *
         * @since 2.0.0
         *
         * @global WP_Query $wp_the_query WordPress Query object.
         */
        public function query_posts()
        {
        }
        /**
         * Set the Headers for 404, if nothing is found for requested URL.
         *
         * Issue a 404 if a request doesn't match any posts and doesn't match any object
         * (e.g. an existing-but-empty category, tag, author) and a 404 was not already issued,
         * and if the request was not a search or the homepage.
         *
         * Otherwise, issue a 200.
         *
         * This sets headers after posts have been queried. handle_404() really means "handle status".
         * By inspecting the result of querying posts, seemingly successful requests can be switched to
         * a 404 so that canonical redirection logic can kick in.
         *
         * @since 2.0.0
         *
         * @global WP_Query $wp_query WordPress Query object.
         * @phpstan-return void
         */
        public function handle_404()
        {
        }
        /**
         * Sets up all of the variables required by the WordPress environment.
         *
         * The action {@see 'wp'} has one parameter that references the WP object. It
         * allows for accessing the properties and methods to further manipulate the
         * object.
         *
         * @since 2.0.0
         *
         * @param string|array $query_args Passed to parse_request().
         */
        public function main($query_args = '')
        {
        }
    }
    /**
     * WordPress database access abstraction class.
     *
     * This class is used to interact with a database without needing to use raw SQL statements.
     * By default, WordPress uses this class to instantiate the global $wpdb object, providing
     * access to the WordPress database.
     *
     * It is possible to replace the global instance with your own by setting the $wpdb global variable
     * in wp-content/db.php file to your class. The wpdb class will still be included, so you can
     * extend it or simply use your own.
     *
     * @link https://developer.wordpress.org/reference/classes/wpdb/
     *
     * @since 0.71
     */
    #[\AllowDynamicProperties]
    class wpdb
    {
        /**
         * Whether to show SQL/DB errors.
         *
         * Default is to show errors if both WP_DEBUG and WP_DEBUG_DISPLAY evaluate to true.
         *
         * @since 0.71
         *
         * @var bool
         */
        public $show_errors = \false;
        /**
         * Whether to suppress errors during the DB bootstrapping. Default false.
         *
         * @since 2.5.0
         *
         * @var bool
         */
        public $suppress_errors = \false;
        /**
         * The error encountered during the last query.
         *
         * @since 2.5.0
         *
         * @var string
         */
        public $last_error = '';
        /**
         * The number of queries made.
         *
         * @since 1.2.0
         *
         * @var int
         */
        public $num_queries = 0;
        /**
         * Count of rows returned by the last query.
         *
         * @since 0.71
         *
         * @var int
         */
        public $num_rows = 0;
        /**
         * Count of rows affected by the last query.
         *
         * @since 0.71
         *
         * @var int
         */
        public $rows_affected = 0;
        /**
         * The ID generated for an AUTO_INCREMENT column by the last query (usually INSERT).
         *
         * @since 0.71
         *
         * @var int
         */
        public $insert_id = 0;
        /**
         * The last query made.
         *
         * @since 0.71
         *
         * @var string
         */
        public $last_query;
        /**
         * Results of the last query.
         *
         * @since 0.71
         *
         * @var stdClass[]|null
         */
        public $last_result;
        /**
         * Database query result.
         *
         * Possible values:
         *
         * - `mysqli_result` instance for successful SELECT, SHOW, DESCRIBE, or EXPLAIN queries
         * - `true` for other query types that were successful
         * - `null` if a query is yet to be made or if the result has since been flushed
         * - `false` if the query returned an error
         *
         * @since 0.71
         *
         * @var mysqli_result|bool|null
         */
        protected $result;
        /**
         * Cached column info, for confidence checking data before inserting.
         *
         * @since 4.2.0
         *
         * @var array
         */
        protected $col_meta = array();
        /**
         * Calculated character sets keyed by table name.
         *
         * @since 4.2.0
         *
         * @var string[]
         */
        protected $table_charset = array();
        /**
         * Whether text fields in the current query need to be confidence checked.
         *
         * @since 4.2.0
         *
         * @var bool
         */
        protected $check_current_query = \true;
        /**
         * Saved info on the table column.
         *
         * @since 0.71
         *
         * @var array
         */
        protected $col_info;
        /**
         * Log of queries that were executed, for debugging purposes.
         *
         * @since 1.5.0
         * @since 2.5.0 The third element in each query log was added to record the calling functions.
         * @since 5.1.0 The fourth element in each query log was added to record the start time.
         * @since 5.3.0 The fifth element in each query log was added to record custom data.
         *
         * @var array[] {
         *     Array of arrays containing information about queries that were executed.
         *
         *     @type array ...$0 {
         *         Data for each query.
         *
         *         @type string $0 The query's SQL.
         *         @type float  $1 Total time spent on the query, in seconds.
         *         @type string $2 Comma-separated list of the calling functions.
         *         @type float  $3 Unix timestamp of the time at the start of the query.
         *         @type array  $4 Custom query data.
         *     }
         * }
         * @phpstan-var array<int|string, array{
         *   0: string,
         *   1: float,
         *   2: string,
         *   3: float,
         *   4: array,
         * }>
         */
        public $queries;
        /**
         * The number of times to retry reconnecting before dying. Default 5.
         *
         * @since 3.9.0
         *
         * @see wpdb::check_connection()
         * @var int
         */
        protected $reconnect_retries = 5;
        /**
         * WordPress table prefix.
         *
         * You can set this to have multiple WordPress installations in a single database.
         *
         * @since 2.5.0
         *
         * @var string
         */
        public $prefix = '';
        /**
         * WordPress base table prefix.
         *
         * @since 3.0.0
         *
         * @var string
         */
        public $base_prefix;
        /**
         * Whether the database queries are ready to start executing.
         *
         * @since 2.3.2
         *
         * @var bool
         */
        public $ready = \false;
        /**
         * Blog ID.
         *
         * @since 3.0.0
         *
         * @var int
         */
        public $blogid = 0;
        /**
         * Site ID.
         *
         * @since 3.0.0
         *
         * @var int
         */
        public $siteid = 0;
        /**
         * List of WordPress per-site tables.
         *
         * @since 2.5.0
         *
         * @see wpdb::tables()
         * @var string[]
         */
        public $tables = array('posts', 'comments', 'links', 'options', 'postmeta', 'terms', 'term_taxonomy', 'term_relationships', 'termmeta', 'commentmeta');
        /**
         * List of deprecated WordPress tables.
         *
         * 'categories', 'post2cat', and 'link2cat' were deprecated in 2.3.0, db version 5539.
         *
         * @since 2.9.0
         *
         * @see wpdb::tables()
         * @var string[]
         */
        public $old_tables = array('categories', 'post2cat', 'link2cat');
        /**
         * List of WordPress global tables.
         *
         * @since 3.0.0
         *
         * @see wpdb::tables()
         * @var string[]
         */
        public $global_tables = array('users', 'usermeta');
        /**
         * List of Multisite global tables.
         *
         * @since 3.0.0
         *
         * @see wpdb::tables()
         * @var string[]
         */
        public $ms_global_tables = array('blogs', 'blogmeta', 'signups', 'site', 'sitemeta', 'registration_log');
        /**
         * List of deprecated WordPress Multisite global tables.
         *
         * @since 6.1.0
         *
         * @see wpdb::tables()
         * @var string[]
         */
        public $old_ms_global_tables = array('sitecategories');
        /**
         * WordPress Comments table.
         *
         * @since 1.5.0
         *
         * @var string
         */
        public $comments;
        /**
         * WordPress Comment Metadata table.
         *
         * @since 2.9.0
         *
         * @var string
         */
        public $commentmeta;
        /**
         * WordPress Links table.
         *
         * @since 1.5.0
         *
         * @var string
         */
        public $links;
        /**
         * WordPress Options table.
         *
         * @since 1.5.0
         *
         * @var string
         */
        public $options;
        /**
         * WordPress Post Metadata table.
         *
         * @since 1.5.0
         *
         * @var string
         */
        public $postmeta;
        /**
         * WordPress Posts table.
         *
         * @since 1.5.0
         *
         * @var string
         */
        public $posts;
        /**
         * WordPress Terms table.
         *
         * @since 2.3.0
         *
         * @var string
         */
        public $terms;
        /**
         * WordPress Term Relationships table.
         *
         * @since 2.3.0
         *
         * @var string
         */
        public $term_relationships;
        /**
         * WordPress Term Taxonomy table.
         *
         * @since 2.3.0
         *
         * @var string
         */
        public $term_taxonomy;
        /**
         * WordPress Term Meta table.
         *
         * @since 4.4.0
         *
         * @var string
         */
        public $termmeta;
        /**
         * WordPress User Metadata table.
         *
         * @since 2.3.0
         *
         * @var string
         */
        public $usermeta;
        /**
         * WordPress Users table.
         *
         * @since 1.5.0
         *
         * @var string
         */
        public $users;
        /**
         * Multisite Blogs table.
         *
         * @since 3.0.0
         *
         * @var string|null
         */
        public $blogs;
        /**
         * Multisite Blog Metadata table.
         *
         * @since 5.1.0
         *
         * @var string|null
         */
        public $blogmeta;
        /**
         * Multisite Registration Log table.
         *
         * @since 3.0.0
         *
         * @var string|null
         */
        public $registration_log;
        /**
         * Multisite Signups table.
         *
         * @since 3.0.0
         *
         * @var string|null
         */
        public $signups;
        /**
         * Multisite Sites table.
         *
         * @since 3.0.0
         *
         * @var string|null
         */
        public $site;
        /**
         * Multisite Sitewide Terms table.
         *
         * @since 3.0.0
         *
         * @var string|null
         */
        public $sitecategories;
        /**
         * Multisite Site Metadata table.
         *
         * @since 3.0.0
         *
         * @var string|null
         */
        public $sitemeta;
        /**
         * Format specifiers for DB columns.
         *
         * Columns not listed here default to %s. Initialized during WP load.
         * Keys are column names, values are format types: 'ID' => '%d'.
         *
         * @since 2.8.0
         *
         * @see wpdb::prepare()
         * @see wpdb::insert()
         * @see wpdb::update()
         * @see wpdb::delete()
         * @see wp_set_wpdb_vars()
         * @var array
         */
        public $field_types = array();
        /**
         * Database table columns charset.
         *
         * @since 2.2.0
         *
         * @var string
         */
        public $charset;
        /**
         * Database table columns collate.
         *
         * @since 2.2.0
         *
         * @var string
         */
        public $collate;
        /**
         * Database Username.
         *
         * @since 2.9.0
         *
         * @var string
         */
        protected $dbuser;
        /**
         * Database Password.
         *
         * @since 3.1.0
         *
         * @var string
         */
        protected $dbpassword;
        /**
         * Database Name.
         *
         * @since 3.1.0
         *
         * @var string
         */
        protected $dbname;
        /**
         * Database Host.
         *
         * @since 3.1.0
         *
         * @var string
         */
        protected $dbhost;
        /**
         * Database handle.
         *
         * Possible values:
         *
         * - `mysqli` instance during normal operation
         * - `null` if the connection is yet to be made or has been closed
         * - `false` if the connection has failed
         *
         * @since 0.71
         *
         * @var mysqli|false|null
         */
        protected $dbh;
        /**
         * A textual description of the last query/get_row/get_var call.
         *
         * @since 3.0.0
         *
         * @var string
         */
        public $func_call;
        /**
         * Whether MySQL is used as the database engine.
         *
         * Set in wpdb::db_connect() to true, by default. This is used when checking
         * against the required MySQL version for WordPress. Normally, a replacement
         * database drop-in (db.php) will skip these checks, but setting this to true
         * will force the checks to occur.
         *
         * @since 3.3.0
         *
         * @var bool
         */
        public $is_mysql = \null;
        /**
         * A list of incompatible SQL modes.
         *
         * @since 3.9.0
         *
         * @var string[]
         */
        protected $incompatible_modes = array('NO_ZERO_DATE', 'ONLY_FULL_GROUP_BY', 'STRICT_TRANS_TABLES', 'STRICT_ALL_TABLES', 'TRADITIONAL', 'ANSI');
        /**
         * Time when the last query was performed.
         *
         * Only set when `SAVEQUERIES` is defined and truthy.
         *
         * @since 1.5.0
         *
         * @var float
         */
        public $time_start = \null;
        /**
         * The last SQL error that was encountered.
         *
         * @since 2.5.0
         *
         * @var WP_Error|string
         */
        public $error = \null;
        /**
         * Connects to the database server and selects a database.
         *
         * Does the actual setting up
         * of the class properties and connection to the database.
         *
         * @since 2.0.8
         *
         * @link https://core.trac.wordpress.org/ticket/3354
         *
         * @param string $dbuser     Database user.
         * @param string $dbpassword Database password.
         * @param string $dbname     Database name.
         * @param string $dbhost     Database host.
         * @phpstan-return void
         */
        public function __construct(
            $dbuser,
            #[\SensitiveParameter]
            $dbpassword,
            $dbname,
            $dbhost
        )
        {
        }
        /**
         * Makes private properties readable for backward compatibility.
         *
         * @since 3.5.0
         *
         * @param string $name The private member to get, and optionally process.
         * @return mixed The private member.
         */
        public function __get($name)
        {
        }
        /**
         * Makes private properties settable for backward compatibility.
         *
         * @since 3.5.0
         *
         * @param string $name  The private member to set.
         * @param mixed  $value The value to set.
         * @phpstan-return void
         */
        public function __set($name, $value)
        {
        }
        /**
         * Makes private properties check-able for backward compatibility.
         *
         * @since 3.5.0
         *
         * @param string $name The private member to check.
         * @return bool If the member is set or not.
         */
        public function __isset($name)
        {
        }
        /**
         * Makes private properties un-settable for backward compatibility.
         *
         * @since 3.5.0
         *
         * @param string $name The private member to unset.
         */
        public function __unset($name)
        {
        }
        /**
         * Sets $this->charset and $this->collate.
         *
         * @since 3.1.0
         */
        public function init_charset()
        {
        }
        /**
         * Determines the best charset and collation to use given a charset and collation.
         *
         * For example, when able, utf8mb4 should be used instead of utf8.
         *
         * @since 4.6.0
         *
         * @param string $charset The character set to check.
         * @param string $collate The collation to check.
         * @return array {
         *     The most appropriate character set and collation to use.
         *
         *     @type string $charset Character set.
         *     @type string $collate Collation.
         * }
         * @phpstan-return array{
         *   charset: string,
         *   collate: string,
         * }
         */
        public function determine_charset($charset, $collate)
        {
        }
        /**
         * Sets the connection's character set.
         *
         * @since 3.1.0
         *
         * @param mysqli $dbh     The connection returned by `mysqli_connect()`.
         * @param string $charset Optional. The character set. Default null.
         * @param string $collate Optional. The collation. Default null.
         */
        public function set_charset($dbh, $charset = \null, $collate = \null)
        {
        }
        /**
         * Changes the current SQL mode, and ensures its WordPress compatibility.
         *
         * If no modes are passed, it will ensure the current SQL server modes are compatible.
         *
         * @since 3.9.0
         *
         * @param array $modes Optional. A list of SQL modes to set. Default empty array.
         * @phpstan-return void
         */
        public function set_sql_mode($modes = array())
        {
        }
        /**
         * Sets the table prefix for the WordPress tables.
         *
         * @since 2.5.0
         *
         * @param string $prefix          Alphanumeric name for the new prefix.
         * @param bool   $set_table_names Optional. Whether the table names, e.g. wpdb::$posts,
         *                                should be updated or not. Default true.
         * @return string|WP_Error Old prefix or WP_Error on error.
         */
        public function set_prefix($prefix, $set_table_names = \true)
        {
        }
        /**
         * Sets blog ID.
         *
         * @since 3.0.0
         *
         * @param int $blog_id
         * @param int $network_id Optional. Network ID. Default 0.
         * @return int Previous blog ID.
         */
        public function set_blog_id($blog_id, $network_id = 0)
        {
        }
        /**
         * Gets blog prefix.
         *
         * @since 3.0.0
         *
         * @param int $blog_id Optional. Blog ID to retrieve the table prefix for.
         *                     Defaults to the current blog ID.
         * @return string Blog prefix.
         */
        public function get_blog_prefix($blog_id = \null)
        {
        }
        /**
         * Returns an array of WordPress tables.
         *
         * Also allows for the `CUSTOM_USER_TABLE` and `CUSTOM_USER_META_TABLE` to override the WordPress users
         * and usermeta tables that would otherwise be determined by the prefix.
         *
         * The `$scope` argument can take one of the following:
         *
         * - 'all' - returns 'all' and 'global' tables. No old tables are returned.
         * - 'blog' - returns the blog-level tables for the queried blog.
         * - 'global' - returns the global tables for the installation, returning multisite tables only on multisite.
         * - 'ms_global' - returns the multisite global tables, regardless if current installation is multisite.
         * - 'old' - returns tables which are deprecated.
         *
         * @since 3.0.0
         * @since 6.1.0 `old` now includes deprecated multisite global tables only on multisite.
         *
         * @uses wpdb::$tables
         * @uses wpdb::$old_tables
         * @uses wpdb::$global_tables
         * @uses wpdb::$ms_global_tables
         * @uses wpdb::$old_ms_global_tables
         *
         * @param string $scope   Optional. Possible values include 'all', 'global', 'ms_global', 'blog',
         *                        or 'old' tables. Default 'all'.
         * @param bool   $prefix  Optional. Whether to include table prefixes. If blog prefix is requested,
         *                        then the custom users and usermeta tables will be mapped. Default true.
         * @param int    $blog_id Optional. The blog_id to prefix. Used only when prefix is requested.
         *                        Defaults to `wpdb::$blogid`.
         * @return string[] Table names. When a prefix is requested, the key is the unprefixed table name.
         */
        public function tables($scope = 'all', $prefix = \true, $blog_id = 0)
        {
        }
        /**
         * Selects a database using the current or provided database connection.
         *
         * The database name will be changed based on the current database connection.
         * On failure, the execution will bail and display a DB error.
         *
         * @since 0.71
         *
         * @param string $db  Database name.
         * @param mysqli $dbh Optional. Database connection.
         *                    Defaults to the current database handle.
         */
        public function select($db, $dbh = \null)
        {
        }
        /**
         * Do not use, deprecated.
         *
         * Use esc_sql() or wpdb::prepare() instead.
         *
         * @since 2.8.0
         * @deprecated 3.6.0 Use wpdb::prepare()
         * @see wpdb::prepare()
         * @see esc_sql()
         *
         * @param string $data
         * @return string
         */
        public function _weak_escape($data)
        {
        }
        /**
         * Real escape using mysqli_real_escape_string().
         *
         * @since 2.8.0
         *
         * @see mysqli_real_escape_string()
         *
         * @param string $data String to escape.
         * @return string Escaped string.
         */
        public function _real_escape($data)
        {
        }
        /**
         * Escapes data. Works on arrays.
         *
         * @since 2.8.0
         *
         * @uses wpdb::_real_escape()
         *
         * @param string|array $data Data to escape.
         * @return string|array Escaped data, in the same type as supplied.
         */
        public function _escape($data)
        {
        }
        /**
         * Do not use, deprecated.
         *
         * Use esc_sql() or wpdb::prepare() instead.
         *
         * @since 0.71
         * @deprecated 3.6.0 Use wpdb::prepare()
         * @see wpdb::prepare()
         * @see esc_sql()
         *
         * @param string|array $data Data to escape.
         * @return string|array Escaped data, in the same type as supplied.
         */
        public function escape($data)
        {
        }
        /**
         * Escapes content by reference for insertion into the database, for security.
         *
         * @uses wpdb::_real_escape()
         *
         * @since 2.3.0
         *
         * @param string $data String to escape.
         */
        public function escape_by_ref(&$data)
        {
        }
        /**
         * Quotes an identifier such as a table or field name.
         *
         * @since 6.2.0
         *
         * @param string $identifier Identifier to escape.
         * @return string Escaped identifier.
         */
        public function quote_identifier($identifier)
        {
        }
        /**
         * Prepares a SQL query for safe execution.
         *
         * Uses `sprintf()`-like syntax. The following placeholders can be used in the query string:
         *
         * - `%d` (integer)
         * - `%f` (float)
         * - `%s` (string)
         * - `%i` (identifier, e.g. table/field names)
         *
         * All placeholders MUST be left unquoted in the query string. A corresponding argument
         * MUST be passed for each placeholder.
         *
         * Note: There is one exception to the above: for compatibility with old behavior,
         * numbered or formatted string placeholders (eg, `%1$s`, `%5s`) will not have quotes
         * added by this function, so should be passed with appropriate quotes around them.
         *
         * Literal percentage signs (`%`) in the query string must be written as `%%`. Percentage wildcards
         * (for example, to use in LIKE syntax) must be passed via a substitution argument containing
         * the complete LIKE string, these cannot be inserted directly in the query string.
         * Also see wpdb::esc_like().
         *
         * Arguments may be passed as individual arguments to the method, or as a single array
         * containing all arguments. A combination of the two is not supported.
         *
         * Examples:
         *
         *     $wpdb->prepare(
         *         "SELECT * FROM `table` WHERE `column` = %s AND `field` = %d OR `other_field` LIKE %s",
         *         array( 'foo', 1337, '%bar' )
         *     );
         *
         *     $wpdb->prepare(
         *         "SELECT DATE_FORMAT(`field`, '%%c') FROM `table` WHERE `column` = %s",
         *         'foo'
         *     );
         *
         *     $wpdb->prepare(
         *         "SELECT * FROM %i WHERE %i = %s",
         *         $table,
         *         $field,
         *         $value
         *     );
         *
         * @since 2.3.0
         * @since 5.3.0 Formalized the existing and already documented `...$args` parameter
         *              by updating the function signature. The second parameter was changed
         *              from `$args` to `...$args`.
         * @since 6.2.0 Added `%i` for identifiers, e.g. table or field names.
         *              Check support via `wpdb::has_cap( 'identifier_placeholders' )`.
         *              This preserves compatibility with `sprintf()`, as the C version uses
         *              `%d` and `$i` as a signed integer, whereas PHP only supports `%d`.
         *
         * @link https://www.php.net/sprintf Description of syntax.
         *
         * @param string      $query   Query statement with `sprintf()`-like placeholders.
         * @param array|mixed $args    The array of variables to substitute into the query's placeholders
         *                             if being called with an array of arguments, or the first variable
         *                             to substitute into the query's placeholders if being called with
         *                             individual arguments.
         * @param mixed       ...$args Further variables to substitute into the query's placeholders
         *                             if being called with individual arguments.
         * @return string|null Sanitized query string, if there is a query to prepare.
         * @phpstan-param literal-string $query
         */
        public function prepare($query, ...$args)
        {
        }
        /**
         * First half of escaping for `LIKE` special characters `%` and `_` before preparing for SQL.
         *
         * Use this only before wpdb::prepare() or esc_sql(). Reversing the order is very bad for security.
         *
         * Example Prepared Statement:
         *
         *     $wild = '%';
         *     $find = 'only 43% of planets';
         *     $like = $wild . $wpdb->esc_like( $find ) . $wild;
         *     $sql  = $wpdb->prepare( "SELECT * FROM $wpdb->posts WHERE post_content LIKE %s", $like );
         *
         * Example Escape Chain:
         *
         *     $sql  = esc_sql( $wpdb->esc_like( $input ) );
         *
         * @since 4.0.0
         *
         * @param string $text The raw text to be escaped. The input typed by the user
         *                     should have no extra or deleted slashes.
         * @return string Text in the form of a LIKE phrase. The output is not SQL safe.
         *                Call wpdb::prepare() or wpdb::_real_escape() next.
         */
        public function esc_like($text)
        {
        }
        /**
         * Prints SQL/DB error.
         *
         * @since 0.71
         *
         * @global array $EZSQL_ERROR Stores error information of query and error string.
         *
         * @param string $str The error to display.
         * @return null|false Null if the showing of errors is enabled, false if disabled.
         */
        public function print_error($str = '')
        {
        }
        /**
         * Enables showing of database errors.
         *
         * This function should be used only to enable showing of errors.
         * wpdb::hide_errors() should be used instead for hiding errors.
         *
         * @since 0.71
         *
         * @see wpdb::hide_errors()
         *
         * @param bool $show Optional. Whether to show errors. Default true.
         * @return bool Whether showing of errors was previously active.
         */
        public function show_errors($show = \true)
        {
        }
        /**
         * Disables showing of database errors.
         *
         * By default database errors are not shown.
         *
         * @since 0.71
         *
         * @see wpdb::show_errors()
         *
         * @return bool Whether showing of errors was previously active.
         */
        public function hide_errors()
        {
        }
        /**
         * Enables or disables suppressing of database errors.
         *
         * By default database errors are suppressed.
         *
         * @since 2.5.0
         *
         * @see wpdb::hide_errors()
         *
         * @param bool $suppress Optional. Whether to suppress errors. Default true.
         * @return bool Whether suppressing of errors was previously active.
         */
        public function suppress_errors($suppress = \true)
        {
        }
        /**
         * Kills cached query results.
         *
         * @since 0.71
         * @phpstan-return void
         */
        public function flush()
        {
        }
        /**
         * Connects to and selects database.
         *
         * If `$allow_bail` is false, the lack of database connection will need to be handled manually.
         *
         * @since 3.0.0
         * @since 3.9.0 $allow_bail parameter added.
         *
         * @param bool $allow_bail Optional. Allows the function to bail. Default true.
         * @return bool True with a successful connection, false on failure.
         */
        public function db_connect($allow_bail = \true)
        {
        }
        /**
         * Parses the DB_HOST setting to interpret it for mysqli_real_connect().
         *
         * mysqli_real_connect() doesn't support the host param including a port or socket
         * like mysql_connect() does. This duplicates how mysql_connect() detects a port
         * and/or socket file.
         *
         * @since 4.9.0
         *
         * @param string $host The DB_HOST setting to parse.
         * @return array|false {
         *     Array containing the host, the port, the socket and
         *     whether it is an IPv6 address, in that order.
         *     False if the host couldn't be parsed.
         *
         *     @type string      $0 Host name.
         *     @type string|null $1 Port.
         *     @type string|null $2 Socket.
         *     @type bool        $3 Whether it is an IPv6 address.
         * }
         * @phpstan-return false|array{
         *   0: string,
         *   1: string|null,
         *   2: string|null,
         *   3: bool,
         * }
         */
        public function parse_db_host($host)
        {
        }
        /**
         * Checks that the connection to the database is still up. If not, try to reconnect.
         *
         * If this function is unable to reconnect, it will forcibly die, or if called
         * after the {@see 'template_redirect'} hook has been fired, return false instead.
         *
         * If `$allow_bail` is false, the lack of database connection will need to be handled manually.
         *
         * @since 3.9.0
         *
         * @param bool $allow_bail Optional. Allows the function to bail. Default true.
         * @return bool Whether the connection is up. Exits if down and $allow_bail is true.
         */
        public function check_connection($allow_bail = \true)
        {
        }
        /**
         * Performs a database query, using current database connection.
         *
         * More information can be found on the documentation page.
         *
         * @since 0.71
         *
         * @link https://developer.wordpress.org/reference/classes/wpdb/
         *
         * @param string $query Database query.
         * @return int|bool Boolean true for CREATE, ALTER, TRUNCATE and DROP queries. Number of rows
         *                  affected/selected for all other queries. Boolean false on error.
         */
        public function query($query)
        {
        }
        /**
         * Logs query data.
         *
         * @since 5.3.0
         *
         * @param string $query           The query's SQL.
         * @param float  $query_time      Total time spent on the query, in seconds.
         * @param string $query_callstack Comma-separated list of the calling functions.
         * @param float  $query_start     Unix timestamp of the time at the start of the query.
         * @param array  $query_data      Custom query data.
         */
        public function log_query($query, $query_time, $query_callstack, $query_start, $query_data)
        {
        }
        /**
         * Generates and returns a placeholder escape string for use in queries returned by ::prepare().
         *
         * @since 4.8.3
         *
         * @return string String to escape placeholders.
         */
        public function placeholder_escape()
        {
        }
        /**
         * Adds a placeholder escape string, to escape anything that resembles a printf() placeholder.
         *
         * @since 4.8.3
         *
         * @param string $query The query to escape.
         * @return string The query with the placeholder escape string inserted where necessary.
         */
        public function add_placeholder_escape($query)
        {
        }
        /**
         * Removes the placeholder escape strings from a query.
         *
         * @since 4.8.3
         *
         * @param string $query The query from which the placeholder will be removed.
         * @return string The query with the placeholder removed.
         */
        public function remove_placeholder_escape($query)
        {
        }
        /**
         * Inserts a row into the table.
         *
         * Examples:
         *
         *     $wpdb->insert(
         *         'table',
         *         array(
         *             'column1' => 'foo',
         *             'column2' => 'bar',
         *         )
         *     );
         *     $wpdb->insert(
         *         'table',
         *         array(
         *             'column1' => 'foo',
         *             'column2' => 1337,
         *         ),
         *         array(
         *             '%s',
         *             '%d',
         *         )
         *     );
         *
         * @since 2.5.0
         *
         * @see wpdb::prepare()
         * @see wpdb::$field_types
         * @see wp_set_wpdb_vars()
         *
         * @param string          $table  Table name.
         * @param array           $data   Data to insert (in column => value pairs).
         *                                Both `$data` columns and `$data` values should be "raw" (neither should be SQL escaped).
         *                                Sending a null value will cause the column to be set to NULL - the corresponding
         *                                format is ignored in this case.
         * @param string[]|string $format Optional. An array of formats to be mapped to each of the value in `$data`.
         *                                If string, that format will be used for all of the values in `$data`.
         *                                A format is one of '%d', '%f', '%s' (integer, float, string).
         *                                If omitted, all values in `$data` will be treated as strings unless otherwise
         *                                specified in wpdb::$field_types. Default null.
         * @return int|false The number of rows inserted, or false on error.
         */
        public function insert($table, $data, $format = \null)
        {
        }
        /**
         * Replaces a row in the table or inserts it if it does not exist, based on a PRIMARY KEY or a UNIQUE index.
         *
         * A REPLACE works exactly like an INSERT, except that if an old row in the table has the same value as a new row
         * for a PRIMARY KEY or a UNIQUE index, the old row is deleted before the new row is inserted.
         *
         * Examples:
         *
         *     $wpdb->replace(
         *         'table',
         *         array(
         *             'ID'      => 123,
         *             'column1' => 'foo',
         *             'column2' => 'bar',
         *         )
         *     );
         *     $wpdb->replace(
         *         'table',
         *         array(
         *             'ID'      => 456,
         *             'column1' => 'foo',
         *             'column2' => 1337,
         *         ),
         *         array(
         *             '%d',
         *             '%s',
         *             '%d',
         *         )
         *     );
         *
         * @since 3.0.0
         *
         * @see wpdb::prepare()
         * @see wpdb::$field_types
         * @see wp_set_wpdb_vars()
         *
         * @param string          $table  Table name.
         * @param array           $data   Data to insert (in column => value pairs).
         *                                Both `$data` columns and `$data` values should be "raw" (neither should be SQL escaped).
         *                                A primary key or unique index is required to perform a replace operation.
         *                                Sending a null value will cause the column to be set to NULL - the corresponding
         *                                format is ignored in this case.
         * @param string[]|string $format Optional. An array of formats to be mapped to each of the value in `$data`.
         *                                If string, that format will be used for all of the values in `$data`.
         *                                A format is one of '%d', '%f', '%s' (integer, float, string).
         *                                If omitted, all values in `$data` will be treated as strings unless otherwise
         *                                specified in wpdb::$field_types. Default null.
         * @return int|false The number of rows affected, or false on error.
         */
        public function replace($table, $data, $format = \null)
        {
        }
        /**
         * Helper function for insert and replace.
         *
         * Runs an insert or replace query based on `$type` argument.
         *
         * @since 3.0.0
         *
         * @see wpdb::prepare()
         * @see wpdb::$field_types
         * @see wp_set_wpdb_vars()
         *
         * @param string          $table  Table name.
         * @param array           $data   Data to insert (in column => value pairs).
         *                                Both `$data` columns and `$data` values should be "raw" (neither should be SQL escaped).
         *                                Sending a null value will cause the column to be set to NULL - the corresponding
         *                                format is ignored in this case.
         * @param string[]|string $format Optional. An array of formats to be mapped to each of the value in `$data`.
         *                                If string, that format will be used for all of the values in `$data`.
         *                                A format is one of '%d', '%f', '%s' (integer, float, string).
         *                                If omitted, all values in `$data` will be treated as strings unless otherwise
         *                                specified in wpdb::$field_types. Default null.
         * @param string          $type   Optional. Type of operation. Either 'INSERT' or 'REPLACE'.
         *                                Default 'INSERT'.
         * @return int|false The number of rows affected, or false on error.
         * @phpstan-param 'INSERT'|'REPLACE' $type
         */
        public function _insert_replace_helper($table, $data, $format = \null, $type = 'INSERT')
        {
        }
        /**
         * Updates a row in the table.
         *
         * Examples:
         *
         *     $wpdb->update(
         *         'table',
         *         array(
         *             'column1' => 'foo',
         *             'column2' => 'bar',
         *         ),
         *         array(
         *             'ID' => 1,
         *         )
         *     );
         *     $wpdb->update(
         *         'table',
         *         array(
         *             'column1' => 'foo',
         *             'column2' => 1337,
         *         ),
         *         array(
         *             'ID' => 1,
         *         ),
         *         array(
         *             '%s',
         *             '%d',
         *         ),
         *         array(
         *             '%d',
         *         )
         *     );
         *
         * @since 2.5.0
         *
         * @see wpdb::prepare()
         * @see wpdb::$field_types
         * @see wp_set_wpdb_vars()
         *
         * @param string          $table        Table name.
         * @param array           $data         Data to update (in column => value pairs).
         *                                      Both $data columns and $data values should be "raw" (neither should be SQL escaped).
         *                                      Sending a null value will cause the column to be set to NULL - the corresponding
         *                                      format is ignored in this case.
         * @param array           $where        A named array of WHERE clauses (in column => value pairs).
         *                                      Multiple clauses will be joined with ANDs.
         *                                      Both $where columns and $where values should be "raw".
         *                                      Sending a null value will create an IS NULL comparison - the corresponding
         *                                      format will be ignored in this case.
         * @param string[]|string $format       Optional. An array of formats to be mapped to each of the values in $data.
         *                                      If string, that format will be used for all of the values in $data.
         *                                      A format is one of '%d', '%f', '%s' (integer, float, string).
         *                                      If omitted, all values in $data will be treated as strings unless otherwise
         *                                      specified in wpdb::$field_types. Default null.
         * @param string[]|string $where_format Optional. An array of formats to be mapped to each of the values in $where.
         *                                      If string, that format will be used for all of the items in $where.
         *                                      A format is one of '%d', '%f', '%s' (integer, float, string).
         *                                      If omitted, all values in $where will be treated as strings unless otherwise
         *                                      specified in wpdb::$field_types. Default null.
         * @return int|false The number of rows updated, or false on error.
         */
        public function update($table, $data, $where, $format = \null, $where_format = \null)
        {
        }
        /**
         * Deletes a row in the table.
         *
         * Examples:
         *
         *     $wpdb->delete(
         *         'table',
         *         array(
         *             'ID' => 1,
         *         )
         *     );
         *     $wpdb->delete(
         *         'table',
         *         array(
         *             'ID' => 1,
         *         ),
         *         array(
         *             '%d',
         *         )
         *     );
         *
         * @since 3.4.0
         *
         * @see wpdb::prepare()
         * @see wpdb::$field_types
         * @see wp_set_wpdb_vars()
         *
         * @param string          $table        Table name.
         * @param array           $where        A named array of WHERE clauses (in column => value pairs).
         *                                      Multiple clauses will be joined with ANDs.
         *                                      Both $where columns and $where values should be "raw".
         *                                      Sending a null value will create an IS NULL comparison - the corresponding
         *                                      format will be ignored in this case.
         * @param string[]|string $where_format Optional. An array of formats to be mapped to each of the values in $where.
         *                                      If string, that format will be used for all of the items in $where.
         *                                      A format is one of '%d', '%f', '%s' (integer, float, string).
         *                                      If omitted, all values in $where will be treated as strings unless otherwise
         *                                      specified in wpdb::$field_types. Default null.
         * @return int|false The number of rows deleted, or false on error.
         */
        public function delete($table, $where, $where_format = \null)
        {
        }
        /**
         * Processes arrays of field/value pairs and field formats.
         *
         * This is a helper method for wpdb's CRUD methods, which take field/value pairs
         * for inserts, updates, and where clauses. This method first pairs each value
         * with a format. Then it determines the charset of that field, using that
         * to determine if any invalid text would be stripped. If text is stripped,
         * then field processing is rejected and the query fails.
         *
         * @since 4.2.0
         *
         * @param string          $table  Table name.
         * @param array           $data   Array of values keyed by their field names.
         * @param string[]|string $format Formats or format to be mapped to the values in the data.
         * @return array|false An array of fields that contain paired value and formats.
         *                     False for invalid values.
         */
        protected function process_fields($table, $data, $format)
        {
        }
        /**
         * Prepares arrays of value/format pairs as passed to wpdb CRUD methods.
         *
         * @since 4.2.0
         *
         * @param array           $data   Array of values keyed by their field names.
         * @param string[]|string $format Formats or format to be mapped to the values in the data.
         * @return array {
         *     Array of values and formats keyed by their field names.
         *
         *     @type array ...$0 {
         *         Value and format for this field.
         *
         *         @type mixed  $value  The value to be formatted.
         *         @type string $format The format to be mapped to the value.
         *     }
         * }
         * @phpstan-return array<int|string, array{
         *   value: mixed,
         *   format: string,
         * }>
         */
        protected function process_field_formats($data, $format)
        {
        }
        /**
         * Adds field charsets to field/value/format arrays generated by wpdb::process_field_formats().
         *
         * @since 4.2.0
         *
         * @param array  $data {
         *     Array of values and formats keyed by their field names,
         *     as it comes from the wpdb::process_field_formats() method.
         *
         *     @type array ...$0 {
         *         Value and format for this field.
         *
         *         @type mixed  $value  The value to be formatted.
         *         @type string $format The format to be mapped to the value.
         *     }
         * }
         * @param string $table Table name.
         * @return array|false {
         *     The same array of data with additional 'charset' keys, or false if
         *     the charset for the table cannot be found.
         *
         *     @type array ...$0 {
         *         Value, format, and charset for this field.
         *
         *         @type mixed        $value   The value to be formatted.
         *         @type string       $format  The format to be mapped to the value.
         *         @type string|false $charset The charset to be used for the value.
         *     }
         * }
         * @phpstan-param array<int|string, array{
         *   value: mixed,
         *   format: string,
         * }> $data
         * @phpstan-return false|array<int|string, array{
         *   value: mixed,
         *   format: string,
         *   charset: string|false,
         * }>
         */
        protected function process_field_charsets($data, $table)
        {
        }
        /**
         * For string fields, records the maximum string length that field can safely save.
         *
         * @since 4.2.1
         *
         * @param array  $data {
         *     Array of values, formats, and charsets keyed by their field names,
         *     as it comes from the wpdb::process_field_charsets() method.
         *
         *     @type array ...$0 {
         *         Value, format, and charset for this field.
         *
         *         @type mixed        $value   The value to be formatted.
         *         @type string       $format  The format to be mapped to the value.
         *         @type string|false $charset The charset to be used for the value.
         *     }
         * }
         * @param string $table Table name.
         * @return array|false {
         *     The same array of data with additional 'length' keys, or false if
         *     information for the table cannot be found.
         *
         *     @type array ...$0 {
         *         Value, format, charset, and length for this field.
         *
         *         @type mixed        $value   The value to be formatted.
         *         @type string       $format  The format to be mapped to the value.
         *         @type string|false $charset The charset to be used for the value.
         *         @type array|false  $length  {
         *             Information about the maximum length of the value.
         *             False if the column has no length.
         *
         *             @type string $type   One of 'byte' or 'char'.
         *             @type int    $length The column length.
         *         }
         *     }
         * }
         * @phpstan-param array<int|string, array{
         *   value: mixed,
         *   format: string,
         *   charset: string|false,
         * }> $data
         * @phpstan-return false|array<int|string, array{
         *   value: mixed,
         *   format: string,
         *   charset: string|false,
         *   length: false|array{
         *     type: string,
         *     length: int,
         *   },
         * }>
         */
        protected function process_field_lengths($data, $table)
        {
        }
        /**
         * Retrieves one value from the database.
         *
         * Executes a SQL query and returns the value from the SQL result.
         * If the SQL result contains more than one column and/or more than one row,
         * the value in the column and row specified is returned. If $query is null,
         * the value in the specified column and row from the previous SQL result is returned.
         *
         * Returns null both on failure and when the matched cell value is an empty
         * string. To distinguish the two cases, check {@see self::$last_error}.
         *
         * @since 0.71
         *
         * @param string|null $query Optional. SQL query. Defaults to null, use the result from the previous query.
         * @param int         $x     Optional. Column of value to return. Indexed from 0. Default 0.
         * @param int         $y     Optional. Row of value to return. Indexed from 0. Default 0.
         * @return string|null Database query result (as string), or null on failure or when the value is an empty string.
         * @phpstan-return non-empty-string|null
         */
        public function get_var($query = \null, $x = 0, $y = 0)
        {
        }
        /**
         * Retrieves one row from the database.
         *
         * Executes a SQL query and returns the row from the SQL result.
         *
         * @since 0.71
         *
         * @param string|null $query  SQL query.
         * @param string      $output Optional. The required return type. One of OBJECT, ARRAY_A, or ARRAY_N, which
         *                            correspond to an stdClass object, an associative array, or a numeric array,
         *                            respectively. Default OBJECT.
         * @param int         $y      Optional. Row to return. Indexed from 0. Default 0.
         * @return array|object|null Database query result in format specified by $output or null on failure.
         * @phpstan-param 'OBJECT'|'ARRAY_A'|'ARRAY_N' $output
         * @phpstan-return (
         *     $query is non-falsy-string
         *         ? (
         *             $output is 'OBJECT'
         *                 ? stdClass|null
         *                 : (
         *                     $output is 'ARRAY_A'
         *                         ? array<array-key, mixed>|null
         *                         : (
         *                             $output is 'ARRAY_N'
         *                                 ? list<mixed>|null
         *                                 : null
         *                         )
         *                 )
         *         )
         *         : null
         * )
         * @phpstan-param int<0, max> $y
         */
        public function get_row($query = \null, $output = \OBJECT, $y = 0)
        {
        }
        /**
         * Retrieves one column from the database.
         *
         * Executes a SQL query and returns the column from the SQL result.
         * If the SQL result contains more than one column, the column specified is returned.
         * If $query is null, the specified column from the previous SQL result is returned.
         *
         * @since 0.71
         *
         * @param string|null $query Optional. SQL query. Defaults to previous query.
         * @param int         $x     Optional. Column to return. Indexed from 0. Default 0.
         * @return array Database query result. Array indexed from 0 by SQL result row number.
         * @phpstan-return list<non-empty-string|null>
         */
        public function get_col($query = \null, $x = 0)
        {
        }
        /**
         * Retrieves an entire SQL result set from the database (i.e., many rows).
         *
         * Executes a SQL query and returns the entire SQL result.
         *
         * Returns an empty array when no rows match or when the database
         * reports an error for the query. Returns null when $query is empty,
         * when $output is not one of the recognized constants, or when the
         * query cannot run because the connection is not ready.
         *
         * @since 0.71
         *
         * @param string|null $query  SQL query.
         * @param string      $output Optional. Any of ARRAY_A | ARRAY_N | OBJECT | OBJECT_K constants.
         *                            With one of the first three, return an array of rows indexed
         *                            from 0 by SQL result row number. Each row is an associative array
         *                            (column => value, ...), a numerically indexed array (0 => value, ...),
         *                            or an object ( ->column = value ), respectively. With OBJECT_K,
         *                            return an associative array of row objects keyed by the value
         *                            of each row's first column's value. Duplicate keys are discarded.
         *                            Default OBJECT.
         * @return array|null Database query results. Empty array when no rows match
         *                    or on database error. Null when $query is empty, when
         *                    $output is invalid, or when the connection is not ready.
         * @phpstan-param 'OBJECT'|'OBJECT_K'|'ARRAY_A'|'ARRAY_N' $output
         * @phpstan-return (
         *     $query is non-falsy-string
         *         ? (
         *             $output is 'OBJECT'
         *                 ? list<stdClass>|null
         *                 : (
         *                     $output is 'OBJECT_K'
         *                         ? array<array-key, stdClass>
         *                         : (
         *                             $output is 'ARRAY_A'
         *                                 ? list<array<array-key, mixed>>
         *                                 : (
         *                                     $output is 'ARRAY_N'
         *                                         ? list<list<mixed>>
         *                                         : null
         *                                 )
         *                         )
         *                 )
         *         )
         *         : null
         * )
         */
        public function get_results($query = \null, $output = \OBJECT)
        {
        }
        /**
         * Retrieves the character set for the given table.
         *
         * @since 4.2.0
         *
         * @param string $table Table name.
         * @return string|WP_Error Table character set, WP_Error object if it couldn't be found.
         */
        protected function get_table_charset($table)
        {
        }
        /**
         * Retrieves the character set for the given column.
         *
         * @since 4.2.0
         *
         * @param string $table  Table name.
         * @param string $column Column name.
         * @return string|false|WP_Error Column character set as a string. False if the column has
         *                               no character set. WP_Error object if there was an error.
         */
        public function get_col_charset($table, $column)
        {
        }
        /**
         * Retrieves the maximum string length allowed in a given column.
         *
         * The length may either be specified as a byte length or a character length.
         *
         * @since 4.2.1
         *
         * @param string $table  Table name.
         * @param string $column Column name.
         * @return array|false|WP_Error {
         *     Array of column length information, false if the column has no length (for
         *     example, numeric column), WP_Error object if there was an error.
         *
         *     @type string $type   One of 'byte' or 'char'.
         *     @type int    $length The column length.
         * }
         * @phpstan-return false|\WP_Error|array{
         *   type: string,
         *   length: int,
         * }
         */
        public function get_col_length($table, $column)
        {
        }
        /**
         * Checks if a string is ASCII.
         *
         * The negative regex is faster for non-ASCII strings, as it allows
         * the search to finish as soon as it encounters a non-ASCII character.
         *
         * @since 4.2.0
         *
         * @param string $input_string String to check.
         * @return bool True if ASCII, false if not.
         */
        protected function check_ascii($input_string)
        {
        }
        /**
         * Checks if the query is accessing a collation considered safe.
         *
         * @since 4.2.0
         *
         * @param string $query The query to check.
         * @return bool True if the collation is safe, false if it isn't.
         */
        protected function check_safe_collation($query)
        {
        }
        /**
         * Strips any invalid characters based on value/charset pairs.
         *
         * @since 4.2.0
         *
         * @param array $data Array of value arrays. Each value array has the keys 'value', 'charset', and 'length'.
         *                    An optional 'ascii' key can be set to false to avoid redundant ASCII checks.
         * @return array|WP_Error The $data parameter, with invalid characters removed from each value.
         *                        This works as a passthrough: any additional keys such as 'field' are
         *                        retained in each value array. If we cannot remove invalid characters,
         *                        a WP_Error object is returned.
         */
        protected function strip_invalid_text($data)
        {
        }
        /**
         * Strips any invalid characters from the query.
         *
         * @since 4.2.0
         *
         * @param string $query Query to convert.
         * @return string|WP_Error The converted query, or a WP_Error object if the conversion fails.
         */
        protected function strip_invalid_text_from_query($query)
        {
        }
        /**
         * Strips any invalid characters from the string for a given table and column.
         *
         * @since 4.2.0
         *
         * @param string $table  Table name.
         * @param string $column Column name.
         * @param string $value  The text to check.
         * @return string|WP_Error The converted string, or a WP_Error object if the conversion fails.
         */
        public function strip_invalid_text_for_column($table, $column, $value)
        {
        }
        /**
         * Finds the first table name referenced in a query.
         *
         * @since 4.2.0
         *
         * @param string $query The query to search.
         * @return string|false The table name found, or false if a table couldn't be found.
         */
        protected function get_table_from_query($query)
        {
        }
        /**
         * Loads the column metadata from the last query.
         *
         * @since 3.5.0
         * @phpstan-return void
         */
        protected function load_col_info()
        {
        }
        /**
         * Retrieves column metadata from the last query.
         *
         * @since 0.71
         *
         * @param string $info_type  Optional. Possible values include 'name', 'table', 'def', 'max_length',
         *                           'not_null', 'primary_key', 'multiple_key', 'unique_key', 'numeric',
         *                           'blob', 'type', 'unsigned', 'zerofill'. Default 'name'.
         * @param int    $col_offset Optional. 0: col name. 1: which table the col's in. 2: col's max length.
         *                           3: if the col is numeric. 4: col's type. Default -1.
         * @return mixed Column results.
         */
        public function get_col_info($info_type = 'name', $col_offset = -1)
        {
        }
        /**
         * Starts the timer, for debugging purposes.
         *
         * @since 1.5.0
         *
         * @return true
         */
        public function timer_start()
        {
        }
        /**
         * Stops the debugging timer.
         *
         * @since 1.5.0
         *
         * @return float Total time spent on the query, in seconds.
         */
        public function timer_stop()
        {
        }
        /**
         * Wraps errors in a nice header and footer and dies.
         *
         * Will not die if wpdb::$show_errors is false.
         *
         * @since 1.5.0
         *
         * @param string $message    The error message.
         * @param string $error_code Optional. A computer-readable string to identify the error.
         *                           Default '500'.
         * @return false False if the showing of errors is disabled.
         */
        public function bail($message, $error_code = '500')
        {
        }
        /**
         * Closes the current database connection.
         *
         * @since 4.5.0
         *
         * @return bool True if the connection was successfully closed,
         *              false if it wasn't, or if the connection doesn't exist.
         */
        public function close()
        {
        }
        /**
         * Determines whether the database server is at least the required minimum version.
         *
         * @since 2.5.0
         *
         * @global string $required_mysql_version The minimum required MySQL version string.
         * @return WP_Error|null
         */
        public function check_database_version()
        {
        }
        /**
         * Determines whether the database supports collation.
         *
         * Called when WordPress is generating the table scheme.
         *
         * Use `wpdb::has_cap( 'collation' )`.
         *
         * @since 2.5.0
         * @deprecated 3.5.0 Use wpdb::has_cap()
         *
         * @return bool True if collation is supported, false if not.
         */
        public function supports_collation()
        {
        }
        /**
         * Retrieves the database character collate.
         *
         * @since 3.5.0
         *
         * @return string The database character collate.
         */
        public function get_charset_collate()
        {
        }
        /**
         * Determines whether the database or WPDB supports a particular feature.
         *
         * Capability sniffs for the database server and current version of WPDB.
         *
         * Database sniffs are based on the version of the database server in use.
         *
         * WPDB sniffs are added as new features are introduced to allow theme and plugin
         * developers to determine feature support. This is to account for drop-ins which may
         * introduce feature support at a different time to WordPress.
         *
         * @since 2.7.0
         * @since 4.1.0 Added support for the 'utf8mb4' feature.
         * @since 4.6.0 Added support for the 'utf8mb4_520' feature.
         * @since 6.2.0 Added support for the 'identifier_placeholders' feature.
         * @since 6.6.0 The `utf8mb4` feature now always returns true.
         *
         * @see wpdb::db_version()
         *
         * @param string $db_cap The feature to check for. Accepts 'collation', 'group_concat',
         *                       'subqueries', 'set_charset', 'utf8mb4', 'utf8mb4_520',
         *                       or 'identifier_placeholders'.
         * @return bool True when the database feature is supported, false otherwise.
         * @phpstan-param 'collation'|'group_concat'|'subqueries'|'set_charset'|'utf8mb4'|'utf8mb4_520'|'identifier_placeholders' $db_cap
         */
        public function has_cap($db_cap)
        {
        }
        /**
         * Retrieves a comma-separated list of the names of the functions that called wpdb.
         *
         * @since 2.5.0
         *
         * @return string Comma-separated list of the calling functions.
         */
        public function get_caller()
        {
        }
        /**
         * Retrieves the database server version number.
         *
         * @since 2.7.0
         *
         * @return string|null Version number on success, null on failure.
         */
        public function db_version()
        {
        }
        /**
         * Returns the raw version string of the database server.
         *
         * @since 5.5.0
         *
         * @return string Database server version as a string.
         */
        public function db_server_info()
        {
        }
    }
    /**
     * Class WP_Translations.
     *
     * @since 6.5.0
     *
     * @property-read array<string, string> $headers
     * @property-read array<string, string[]> $entries
     */
    class WP_Translations
    {
        /**
         * Text domain.
         *
         * @since 6.5.0
         * @var string
         */
        protected $textdomain = 'default';
        /**
         * Translation controller instance.
         *
         * @since 6.5.0
         * @var WP_Translation_Controller
         */
        protected $controller;
        /**
         * Constructor.
         *
         * @since 6.5.0
         *
         * @param WP_Translation_Controller $controller I18N controller.
         * @param string                    $textdomain Optional. Text domain. Default 'default'.
         */
        public function __construct(\WP_Translation_Controller $controller, string $textdomain = 'default')
        {
        }
        /**
         * Magic getter for backward compatibility.
         *
         * @since 6.5.0
         *
         * @param string $name Property name.
         * @return mixed
         */
        public function __get(string $name)
        {
        }
        /**
         * Translates a plural string.
         *
         * @since 6.5.0
         *
         * @param string|null $singular Singular string.
         * @param string|null $plural   Plural string.
         * @param int|float   $count    Count. Should be an integer, but some plugins pass floats.
         * @param string|null $context  Context.
         * @return string|null Translation if it exists, or the unchanged singular string.
         * @phpstan-template T of string|null
         * @phpstan-param T $singular
         * @phpstan-param int $count
         * @phpstan-return ($singular is null ? null : ($plural is null ? T : string))
         */
        public function translate_plural($singular, $plural, $count = 1, $context = '')
        {
        }
        /**
         * Translates a singular string.
         *
         * @since 6.5.0
         *
         * @param string|null $singular Singular string.
         * @param string|null $context  Context.
         * @return string|null Translation if it exists, or the unchanged singular string
         * @phpstan-return ($singular is null ? null : string)
         */
        public function translate($singular, $context = '')
        {
        }
    }
}

namespace {
    /**
     * Translations class.
     *
     * @since 2.8.0
     */
    #[\AllowDynamicProperties]
    class Translations
    {
        /**
         * List of translation entries.
         *
         * @since 2.8.0
         *
         * @var Translation_Entry[]
         */
        public $entries = array();
        /**
         * List of translation headers.
         *
         * @since 2.8.0
         *
         * @var array<string, string>
         */
        public $headers = array();
        /**
         * Adds an entry to the PO structure.
         *
         * @since 2.8.0
         *
         * @param array|Translation_Entry $entry
         * @return bool True on success, false if the entry doesn't have a key.
         */
        public function add_entry($entry)
        {
        }
        /**
         * Adds or merges an entry to the PO structure.
         *
         * @since 2.8.0
         *
         * @param array|Translation_Entry $entry
         * @return bool True on success, false if the entry doesn't have a key.
         */
        public function add_entry_or_merge($entry)
        {
        }
        /**
         * Sets $header PO header to $value
         *
         * If the header already exists, it will be overwritten
         *
         * TODO: this should be out of this class, it is gettext specific
         *
         * @since 2.8.0
         *
         * @param string $header header name, without trailing :
         * @param string $value header value, without trailing \n
         */
        public function set_header($header, $value)
        {
        }
        /**
         * Sets translation headers.
         *
         * @since 2.8.0
         *
         * @param array $headers Associative array of headers.
         */
        public function set_headers($headers)
        {
        }
        /**
         * Returns a given translation header.
         *
         * @since 2.8.0
         *
         * @param string $header
         * @return string|false Header if it exists, false otherwise.
         */
        public function get_header($header)
        {
        }
        /**
         * Returns a given translation entry.
         *
         * @since 2.8.0
         *
         * @param Translation_Entry $entry Translation entry.
         * @return Translation_Entry|false Translation entry if it exists, false otherwise.
         */
        public function translate_entry(&$entry)
        {
        }
        /**
         * Translates a singular string.
         *
         * @since 2.8.0
         *
         * @param string $singular
         * @param string $context
         * @return string
         */
        public function translate($singular, $context = \null)
        {
        }
        /**
         * Given the number of items, returns the 0-based index of the plural form to use
         *
         * Here, in the base Translations class, the common logic for English is implemented:
         *  0 if there is one element, 1 otherwise
         *
         * This function should be overridden by the subclasses. For example MO/PO can derive the logic
         * from their headers.
         *
         * @since 2.8.0
         *
         * @param int $count Number of items.
         * @return int Plural form to use.
         */
        public function select_plural_form($count)
        {
        }
        /**
         * Returns the plural forms count.
         *
         * @since 2.8.0
         *
         * @return int Plural forms count.
         */
        public function get_plural_forms_count()
        {
        }
        /**
         * Translates a plural string.
         *
         * @since 2.8.0
         *
         * @param string $singular
         * @param string $plural
         * @param int    $count
         * @param string $context
         * @return string
         */
        public function translate_plural($singular, $plural, $count, $context = \null)
        {
        }
        /**
         * Merges other translations into the current one.
         *
         * @since 2.8.0
         *
         * @param Translations $other Another Translation object, whose translations will be merged in this one (passed by reference).
         */
        public function merge_with(&$other)
        {
        }
        /**
         * Merges originals with existing entries.
         *
         * @since 2.8.0
         *
         * @param Translations $other
         */
        public function merge_originals_with(&$other)
        {
        }
    }
    /**
     * Core class used to implement a REST request object.
     *
     * Contains data from the request, to be passed to the callback.
     *
     * Note: This implements ArrayAccess, and acts as an array of parameters when
     * used in that manner. It does not use ArrayObject (as we cannot rely on SPL),
     * so be aware it may have non-array behavior in some cases.
     *
     * Note: When using features provided by ArrayAccess, be aware that WordPress deliberately
     * does not distinguish between arguments of the same name for different request methods.
     * For instance, in a request with `GET id=1` and `POST id=2`, `$request['id']` will equal
     * 2 (`POST`) not 1 (`GET`). For more precision between request methods, use
     * WP_REST_Request::get_body_params(), WP_REST_Request::get_url_params(), etc.
     *
     * @since 4.4.0
     *
     * @link https://www.php.net/manual/en/class.arrayaccess.php
     * @phpstan-template T of array = array<string, mixed>
     * @phpstan-implements ArrayAccess<key-of<T>, value-of<T>>
     */
    #[\AllowDynamicProperties]
    class WP_REST_Request implements \ArrayAccess
    {
        /**
         * HTTP method.
         *
         * @since 4.4.0
         * @var string
         */
        protected $method = '';
        /**
         * Parameters passed to the request.
         *
         * These typically come from the `$_GET`, `$_POST` and `$_FILES`
         * superglobals when being created from the global scope.
         *
         * @since 4.4.0
         * @var array Contains GET, POST and FILES keys mapping to arrays of data.
         */
        protected $params;
        /**
         * HTTP headers for the request.
         *
         * @since 4.4.0
         * @var array Map of key to value. Key is always lowercase, as per HTTP specification.
         */
        protected $headers = array();
        /**
         * Body data.
         *
         * @since 4.4.0
         * @var string Binary data from the request.
         */
        protected $body = \null;
        /**
         * Route matched for the request.
         *
         * @since 4.4.0
         * @var string
         */
        protected $route;
        /**
         * Attributes (options) for the route that was matched.
         *
         * This is the options array used when the route was registered, typically
         * containing the callback as well as the valid methods for the route.
         *
         * @since 4.4.0
         * @var array Attributes for the request.
         */
        protected $attributes = array();
        /**
         * Used to determine if the JSON data has been parsed yet.
         *
         * Allows lazy-parsing of JSON data where possible.
         *
         * @since 4.4.0
         * @var bool
         */
        protected $parsed_json = \false;
        /**
         * Used to determine if the body data has been parsed yet.
         *
         * @since 4.4.0
         * @var bool
         */
        protected $parsed_body = \false;
        /**
         * Constructor.
         *
         * @since 4.4.0
         *
         * @param string $method     Optional. Request method. Default empty.
         * @param string $route      Optional. Request route. Default empty.
         * @param array  $attributes Optional. Request attributes. Default empty array.
         */
        public function __construct($method = '', $route = '', $attributes = array())
        {
        }
        /**
         * Retrieves the HTTP method for the request.
         *
         * @since 4.4.0
         *
         * @return string HTTP method.
         */
        public function get_method()
        {
        }
        /**
         * Sets HTTP method for the request.
         *
         * @since 4.4.0
         *
         * @param string $method HTTP method.
         */
        public function set_method($method)
        {
        }
        /**
         * Retrieves all headers from the request.
         *
         * @since 4.4.0
         *
         * @return array Map of key to value. Key is always lowercase, as per HTTP specification.
         */
        public function get_headers()
        {
        }
        /**
         * Determines if the request is the given method.
         *
         * @since 6.8.0
         *
         * @param string $method HTTP method.
         * @return bool Whether the request is of the given method.
         */
        public function is_method($method)
        {
        }
        /**
         * Canonicalizes the header name.
         *
         * Ensures that header names are always treated the same regardless of
         * source. Header names are always case-insensitive.
         *
         * Note that we treat `-` (dashes) and `_` (underscores) as the same
         * character, as per header parsing rules in both Apache and nginx.
         *
         * @link https://stackoverflow.com/q/18185366
         * @link https://www.nginx.com/resources/wiki/start/topics/tutorials/config_pitfalls/#missing-disappearing-http-headers
         * @link https://nginx.org/en/docs/http/ngx_http_core_module.html#underscores_in_headers
         *
         * @since 4.4.0
         *
         * @param string $key Header name.
         * @return string Canonicalized name.
         */
        public static function canonicalize_header_name($key)
        {
        }
        /**
         * Retrieves the given header from the request.
         *
         * If the header has multiple values, they will be concatenated with a comma
         * as per the HTTP specification. Be aware that some non-compliant headers
         * (notably cookie headers) cannot be joined this way.
         *
         * @since 4.4.0
         *
         * @param string $key Header name, will be canonicalized to lowercase.
         * @return string|null String value if set, null otherwise.
         */
        public function get_header($key)
        {
        }
        /**
         * Retrieves header values from the request.
         *
         * @since 4.4.0
         *
         * @param string $key Header name, will be canonicalized to lowercase.
         * @return array|null List of string values if set, null otherwise.
         */
        public function get_header_as_array($key)
        {
        }
        /**
         * Sets the header on request.
         *
         * @since 4.4.0
         *
         * @param string $key   Header name.
         * @param string $value Header value, or list of values.
         */
        public function set_header($key, $value)
        {
        }
        /**
         * Appends a header value for the given header.
         *
         * @since 4.4.0
         *
         * @param string $key   Header name.
         * @param string $value Header value, or list of values.
         */
        public function add_header($key, $value)
        {
        }
        /**
         * Removes all values for a header.
         *
         * @since 4.4.0
         *
         * @param string $key Header name.
         */
        public function remove_header($key)
        {
        }
        /**
         * Sets headers on the request.
         *
         * @since 4.4.0
         *
         * @param array $headers  Map of header name to value.
         * @param bool  $override If true, replace the request's headers. Otherwise, merge with existing.
         */
        public function set_headers($headers, $override = \true)
        {
        }
        /**
         * Retrieves the Content-Type of the request.
         *
         * @since 4.4.0
         *
         * @return array|null Map containing 'value' and 'parameters' keys
         *                    or null when no valid Content-Type header was
         *                    available.
         */
        public function get_content_type()
        {
        }
        /**
         * Checks if the request has specified a JSON Content-Type.
         *
         * @since 5.6.0
         *
         * @return bool True if the Content-Type header is JSON.
         */
        public function is_json_content_type()
        {
        }
        /**
         * Retrieves the parameter priority order.
         *
         * Used when checking parameters in WP_REST_Request::get_param().
         *
         * @since 4.4.0
         *
         * @return string[] Array of types to check, in order of priority.
         */
        protected function get_parameter_order()
        {
        }
        /**
         * Retrieves a parameter from the request.
         *
         * @since 4.4.0
         *
         * @param string $key Parameter name.
         * @return mixed|null Value if set, null otherwise.
         * @phpstan-template TOffset of key-of<T>
         * @phpstan-param TOffset $key
         * @phpstan-return T[TOffset]|null
         */
        public function get_param($key)
        {
        }
        /**
         * Checks if a parameter exists in the request.
         *
         * This allows distinguishing between an omitted parameter,
         * and a parameter specifically set to null.
         *
         * @since 5.3.0
         *
         * @param string $key Parameter name.
         * @return bool True if a param exists for the given key.
         * @phpstan-param key-of<T> $key
         */
        public function has_param($key)
        {
        }
        /**
         * Sets a parameter on the request.
         *
         * If the given parameter key exists in any parameter type an update will take place,
         * otherwise a new param will be created in the first parameter type (respecting
         * get_parameter_order()).
         *
         * @since 4.4.0
         *
         * @param string $key   Parameter name.
         * @param mixed  $value Parameter value.
         * @phpstan-template TOffset of key-of<T>
         * @phpstan-param TOffset $key
         * @phpstan-param T[TOffset] $value
         * @phpstan-return void
         */
        public function set_param($key, $value)
        {
        }
        /**
         * Retrieves merged parameters from the request.
         *
         * The equivalent of get_param(), but returns all parameters for the request.
         * Handles merging all the available values into a single array.
         *
         * @since 4.4.0
         *
         * @return array Map of key to value.
         * @phpstan-return T
         */
        public function get_params()
        {
        }
        /**
         * Retrieves parameters from the route itself.
         *
         * These are parsed from the URL using the regex.
         *
         * @since 4.4.0
         *
         * @return array Parameter map of key to value.
         */
        public function get_url_params()
        {
        }
        /**
         * Sets parameters from the route.
         *
         * Typically, this is set after parsing the URL.
         *
         * @since 4.4.0
         *
         * @param array $params Parameter map of key to value.
         */
        public function set_url_params($params)
        {
        }
        /**
         * Retrieves parameters from the query string.
         *
         * These are the parameters you'd typically find in `$_GET`.
         *
         * @since 4.4.0
         *
         * @return array Parameter map of key to value.
         */
        public function get_query_params()
        {
        }
        /**
         * Sets parameters from the query string.
         *
         * Typically, this is set from `$_GET`.
         *
         * @since 4.4.0
         *
         * @param array $params Parameter map of key to value.
         */
        public function set_query_params($params)
        {
        }
        /**
         * Retrieves parameters from the body.
         *
         * These are the parameters you'd typically find in `$_POST`.
         *
         * @since 4.4.0
         *
         * @return array Parameter map of key to value.
         */
        public function get_body_params()
        {
        }
        /**
         * Sets parameters from the body.
         *
         * Typically, this is set from `$_POST`.
         *
         * @since 4.4.0
         *
         * @param array $params Parameter map of key to value.
         */
        public function set_body_params($params)
        {
        }
        /**
         * Retrieves multipart file parameters from the body.
         *
         * These are the parameters you'd typically find in `$_FILES`.
         *
         * @since 4.4.0
         *
         * @return array Parameter map of key to value.
         *
         * @phpstan-return array<string, array{
         *                                   name: non-empty-string,
         *                                   type: non-empty-string,
         *                                   size: non-negative-int,
         *                                   tmp_name: non-empty-string,
         *                                   error: int<0, 8>,
         *                                   full_path?: non-empty-string,
         *                               }>
         */
        public function get_file_params()
        {
        }
        /**
         * Sets multipart file parameters from the body.
         *
         * Typically, this is set from `$_FILES`.
         *
         * @since 4.4.0
         *
         * @param array $params Parameter map of key to value.
         *
         * @phpstan-param array<string, array{
         *                                  name: non-empty-string,
         *                                  type: non-empty-string,
         *                                  size: non-negative-int,
         *                                  tmp_name: non-empty-string,
         *                                  error: int<0, 8>,
         *                                  full_path?: non-empty-string,
         *                              }> $params
         */
        public function set_file_params($params)
        {
        }
        /**
         * Retrieves the default parameters.
         *
         * These are the parameters set in the route registration.
         *
         * @since 4.4.0
         *
         * @return array Parameter map of key to value.
         */
        public function get_default_params()
        {
        }
        /**
         * Sets default parameters.
         *
         * These are the parameters set in the route registration.
         *
         * @since 4.4.0
         *
         * @param array $params Parameter map of key to value.
         */
        public function set_default_params($params)
        {
        }
        /**
         * Retrieves the request body content.
         *
         * @since 4.4.0
         *
         * @return string Binary data from the request body.
         */
        public function get_body()
        {
        }
        /**
         * Sets body content.
         *
         * @since 4.4.0
         *
         * @param string $data Binary data from the request body.
         */
        public function set_body($data)
        {
        }
        /**
         * Retrieves the parameters from a JSON-formatted body.
         *
         * @since 4.4.0
         *
         * @return array Parameter map of key to value.
         */
        public function get_json_params()
        {
        }
        /**
         * Parses the JSON parameters.
         *
         * Avoids parsing the JSON data until we need to access it.
         *
         * @since 4.4.0
         * @since 4.7.0 Returns error instance if value cannot be decoded.
         * @return true|WP_Error True if the JSON data was passed or no JSON data was provided, WP_Error if invalid JSON was passed.
         */
        protected function parse_json_params()
        {
        }
        /**
         * Parses the request body parameters.
         *
         * Parses out URL-encoded bodies for request methods that aren't supported
         * natively by PHP.
         *
         * @since 4.4.0
         * @phpstan-return void
         */
        protected function parse_body_params()
        {
        }
        /**
         * Retrieves the route that matched the request.
         *
         * @since 4.4.0
         *
         * @return string Route matching regex.
         */
        public function get_route()
        {
        }
        /**
         * Sets the route that matched the request.
         *
         * @since 4.4.0
         *
         * @param string $route Route matching regex.
         */
        public function set_route($route)
        {
        }
        /**
         * Retrieves the attributes for the request.
         *
         * These are the options for the route that was matched.
         *
         * @since 4.4.0
         *
         * @return array Attributes for the request.
         */
        public function get_attributes()
        {
        }
        /**
         * Sets the attributes for the request.
         *
         * @since 4.4.0
         *
         * @param array $attributes Attributes for the request.
         */
        public function set_attributes($attributes)
        {
        }
        /**
         * Sanitizes (where possible) the params on the request.
         *
         * This is primarily based off the sanitize_callback param on each registered
         * argument.
         *
         * @since 4.4.0
         *
         * @return true|WP_Error True if parameters were sanitized, WP_Error if an error occurred during sanitization.
         */
        public function sanitize_params()
        {
        }
        /**
         * Checks whether this request is valid according to its attributes.
         *
         * @since 4.4.0
         *
         * @return true|WP_Error True if there are no parameters to validate or if all pass validation,
         *                       WP_Error if required parameters are missing.
         */
        public function has_valid_params()
        {
        }
        /**
         * Checks if a parameter is set.
         *
         * @since 4.4.0
         *
         * @param string $offset Parameter name.
         * @return bool Whether the parameter is set.
         * @phpstan-param key-of<T> $offset
         */
        #[\ReturnTypeWillChange]
        public function offsetExists($offset)
        {
        }
        /**
         * Retrieves a parameter from the request.
         *
         * @since 4.4.0
         *
         * @param string $offset Parameter name.
         * @return mixed|null Value if set, null otherwise.
         * @phpstan-template TOffset of key-of<T>
         * @phpstan-param TOffset $offset
         * @phpstan-return T[TOffset]|null
         */
        #[\ReturnTypeWillChange]
        public function offsetGet($offset)
        {
        }
        /**
         * Sets a parameter on the request.
         *
         * @since 4.4.0
         *
         * @param string $offset Parameter name.
         * @param mixed  $value  Parameter value.
         * @phpstan-template TOffset of key-of<T>
         * @phpstan-param TOffset $offset
         * @phpstan-param T[TOffset] $value
         * @phpstan-return void
         */
        #[\ReturnTypeWillChange]
        public function offsetSet($offset, $value)
        {
        }
        /**
         * Removes a parameter from the request.
         *
         * @since 4.4.0
         *
         * @param string $offset Parameter name.
         * @phpstan-template TOffset of key-of<T>
         * @phpstan-param TOffset $offset
         * @phpstan-return void
         */
        #[\ReturnTypeWillChange]
        public function offsetUnset($offset)
        {
        }
        /**
         * Retrieves a WP_REST_Request object from a full URL.
         *
         * @since 4.5.0
         *
         * @param string $url URL with protocol, domain, path and query args.
         * @return WP_REST_Request|false WP_REST_Request object on success, false on failure.
         */
        public static function from_url($url)
        {
        }
    }
    /**
     * Core class used to implement a REST response object.
     *
     * @since 4.4.0
     *
     * @see WP_HTTP_Response
     */
    class WP_REST_Response extends \WP_HTTP_Response
    {
        /**
         * Links related to the response.
         *
         * @since 4.4.0
         * @var array
         */
        protected $links = array();
        /**
         * The route that was to create the response.
         *
         * @since 4.4.0
         * @var string
         */
        protected $matched_route = '';
        /**
         * The handler that was used to create the response.
         *
         * @since 4.4.0
         * @var null|array
         */
        protected $matched_handler = \null;
        /**
         * Adds a link to the response.
         *
         * {@internal The $rel parameter is first, as this looks nicer when sending multiple.}
         *
         * @since 4.4.0
         *
         * @link https://tools.ietf.org/html/rfc5988
         * @link https://www.iana.org/assignments/link-relations/link-relations.xml
         *
         * @param string $rel        Link relation. Either an IANA registered type,
         *                           or an absolute URL.
         * @param string $href       Target URI for the link.
         * @param array  $attributes Optional. Link parameters to send along with the URL. Default empty array.
         */
        public function add_link($rel, $href, $attributes = array())
        {
        }
        /**
         * Removes a link from the response.
         *
         * @since 4.4.0
         *
         * @param string      $rel  Link relation. Either an IANA registered type, or an absolute URL.
         * @param string|null $href Optional. Only remove links for the relation matching the given href.
         *                          Default null.
         * @phpstan-return void
         */
        public function remove_link($rel, $href = \null)
        {
        }
        /**
         * Adds multiple links to the response.
         *
         * Link data should be an associative array with link relation as the key.
         * The value can either be an associative array of link attributes
         * (including `href` with the URL for the response), or a list of these
         * associative arrays.
         *
         * @since 4.4.0
         *
         * @param array $links Map of link relation to list of links.
         */
        public function add_links($links)
        {
        }
        /**
         * Retrieves links for the response.
         *
         * @since 4.4.0
         *
         * @return array List of links.
         */
        public function get_links()
        {
        }
        /**
         * Sets a single link header.
         *
         * {@internal The $rel parameter is first, as this looks nicer when sending multiple.}
         *
         * @since 4.4.0
         *
         * @link https://tools.ietf.org/html/rfc5988
         * @link https://www.iana.org/assignments/link-relations/link-relations.xml
         *
         * @param string $rel   Link relation. Either an IANA registered type, or an absolute URL.
         * @param string $link  Target IRI for the link.
         * @param array  $other Optional. Other parameters to send, as an associative array.
         *                      Default empty array.
         */
        public function link_header($rel, $link, $other = array())
        {
        }
        /**
         * Retrieves the route that was used.
         *
         * @since 4.4.0
         *
         * @return string The matched route.
         */
        public function get_matched_route()
        {
        }
        /**
         * Sets the route (regex for path) that caused the response.
         *
         * @since 4.4.0
         *
         * @param string $route Route name.
         */
        public function set_matched_route($route)
        {
        }
        /**
         * Retrieves the handler that was used to generate the response.
         *
         * @since 4.4.0
         *
         * @return null|array The handler that was used to create the response.
         */
        public function get_matched_handler()
        {
        }
        /**
         * Sets the handler that was responsible for generating the response.
         *
         * @since 4.4.0
         *
         * @param array $handler The matched handler.
         */
        public function set_matched_handler($handler)
        {
        }
        /**
         * Checks if the response is an error, i.e. >= 400 response code.
         *
         * @since 4.4.0
         *
         * @return bool Whether the response is an error.
         */
        public function is_error()
        {
        }
        /**
         * Retrieves a WP_Error object from the response.
         *
         * @since 4.4.0
         *
         * @return WP_Error|null WP_Error or null on not an errored response.
         */
        public function as_error()
        {
        }
        /**
         * Retrieves the CURIEs (compact URIs) used for relations.
         *
         * @since 4.5.0
         *
         * @return array Compact URIs.
         */
        public function get_curies()
        {
        }
    }
    /**
     * Core class used to implement the Archives widget.
     *
     * @since 2.8.0
     *
     * @see WP_Widget
     */
    class WP_Widget_Archives extends \WP_Widget
    {
        /**
         * Sets up a new Archives widget instance.
         *
         * @since 2.8.0
         */
        public function __construct()
        {
        }
        /**
         * Outputs the content for the current Archives widget instance.
         *
         * @since 2.8.0
         *
         * @param array $args     Display arguments including 'before_title', 'after_title',
         *                        'before_widget', and 'after_widget'.
         * @param array $instance Settings for the current Archives widget instance.
         */
        public function widget($args, $instance)
        {
        }
        /**
         * Handles updating settings for the current Archives widget instance.
         *
         * @since 2.8.0
         *
         * @param array $new_instance New settings for this instance as input by the user via
         *                            WP_Widget_Archives::form().
         * @param array $old_instance Old settings for this instance.
         * @return array Updated settings to save.
         */
        public function update($new_instance, $old_instance)
        {
        }
        /**
         * Outputs the settings form for the Archives widget.
         *
         * @since 2.8.0
         *
         * @param array $instance Current settings.
         */
        public function form($instance)
        {
        }
    }
}

namespace {
    /**
     * Adds a link using values provided in $_POST.
     *
     * @since 2.0.0
     *
     * @return int The link ID on success. The value 0 on failure.
     * @phpstan-return int<0, max>
     */
    function add_link()
    {
    }
    /**
     * Updates or inserts a link using values provided in $_POST.
     *
     * @since 2.0.0
     *
     * @param int $link_id Optional. ID of the link to edit. Default 0.
     * @return int The link ID on success. The value 0 on failure.
     * @phpstan-return int<0, max>
     */
    function edit_link($link_id = 0)
    {
    }
    /**
     * Retrieves the default link for editing.
     *
     * @since 2.0.0
     *
     * @return stdClass Default link object.
     * @phpstan-impure
     * @phpstan-return object{link_url: string, link_name: string, link_visible: 'Y'}&\stdClass
     */
    function get_default_link_to_edit()
    {
    }
    /**
     * Retrieves the link category IDs associated with the link specified.
     *
     * @since 2.1.0
     *
     * @param int $link_id Link ID to look up.
     * @return int[] The IDs of the requested link's categories.
     * @phpstan-return ($link_id is empty ? array{} : array<int, int<1, max>>)
     */
    function wp_get_link_cats($link_id = 0)
    {
    }
    /**
     * Inserts a link into the database, or updates an existing link.
     *
     * Runs all the necessary sanitizing, provides default values if arguments are missing,
     * and finally saves the link.
     *
     * @since 2.0.0
     *
     * @global wpdb $wpdb WordPress database abstraction object.
     *
     * @param array $linkdata {
     *     Elements that make up the link to insert.
     *
     *     @type int    $link_id          Optional. The ID of the existing link if updating.
     *     @type string $link_url         The URL the link points to.
     *     @type string $link_name        The title of the link.
     *     @type string $link_image       Optional. A URL of an image.
     *     @type string $link_target      Optional. The target element for the anchor tag.
     *     @type string $link_description Optional. A short description of the link.
     *     @type string $link_visible     Optional. 'Y' means visible, anything else means not.
     *     @type int    $link_owner       Optional. A user ID.
     *     @type int    $link_rating      Optional. A rating for the link.
     *     @type string $link_rel         Optional. A relationship of the link to you.
     *     @type string $link_notes       Optional. An extended description of or notes on the link.
     *     @type string $link_rss         Optional. A URL of an associated RSS feed.
     *     @type int    $link_category    Optional. The term ID of the link category.
     *                                    If empty, uses default link category.
     * }
     * @param bool  $wp_error Optional. Whether to return a WP_Error object on failure. Default false.
     * @return int|WP_Error The link ID on success. The value 0 or WP_Error on failure.
     * @phpstan-param array{
     *   link_id?: int,
     *   link_url?: string,
     *   link_name?: string,
     *   link_image?: string,
     *   link_target?: string,
     *   link_description?: string,
     *   link_visible?: string,
     *   link_owner?: int,
     *   link_rating?: int,
     *   link_rel?: string,
     *   link_notes?: string,
     *   link_rss?: string,
     *   link_category?: int,
     * } $linkdata
     * @phpstan-return ($wp_error is false ? int<0, max> : int<0, max>|\WP_Error)
     */
    function wp_insert_link($linkdata, $wp_error = \false)
    {
    }
    /**
     * Updates a link in the database.
     *
     * @since 2.0.0
     *
     * @param array $linkdata Link data to update. See wp_insert_link() for accepted arguments.
     * @return int The updated link ID on success. The value 0 on failure.
     * @phpstan-param array{
     *   link_id?: int,
     *   link_url?: string,
     *   link_name?: string,
     *   link_image?: string,
     *   link_target?: string,
     *   link_description?: string,
     *   link_visible?: string,
     *   link_owner?: int,
     *   link_rating?: int,
     *   link_rel?: string,
     *   link_notes?: string,
     *   link_rss?: string,
     *   link_category?: int,
     * } $linkdata See wp_insert_link()
     * @phpstan-return int<0, max>
     */
    function wp_update_link($linkdata)
    {
    }
    /**
     * Adds avatars to relevant places in admin.
     *
     * @since 2.5.0
     *
     * @param string $name User name.
     * @return string Avatar with the user name.
     */
    function floated_admin_avatar($name)
    {
    }
    /**
     * Fetches an instance of a WP_List_Table class.
     *
     * @since 3.1.0
     *
     * @global string $hook_suffix
     *
     * @param string $class_name The type of the list table, which is the class name.
     * @param array  $args       Optional. Arguments to pass to the class. Accepts 'screen'.
     * @return WP_List_Table|false List table object on success, false if the class does not exist.
     * @phpstan-template T of string
     * @phpstan-param T $class_name
     * @phpstan-param array{screen?: string} $args
     * @phpstan-return ($class_name is 'WP_Posts_List_Table'|'WP_Media_List_Table'|'WP_Terms_List_Table'|'WP_Users_List_Table'|'WP_Comments_List_Table'|'WP_Post_Comments_List_Table'|'WP_Links_List_Table'|'WP_Plugin_Install_List_Table'|'WP_Themes_List_Table'|'WP_Theme_Install_List_Table'|'WP_Plugins_List_Table'|'WP_Application_Passwords_List_Table'|'WP_MS_Sites_List_Table'|'WP_MS_Users_List_Table'|'WP_MS_Themes_List_Table'|'WP_Privacy_Data_Export_Requests_List_Table'|'WP_Privacy_Data_Removal_Requests_List_Table' ? new<T> : false)
     */
    function _get_list_table($class_name, $args = array())
    {
    }
    /**
     * Retrieves HTML for the size radio buttons with the specified one checked.
     *
     * @since 2.7.0
     *
     * @param WP_Post     $post
     * @param bool|string $check
     * @return array<string, string> An array of data for the image size input fields.
     * @phpstan-return array{label: string, input: 'html', html: string}
     */
    function image_size_input_fields($post, $check = '')
    {
    }
    /**
     * Retrieves HTML for the Link URL buttons with the default link type as specified.
     *
     * @since 2.7.0
     *
     * @param WP_Post $post
     * @param string  $url_type
     * @return string HTML markup for the link URL buttons.
     * @phpstan-return non-falsy-string
     */
    function image_link_input_fields($post, $url_type = '')
    {
    }
    /**
     * Outputs a textarea element for inputting an attachment caption.
     *
     * @since 3.4.0
     *
     * @param WP_Post $edit_post Attachment WP_Post object.
     * @return string HTML markup for the textarea element.
     * @phpstan-return non-falsy-string
     */
    function wp_caption_input_textarea($edit_post)
    {
    }
    /**
     * Retrieves HTML form for modifying the image attachment.
     *
     * @since 2.5.0
     *
     * @global string $redir_tab
     *
     * @param int          $attachment_id Attachment ID for modification.
     * @param string|array $args          Optional. Override defaults.
     * @return string HTML form for attachment.
     * @phpstan-return non-falsy-string
     */
    function get_media_item($attachment_id, $args = \null)
    {
    }
    /**
     * Retrieves the media markup for an attachment.
     *
     * @since 3.5.0
     *
     * @param int   $attachment_id
     * @param array $args
     * @return array<string, string> An array containing the media item and its metadata.
     * @phpstan-return array{item: string, meta: string}
     */
    function get_compat_media_markup($attachment_id, $args = \null)
    {
    }
    /**
     * Creates the form for external url.
     *
     * @since 2.7.0
     *
     * @param string $default_view
     * @return string HTML content of the form.
     * @phpstan-return non-falsy-string
     */
    function wp_media_insert_url_form($default_view = 'image')
    {
    }
    /**
     * Adds a CSS class to a string.
     *
     * @since 2.7.0
     *
     * @param string $class_to_add The CSS class to add.
     * @param string $classes      The string to add the CSS class to.
     * @return string The string with the CSS class added.
     * @phpstan-template T of string
     * @phpstan-param T $class_to_add
     * @phpstan-pure
     * @phpstan-return ($classes is empty ? T : non-empty-string)
     */
    function add_cssclass($class_to_add, $classes)
    {
    }
    /**
     * Returns the arguments for the help tab on the Edit Site screens.
     *
     * @since 4.9.0
     *
     * @return array Help tab arguments.
     * @phpstan-return array{id: 'overview', title: string, content: non-falsy-string}
     */
    function get_site_screen_help_tab_args()
    {
    }
    /**
     * Returns the content for the help sidebar on the Edit Site screens.
     *
     * @since 4.9.0
     *
     * @return string Help sidebar content.
     * @phpstan-return non-falsy-string
     */
    function get_site_screen_help_sidebar_content()
    {
    }
    /**
     * Returns the columns for the nav menus page.
     *
     * @since 3.0.0
     *
     * @return string[] Array of column titles keyed by their column name.
     * @phpstan-return array{_title: string, cb: '<input type="checkbox" />', link-target: string, title-attribute: string, css-classes: string, xfn: string, description: string}
     */
    function wp_nav_menu_manage_columns()
    {
    }
    /**
     * Removes directory and files of a plugin for a list of plugins.
     *
     * @since 2.6.0
     *
     * @global WP_Filesystem_Base $wp_filesystem WordPress filesystem subclass.
     *
     * @param string[] $plugins    List of plugin paths to delete, relative to the plugins directory.
     * @param string   $deprecated Not used.
     * @return bool|null|WP_Error True on success, false if `$plugins` is empty, `WP_Error` on failure.
     *                            `null` if filesystem credentials are required to proceed.
     * @phpstan-param '' $deprecated
     * @phpstan-return ($plugins is empty ? false : true|null|WP_Error)
     */
    function delete_plugins($plugins, $deprecated = '')
    {
    }
    /**
     * Validates the plugin path.
     *
     * Checks that the main plugin file exists and is a valid plugin. See validate_file().
     *
     * @since 2.5.0
     *
     * @param string $plugin Path to the plugin file relative to the plugins directory.
     * @return int|WP_Error 0 on success, WP_Error on failure.
     * @phpstan-return ($plugin is empty ? \WP_Error : 0|\WP_Error)
     */
    function validate_plugin($plugin)
    {
    }
    /**
     * Gets the hook name for the administrative page of a plugin.
     *
     * @since 1.5.0
     *
     * @global array $admin_page_hooks
     *
     * @param string $plugin_page The slug name of the plugin page.
     * @param string $parent_page The slug name for the parent menu (or the file name of a standard
     *                            WordPress admin page).
     * @return string Hook name for the plugin page.
     * @phpstan-return non-falsy-string
     */
    function get_plugin_page_hookname($plugin_page, $parent_page)
    {
    }
    /**
     * Returns all the possible statuses for a post type.
     *
     * @since 2.5.0
     *
     * @param string $type The post_type you want the statuses for. Default 'post'.
     * @return string[] An array of all the statuses for the supplied post type.
     * @phpstan-return list<string>
     */
    function get_available_post_statuses($type = 'post')
    {
    }
    /**
     * Updates an existing Category or creates a new Category.
     *
     * @since 2.0.0
     * @since 2.5.0 $wp_error parameter was added.
     * @since 3.0.0 The 'taxonomy' argument was added.
     *
     * @param array $catarr {
     *     Array of arguments for inserting a new category.
     *
     *     @type int        $cat_ID               Category ID. A non-zero value updates an existing category.
     *                                            Default 0.
     *     @type string     $taxonomy             Taxonomy slug. Default 'category'.
     *     @type string     $cat_name             Category name. Default empty.
     *     @type string     $category_description Category description. Default empty.
     *     @type string     $category_nicename    Category nice (display) name. Default empty.
     *     @type int|string $category_parent      Category parent ID. Default empty.
     * }
     * @param bool  $wp_error Optional. Default false.
     * @return int|WP_Error The ID number of the new or updated Category on success. Zero or a WP_Error on failure,
     *                      depending on param `$wp_error`.
     * @phpstan-param array{
     *   cat_ID?: int,
     *   taxonomy?: string,
     *   cat_name?: string,
     *   category_description?: string,
     *   category_nicename?: string,
     *   category_parent?: int|string,
     * } $catarr
     * @phpstan-return ($wp_error is false ? int<0, max> : int<1, max>|\WP_Error)
     */
    function wp_insert_category($catarr, $wp_error = \false)
    {
    }
    /**
     * Aliases wp_insert_category() with minimal args.
     *
     * If you want to update only some fields of an existing category, call this
     * function with only the new values set inside $catarr.
     *
     * @since 2.0.0
     *
     * @param array $catarr The 'cat_ID' value is required. All other keys are optional.
     * @return int|false The ID number of the new or updated Category on success. Zero or FALSE on failure.
     * @phpstan-return int<0, max>|false
     */
    function wp_update_category($catarr)
    {
    }
    /**
     * Checks whether a post tag with a given name exists.
     *
     * @since 2.3.0
     *
     * @param int|string $tag_name
     * @return mixed Returns null if the term does not exist.
     *               Returns an array of the term ID and the term taxonomy ID if the pairing exists.
     *               Returns 0 if term ID 0 is passed to the function.
     * @phpstan-return ($tag_name is 0 ? 0 : ($tag_name is '' ? null : array{term_id: string, term_taxonomy_id: string}|null))
     */
    function tag_exists($tag_name)
    {
    }
    /**
     * Meta-Box template function.
     *
     * @since 2.5.0
     *
     * @global array $wp_meta_boxes Global meta box state.
     *
     * @param string|WP_Screen $screen      The screen identifier. If you have used add_menu_page() or
     *                                      add_submenu_page() to create a new screen (and hence screen_id)
     *                                      make sure your menu slug conforms to the limits of sanitize_key()
     *                                      otherwise the 'screen' menu may not correctly render on your page.
     * @param string           $context     The screen context for which to display meta boxes.
     * @param mixed            $data_object Gets passed to the meta box callback function as the first parameter.
     *                                      Often this is the object that's the focus of the current screen,
     *                                      for example a `WP_Post` or `WP_Comment` object.
     * @return int Number of meta_boxes.
     * @phpstan-return int<0, max>
     */
    function do_meta_boxes($screen, $context, $data_object)
    {
    }
    /**
     * Returns a submit button, with provided text and appropriate class.
     *
     * @since 3.1.0
     *
     * @param string       $text             Optional. The text of the button. Defaults to 'Save Changes'.
     * @param string       $type             Optional. The type and CSS class(es) of the button. Core values
     *                                       include 'primary', 'small', 'compact' and 'large'. Default 'primary large'.
     * @param string       $name             Optional. The HTML name of the submit button. If no `id` attribute
     *                                       is given in the `$other_attributes` parameter, `$name` will be used
     *                                       as the button's `id`. Default 'submit'.
     * @param bool         $wrap             Optional. True if the output button should be wrapped in a paragraph tag,
     *                                       false otherwise. Default true.
     * @param array|string $other_attributes Optional. Other attributes that should be output with the button,
     *                                       mapping attributes to their values, e.g. `array( 'id' => 'search-submit' )`.
     *                                       These key/value attribute pairs will be output as `attribute="value"`,
     *                                       where attribute is the key. Attributes can also be provided as a string,
     *                                       e.g. `id="search-submit"`, though the array format is generally preferred.
     *                                       Default empty string.
     * @return string Submit button HTML.
     * @phpstan-return non-falsy-string
     */
    function get_submit_button($text = '', $type = 'primary large', $name = 'submit', $wrap = \true, $other_attributes = '')
    {
    }
    /**
     * @param string $classes
     * @return string Modified body classes.
     * @phpstan-return non-falsy-string
     */
    function wp_widgets_access_body_class($classes)
    {
    }
    /**
     * Gets the elements class names.
     *
     * @since 6.0.0
     * @access private
     *
     * @return string The unique class name.
     * @phpstan-return lowercase-string&non-falsy-string
     */
    function wp_get_elements_class_name(): string
    {
    }
    /**
     * Creates an export of the current templates and
     * template parts from the site editor at the
     * specified path in a ZIP file.
     *
     * @since 5.9.0
     * @since 6.0.0 Adds the whole theme to the export archive.
     *
     * @return WP_Error|string Path of the ZIP file or error on failure.
     * @phpstan-return non-falsy-string|\WP_Error
     */
    function wp_generate_block_templates_export_file()
    {
    }
    /**
     * Returns the current version of the block format that the content string is using.
     *
     * If the string doesn't contain blocks, it returns 0.
     *
     * @since 5.0.0
     *
     * @param string $content Content to test.
     * @return int The block format version is 1 if the content contains one or more blocks, 0 otherwise.
     * @phpstan-pure
     * @phpstan-return ($content is '' ? 0 : 0|1)
     */
    function block_version($content)
    {
    }
    /**
     * Helper function that returns the proper pagination arrow HTML for
     * `QueryPaginationNext` and `QueryPaginationPrevious` blocks based
     * on the provided `paginationArrow` from `QueryPagination` context.
     *
     * It's used in QueryPaginationNext and QueryPaginationPrevious blocks.
     *
     * @since 5.9.0
     *
     * @param WP_Block $block   Block instance.
     * @param bool     $is_next Flag for handling `next/previous` blocks.
     * @return string|null The pagination arrow HTML or null if there is none.
     * @phpstan-return non-falsy-string|null
     */
    function get_query_pagination_arrow($block, $is_next)
    {
    }
    /**
     * Helper function that returns the proper pagination arrow HTML for
     * `CommentsPaginationNext` and `CommentsPaginationPrevious` blocks based on the
     * provided `paginationArrow` from `CommentsPagination` context.
     *
     * It's used in CommentsPaginationNext and CommentsPaginationPrevious blocks.
     *
     * @since 6.0.0
     *
     * @param WP_Block $block           Block instance.
     * @param string   $pagination_type Optional. Type of the arrow we will be rendering.
     *                                  Accepts 'next' or 'previous'. Default 'next'.
     * @return string|null The pagination arrow HTML or null if there is none.
     * @phpstan-param 'next'|'previous' $pagination_type
     * @phpstan-return non-falsy-string|null
     */
    function get_comments_pagination_arrow($block, $pagination_type = 'next')
    {
    }
    /**
     * Renders the `core/archives` block on server.
     *
     * @since 5.0.0
     *
     * @see WP_Widget_Archives
     *
     * @param array $attributes The block attributes.
     *
     * @return string Returns the post content with archives added.
     * @phpstan-return non-falsy-string
     */
    function render_block_core_archives($attributes)
    {
    }
    /**
     * Creates a breadcrumb item that's either a link or current page item.
     *
     * When paginated (is_paged is true), creates a link to page 1.
     * Otherwise, creates a span marked as the current page.
     *
     * @since 7.0.0
     *
     * @param string $text       The text content.
     * @param bool   $is_paged   Whether we're on a paginated view.
     *
     * @return array The breadcrumb item data.
     * @phpstan-template T of string
     * @phpstan-param T $text
     * @phpstan-return ($is_paged is true ? array{label: T, url: string} : array{label: T})
     */
    function block_core_breadcrumbs_create_item($text, $is_paged = \false)
    {
    }
    /**
     * Renders the `core/categories` block on server.
     *
     * @since 5.0.0
     * @since 6.7.0 Enable client-side rendering if enhancedPagination context is true.
     *
     * @param array    $attributes The block attributes.
     * @param string   $content    Block default content.
     * @param WP_Block $block      Block instance.
     *
     * @return string Returns the categories list/dropdown markup.
     * @phpstan-return non-falsy-string
     */
    function render_block_core_categories($attributes, $content, $block)
    {
    }
    /**
     * Generates the inline script for a categories dropdown field.
     *
     * @since 5.0.0
     *
     * @param string $dropdown_id ID of the dropdown field.
     *
     * @return string Returns the dropdown onChange redirection script.
     * @phpstan-return non-falsy-string
     */
    function build_dropdown_script_block_core_categories($dropdown_id)
    {
    }
    /**
     * Build an array with CSS classes and inline styles defining the colors
     * which will be applied to the home link markup in the front-end.
     *
     * @since 6.0.0
     *
     * @param  array $context home link block context.
     * @return array Colors CSS classes and inline styles.
     * @phpstan-return array{css_classes: list<string>, inline_styles: string}
     */
    function block_core_home_link_build_css_colors($context)
    {
    }
    /**
     * Get the post title.
     *
     * The post title is fetched and if it is blank then a default string is
     * returned.
     *
     * Copied from `wp-admin/includes/template.php`, but we can't include that
     * file because:
     *
     * 1. It causes bugs with test fixture generation and strange Docker 255 error
     *    codes.
     * 2. It's in the admin; ideally we *shouldn't* be including files from the
     *    admin for a block's output. It's a very small/simple function as well,
     *    so duplicating it isn't too terrible.
     *
     * @since 3.3.0
     *
     * @param int|WP_Post $post Optional. Post ID or WP_Post object. Default is global $post.
     * @return string The post title if set; "(no title)" if no title is set.
     * @phpstan-return non-falsy-string
     */
    function wp_latest_comments_draft_or_post_title($post = 0)
    {
    }
    /**
     * Renders the `core/latest-comments` block on server.
     *
     * @since 5.1.0
     *
     * @param array $attributes The block attributes.
     *
     * @return string Returns the post content with latest comments added.
     * @phpstan-return non-falsy-string
     */
    function render_block_core_latest_comments($attributes)
    {
    }
    /**
     * Renders the `core/latest-posts` block on server.
     *
     * @since 5.0.0
     *
     * @global WP_Post $post                                   Global post object.
     * @global int     $block_core_latest_posts_excerpt_length Excerpt length set by the Latest Posts core block.
     *
     * @param array $attributes The block attributes.
     *
     * @return string Returns the post content with latest posts added.
     * @phpstan-return non-falsy-string
     */
    function render_block_core_latest_posts($attributes)
    {
    }
    /**
     * Renders the `core/loginout` block on server.
     *
     * @since 5.8.0
     *
     * @param array $attributes The block attributes.
     *
     * @return string Returns the login-out link or form.
     * @phpstan-return non-falsy-string
     */
    function render_block_core_loginout($attributes)
    {
    }
    /**
     * Build an array with CSS classes and inline styles defining the colors
     * which will be applied to the navigation markup in the front-end.
     *
     * @since 5.9.0
     *
     * @param  array $context     Navigation block context.
     * @param  array $attributes  Block attributes.
     * @param  bool  $is_sub_menu Whether the link is part of a sub-menu. Default false.
     * @return array Colors CSS classes and inline styles.
     * @phpstan-return array{css_classes: list<string>, inline_styles: string}
     */
    function block_core_navigation_link_build_css_colors($context, $attributes, $is_sub_menu = \false)
    {
    }
    /**
     * Build an array with CSS classes and inline styles defining the font sizes
     * which will be applied to the navigation markup in the front-end.
     *
     * This function is no longer used internally and is kept only for backward
     * compatibility with third-party code that may call it directly.
     *
     * @since 5.9.0
     * @deprecated 7.0.0
     *
     * @param  array $context Navigation block context.
     * @return array Font size CSS classes and inline styles.
     * @phpstan-return array{css_classes: list<string>, inline_styles: string}
     */
    function block_core_navigation_link_build_css_font_sizes($context)
    {
    }
    /**
     * Returns the submenu SVG chevron icon.
     *
     * @since 5.9.0
     *
     * @return string
     * @phpstan-return non-falsy-string
     */
    function block_core_shared_navigation_render_submenu_icon()
    {
    }
    /**
     * Build an array with CSS classes and inline styles defining the colors
     * which will be applied to the navigation markup in the front-end.
     *
     * @since 5.9.0
     *
     * @param array $attributes Navigation block attributes.
     *
     * @return array Colors CSS classes and inline styles.
     * @phpstan-return array{css_classes: list<string>, inline_styles: string, overlay_css_classes: list<string>, overlay_inline_styles: string}
     */
    function block_core_navigation_build_css_colors($attributes)
    {
    }
    /**
     * Build an array with CSS classes and inline styles defining the font sizes
     * which will be applied to the navigation markup in the front-end.
     *
     * @since 5.9.0
     *
     * @param array $attributes Navigation block attributes.
     *
     * @return array Font size CSS classes and inline styles.
     * @phpstan-return array{css_classes: list<string>, inline_styles: string}
     */
    function block_core_navigation_build_css_font_sizes($attributes)
    {
    }
    /**
     * Build an array with CSS classes and inline styles defining the colors
     * which will be applied to the pages markup in the front-end when it is a descendant of navigation.
     *
     * @since 5.8.0
     *
     * @param  array $attributes Block attributes.
     * @param  array $context    Navigation block context.
     * @return array Colors CSS classes and inline styles.
     * @phpstan-return array{css_classes: list<string>, inline_styles: string, overlay_css_classes: list<string>, overlay_inline_styles: string}
     */
    function block_core_page_list_build_css_colors($attributes, $context)
    {
    }
    /**
     * Counts words or characters in a provided text string.
     *
     * This function currently employs an array of regular expressions
     * to parse HTML and count words, which may result in inaccurate
     * word counts. However, it is designed primarily to agree with the
     * corresponding JavaScript function.
     *
     * Any improvements in the word counting, for example with the HTML API
     * and {@see \IntlBreakIterator::createWordInstance()} should coordinate
     * with changes to the JavaScript implementation to ensure consistency
     * between the editor and the rendered page.
     *
     * @since 6.9.0
     *
     * @param string $text Text to count elements in.
     * @param string $type The type of count. Accepts 'words', 'characters_excluding_spaces', or 'characters_including_spaces'.
     *
     * @return string The rendered word count.
     * @phpstan-param 'words'|'characters_excluding_spaces'|'characters_including_spaces' $type
     * @phpstan-return int<0, max>
     */
    function block_core_post_time_to_read_word_count($text, $type)
    {
    }
    /**
     * Renders the `query-total` block on the server.
     *
     * @since 6.8.0
     *
     * @global WP_Query $wp_query WordPress Query object.
     *
     * @param array    $attributes Block attributes.
     * @param string   $content    Block default content.
     * @param WP_Block $block      Block instance.
     *
     * @return string The rendered block content.
     * @phpstan-return non-falsy-string
     */
    function render_block_core_query_total($attributes, $content, $block)
    {
    }
    /**
     * Renders the `core/rss` block on server.
     *
     * @since 5.2.0
     *
     * @param array $attributes The block attributes.
     *
     * @return string Returns the block content with received rss items.
     * @phpstan-return non-falsy-string
     */
    function render_block_core_rss($attributes)
    {
    }
    /**
     * Dynamically renders the `core/search` block.
     *
     * @since 6.3.0 Using block.json `viewScript` to register script, and update `view_script_handles()` only when needed.
     *
     * @param array $attributes The block attributes.
     *
     * @return string The search block markup.
     * @phpstan-return non-falsy-string
     */
    function render_block_core_search($attributes)
    {
    }
    /**
     * Renders the `core/site-logo` block on the server.
     *
     * @since 5.8.0
     *
     * @param array $attributes The block attributes.
     *
     * @return string The render.
     * @phpstan-return non-falsy-string
     */
    function render_block_core_site_logo($attributes)
    {
    }
    /**
     * Renders the 'core/widget-group' block.
     *
     * @since 5.9.0
     *
     * @global array      $wp_registered_sidebars
     * @global int|string $_sidebar_being_rendered
     *
     * @param array    $attributes The block attributes.
     * @param string   $content The block content.
     * @param WP_Block $block The block.
     *
     * @return string Rendered block.
     * @phpstan-return non-falsy-string
     */
    function render_block_core_widget_group($attributes, $content, $block)
    {
    }
    /**
     * Retrieves or echoes all of the bookmarks.
     *
     * List of default arguments are as follows:
     *
     * These options define how the Category name will appear before the category
     * links are displayed, if 'categorize' is 1. If 'categorize' is 0, then it will
     * display for only the 'title_li' string and only if 'title_li' is not empty.
     *
     * @since 2.1.0
     *
     * @see _walk_bookmarks()
     *
     * @param string|array $args {
     *     Optional. String or array of arguments to list bookmarks.
     *
     *     @type string       $orderby          How to order the links by. Accepts post fields. Default 'name'.
     *     @type string       $order            Whether to order bookmarks in ascending or descending order.
     *                                          Accepts 'ASC' (ascending) or 'DESC' (descending). Default 'ASC'.
     *     @type int          $limit            Amount of bookmarks to display. Accepts 1+ or -1 for all.
     *                                          Default -1.
     *     @type string       $category         Comma-separated list of category IDs to include links from.
     *                                          Default empty.
     *     @type string       $category_name    Category to retrieve links for by name. Default empty.
     *     @type int|bool     $hide_invisible   Whether to show or hide links marked as 'invisible'. Accepts
     *                                          1|true or 0|false. Default 1|true.
     *     @type int|bool     $show_updated     Whether to display the time the bookmark was last updated.
     *                                          Accepts 1|true or 0|false. Default 0|false.
     *     @type int|bool     $echo             Whether to echo or return the formatted bookmarks. Accepts
     *                                          1|true (echo) or 0|false (return). Default 1|true.
     *     @type int|bool     $categorize       Whether to show links listed by category or in a single column.
     *                                          Accepts 1|true (by category) or 0|false (one column). Default 1|true.
     *     @type int|bool     $show_description Whether to show the bookmark descriptions. Accepts 1|true or 0|false.
     *                                          Default 0|false.
     *     @type string       $title_li         What to show before the links appear. Default 'Bookmarks'.
     *     @type string       $title_before     The HTML or text to prepend to the $title_li string. Default '<h2>'.
     *     @type string       $title_after      The HTML or text to append to the $title_li string. Default '</h2>'.
     *     @type string|array $class            The CSS class or an array of classes to use for the $title_li.
     *                                          Default 'linkcat'.
     *     @type string       $category_before  The HTML or text to prepend to $title_before if $categorize is true.
     *                                          String must contain '%id' and '%class' to inherit the category ID and
     *                                          the $class argument used for formatting in themes.
     *                                          Default '<li id="%id" class="%class">'.
     *     @type string       $category_after   The HTML or text to append to $title_after if $categorize is true.
     *                                          Default '</li>'.
     *     @type string       $category_orderby How to order the bookmark category based on term scheme if $categorize
     *                                          is true. Default 'name'.
     *     @type string       $category_order   Whether to order categories in ascending or descending order if
     *                                          $categorize is true. Accepts 'ASC' (ascending) or 'DESC' (descending).
     *                                          Default 'ASC'.
     * }
     * @return void|string Void if 'echo' argument is true, HTML list of bookmarks if 'echo' is false.
     * @phpstan-param array{
     *   orderby?: string,
     *   order?: string,
     *   limit?: int,
     *   category?: string,
     *   category_name?: string,
     *   hide_invisible?: int|bool,
     *   show_updated?: int|bool,
     *   echo?: int|bool,
     *   categorize?: int|bool,
     *   show_description?: int|bool,
     *   title_li?: string,
     *   title_before?: string,
     *   title_after?: string,
     *   class?: string|array,
     *   category_before?: string,
     *   category_after?: string,
     *   category_orderby?: string,
     *   category_order?: string,
     * } $args
     * @phpstan-return ($args is array{echo: false|0, ...} ? string : void)
     */
    function wp_list_bookmarks($args = '')
    {
    }
    /**
     * Retrieves bookmark data.
     *
     * @since 2.1.0
     *
     * @global object $link Current link object.
     * @global wpdb   $wpdb WordPress database abstraction object.
     *
     * @param int|stdClass $bookmark
     * @param string       $output   Optional. The required return type. One of OBJECT, ARRAY_A, or ARRAY_N, which
     *                               correspond to an stdClass object, an associative array, or a numeric array,
     *                               respectively. Default OBJECT.
     * @param string       $filter   Optional. How to sanitize bookmark fields. Default 'raw'.
     * @return array|object|null Type returned depends on $output value.
     * @phpstan-param 'OBJECT'|'ARRAY_A'|'ARRAY_N' $output
     * @phpstan-return null|($output is 'ARRAY_A' ? array<string, mixed> : ($output is 'ARRAY_N' ? array<int, mixed> : \stdClass))
     */
    function get_bookmark($bookmark, $output = \OBJECT, $filter = 'raw')
    {
    }
    /**
     * Retrieves single bookmark data item or field.
     *
     * @since 2.3.0
     *
     * @param string $field    The name of the data field to return.
     * @param int    $bookmark The bookmark ID to get field.
     * @param string $context  Optional. The context of how the field will be used. Default 'display'.
     * @return string|WP_Error
     * @phpstan-param 'link_id'|'link_url'|'link_name'|'link_image'|'link_target'|'link_description'|'link_visible'|'link_owner'|'link_rating'|'link_updated'|'link_rel'|'link_notes'|'link_rss'|'link_category' $field
     * @phpstan-return array<int, int<1, max>>|int|string|\WP_Error
     */
    function get_bookmark_field($field, $bookmark, $context = 'display')
    {
    }
    /**
     * Sanitizes a bookmark field.
     *
     * Sanitizes the bookmark fields based on what the field name is. If the field
     * has a strict value set, then it will be tested for that, else a more generic
     * filtering is applied. After the more strict filter is applied, if the `$context`
     * is 'raw' then the value is immediately return.
     *
     * Hooks exist for the more generic cases. With the 'edit' context, the {@see 'edit_$field'}
     * filter will be called and passed the `$value` and `$bookmark_id` respectively.
     *
     * With the 'db' context, the {@see 'pre_$field'} filter is called and passed the value.
     * The 'display' context is the final context and has the `$field` has the filter name
     * and is passed the `$value`, `$bookmark_id`, and `$context`, respectively.
     *
     * @since 2.3.0
     *
     * @param string $field       The bookmark field.
     * @param mixed  $value       The bookmark field value.
     * @param int    $bookmark_id Bookmark ID.
     * @param string $context     How to filter the field value. Accepts 'raw', 'edit', 'db',
     *                            'display', 'attribute', or 'js'. Default 'display'.
     * @return mixed The filtered value.
     * @phpstan-param 'raw'|'edit'|'db'|'display'|'attribute'|'js' $context
     * @phpstan-param 'link_id'|'link_url'|'link_name'|'link_image'|'link_target'|'link_description'|'link_visible'|'link_owner'|'link_rating'|'link_updated'|'link_rel'|'link_notes'|'link_rss'|'link_category' $field
     * @phpstan-return array<int, int>|int|string
     */
    function sanitize_bookmark_field($field, $value, $bookmark_id, $context)
    {
    }
    /**
     * Displays or retrieves the HTML list of categories.
     *
     * @since 2.1.0
     * @since 4.4.0 Introduced the `hide_title_if_empty` and `separator` arguments.
     * @since 4.4.0 The `current_category` argument was modified to optionally accept an array of values.
     * @since 6.1.0 Default value of the 'use_desc_for_title' argument was changed from 1 to 0.
     *
     * @param array|string $args {
     *     Array of optional arguments. See get_categories(), get_terms(), and WP_Term_Query::__construct()
     *     for information on additional accepted arguments.
     *
     *     @type int|int[]    $current_category      ID of category, or array of IDs of categories, that should get the
     *                                               'current-cat' class. Default 0.
     *     @type int          $depth                 Category depth. Used for tab indentation. Default 0.
     *     @type bool|int     $echo                  Whether to echo or return the generated markup. Accepts 0, 1, or their
     *                                               bool equivalents. Default 1.
     *     @type int[]|string $exclude               Array or comma/space-separated string of term IDs to exclude.
     *                                               If `$hierarchical` is true, descendants of `$exclude` terms will also
     *                                               be excluded; see `$exclude_tree`. See get_terms().
     *                                               Default empty string.
     *     @type int[]|string $exclude_tree          Array or comma/space-separated string of term IDs to exclude, along
     *                                               with their descendants. See get_terms(). Default empty string.
     *     @type string       $feed                  Text to use for the feed link. Default 'Feed for all posts filed
     *                                               under [cat name]'.
     *     @type string       $feed_image            URL of an image to use for the feed link. Default empty string.
     *     @type string       $feed_type             Feed type. Used to build feed link. See get_term_feed_link().
     *                                               Default empty string (default feed).
     *     @type bool         $hide_title_if_empty   Whether to hide the `$title_li` element if there are no terms in
     *                                               the list. Default false (title will always be shown).
     *     @type string       $separator             Separator between links. Default '<br />'.
     *     @type bool|int     $show_count            Whether to include post counts. Accepts 0, 1, or their bool equivalents.
     *                                               Default 0.
     *     @type string       $show_option_all       Text to display for showing all categories. Default empty string.
     *     @type string       $show_option_none      Text to display for the 'no categories' option.
     *                                               Default 'No categories'.
     *     @type string       $style                 The style used to display the categories list. If 'list', categories
     *                                               will be output as an unordered list. If left empty or another value,
     *                                               categories will be output separated by `<br>` tags. Default 'list'.
     *     @type string       $taxonomy              Name of the taxonomy to retrieve. Default 'category'.
     *     @type string       $title_li              Text to use for the list title `<li>` element. Pass an empty string
     *                                               to disable. Default 'Categories'.
     *     @type bool|int     $use_desc_for_title    Whether to use the category description as the title attribute.
     *                                               Accepts 0, 1, or their bool equivalents. Default 0.
     *     @type Walker       $walker                Walker object to use to build the output. Default empty which results
     *                                               in a Walker_Category instance being used.
     * }
     * @return void|string|false Void if 'echo' argument is true, HTML list of categories if 'echo' is false.
     *                           False if the taxonomy does not exist.
     * @phpstan-param array{
     *   current_category?: int|int[],
     *   depth?: int,
     *   echo?: bool|int,
     *   exclude?: int[]|string,
     *   exclude_tree?: int[]|string,
     *   feed?: string,
     *   feed_image?: string,
     *   feed_type?: string,
     *   hide_title_if_empty?: bool,
     *   separator?: string,
     *   show_count?: bool|int,
     *   show_option_all?: string,
     *   show_option_none?: string,
     *   style?: string,
     *   taxonomy?: string,
     *   title_li?: string,
     *   use_desc_for_title?: bool|int,
     *   walker?: Walker,
     * } $args
     * @phpstan-return ($args is array{echo: false|0, ...} ? string|false : false|void)
     */
    function wp_list_categories($args = '')
    {
    }
    /**
     * Displays a tag cloud.
     *
     * Outputs a list of tags in what is called a 'tag cloud', where the size of each tag
     * is determined by how many times that particular tag has been assigned to posts.
     *
     * @since 2.3.0
     * @since 2.8.0 Added the `taxonomy` argument.
     * @since 4.8.0 Added the `show_count` argument.
     *
     * @param array|string $args {
     *     Optional. Array or string of arguments for displaying a tag cloud. See wp_generate_tag_cloud()
     *     and get_terms() for the full lists of arguments that can be passed in `$args`.
     *
     *     @type int    $number    The number of tags to display. Accepts any positive integer
     *                             or zero to return all. Default 45.
     *     @type string $link      Whether to display term editing links or term permalinks.
     *                             Accepts 'edit' and 'view'. Default 'view'.
     *     @type string $post_type The post type. Used to highlight the proper post type menu
     *                             on the linked edit page. Defaults to the first post type
     *                             associated with the taxonomy.
     *     @type bool   $echo      Whether or not to echo the return value. Default true.
     * }
     * @return void|string|string[] Void if 'echo' argument is true, or on failure. Otherwise, tag cloud
     *                              as a string or an array, depending on 'format' argument.
     * @phpstan-param array{
     *   number?: int,
     *   link?: string,
     *   post_type?: string,
     *   echo?: bool,
     *   smallest?: int,
     *   largest?: int,
     *   unit?: string,
     *   format?: string,
     *   separator?: string,
     *   orderby?: string,
     *   order?: string,
     *   filter?: int|bool,
     *   topic_count_text?: array,
     *   topic_count_text_callback?: callable,
     *   topic_count_scale_callback?: callable,
     *   show_count?: bool|int,
     * } $args
     * @phpstan-return ($args is array{format: 'array', ...} ? array<int, string>|void : ($args is array{echo: false|0, ...} ? string|void : void))
     */
    function wp_tag_cloud($args = '')
    {
    }
    /**
     * Generates a tag cloud (heatmap) from provided data.
     *
     * @todo Complete functionality.
     * @since 2.3.0
     * @since 4.8.0 Added the `show_count` argument.
     *
     * @param WP_Term[]    $tags Array of WP_Term objects to generate the tag cloud for.
     * @param string|array $args {
     *     Optional. Array or string of arguments for generating a tag cloud.
     *
     *     @type int      $smallest                   Smallest font size used to display tags. Paired
     *                                                with the value of `$unit`, to determine CSS text
     *                                                size unit. Default 8 (pt).
     *     @type int      $largest                    Largest font size used to display tags. Paired
     *                                                with the value of `$unit`, to determine CSS text
     *                                                size unit. Default 22 (pt).
     *     @type string   $unit                       CSS text size unit to use with the `$smallest`
     *                                                and `$largest` values. Accepts any valid CSS text
     *                                                size unit. Default 'pt'.
     *     @type int      $number                     The number of tags to return. Accepts any
     *                                                positive integer or zero to return all.
     *                                                Default 0.
     *     @type string   $format                     Format to display the tag cloud in. Accepts 'flat'
     *                                                (tags separated with spaces), 'list' (tags displayed
     *                                                in an unordered list), or 'array' (returns an array).
     *                                                Default 'flat'.
     *     @type string   $separator                  HTML or text to separate the tags. Default "\n" (newline).
     *     @type string   $orderby                    Value to order tags by. Accepts 'name' or 'count'.
     *                                                Default 'name'. The {@see 'tag_cloud_sort'} filter
     *                                                can also affect how tags are sorted.
     *     @type string   $order                      How to order the tags. Accepts 'ASC' (ascending),
     *                                                'DESC' (descending), or 'RAND' (random). Default 'ASC'.
     *     @type int|bool $filter                     Whether to enable filtering of the final output
     *                                                via {@see 'wp_generate_tag_cloud'}. Default 1.
     *     @type array    $topic_count_text           Nooped plural text from _n_noop() to supply to
     *                                                tag counts. Default null.
     *     @type callable $topic_count_text_callback  Callback used to generate nooped plural text for
     *                                                tag counts based on the count. Default null.
     *     @type callable $topic_count_scale_callback Callback used to determine the tag count scaling
     *                                                value. Default default_topic_count_scale().
     *     @type bool|int $show_count                 Whether to display the tag counts. Default 0. Accepts
     *                                                0, 1, or their bool equivalents.
     * }
     * @return string|string[] Tag cloud as a string or an array, depending on 'format' argument.
     * @phpstan-param array{
     *   smallest?: int,
     *   largest?: int,
     *   unit?: string,
     *   number?: int,
     *   format?: string,
     *   separator?: string,
     *   orderby?: string,
     *   order?: string,
     *   filter?: int|bool,
     *   topic_count_text?: array,
     *   topic_count_text_callback?: callable,
     *   topic_count_scale_callback?: callable,
     *   show_count?: bool|int,
     * } $args
     * @phpstan-return ($args is array{format: 'array', ...} ? array<int, string> : string)
     */
    function wp_generate_tag_cloud($tags, $args = '')
    {
    }
    /**
     * Retrieves a list of category objects.
     *
     * If you set the 'taxonomy' argument to 'link_category', the link categories
     * will be returned instead.
     *
     * @since 2.1.0
     *
     * @see get_terms() Type of arguments that can be changed.
     *
     * @param string|array $args {
     *     Optional. Arguments to retrieve categories. See get_terms() for additional options.
     *
     *     @type string $taxonomy Taxonomy to retrieve terms for. Default 'category'.
     * }
     * @return array List of category objects.
     * @phpstan-param array{
     *   taxonomy?: string,
     * } $args
     * @phpstan-param array{taxonomy?:string,...} $args
     * @phpstan-return ($args is array{fields: 'count', ...} ? list<numeric-string> : ($args is array{fields: 'names'|'slugs', ...} ? list<string> : ($args is array{fields: 'id=>name'|'id=>slug', ...} ? array<int, string> : ($args is array{fields: 'id=>parent', ...} ? array<int, int> : ($args is array{fields: 'ids'|'tt_ids', ...} ? list<int> : array<int, \WP_Term>)))))
     */
    function get_categories($args = '')
    {
    }
    /**
     * Retrieves category data given a category ID or category object.
     *
     * If you pass the $category parameter an object, which is assumed to be the
     * category row object retrieved the database. It will cache the category data.
     *
     * If you pass $category an integer of the category ID, then that category will
     * be retrieved from the database, if it isn't already cached, and pass it back.
     *
     * If you look at get_term(), then both types will be passed through several
     * filters and finally sanitized based on the $filter parameter value.
     *
     * @since 1.5.1
     *
     * @param int|object $category Category ID or category row object.
     * @param string     $output   Optional. The required return type. One of OBJECT, ARRAY_A, or ARRAY_N, which
     *                             correspond to a WP_Term object, an associative array, or a numeric array,
     *                             respectively. Default OBJECT.
     * @param string     $filter   Optional. How to sanitize category fields. Default 'raw'.
     * @return WP_Term|array|WP_Error|null Category data in type defined by $output parameter.
     *                                     Returns a WP_Term object with backwards compatible property aliases filled in.
     *                                     WP_Error if $category is empty, null if it does not exist.
     * @phpstan-param 'OBJECT'|'ARRAY_A'|'ARRAY_N' $output
     * @phpstan-return ($category is object ? array<array-key, mixed>|\WP_Term : array<array-key, mixed>|\WP_Term|\WP_Error|null) & ($output is 'ARRAY_A' ? array<string, mixed>|\WP_Error|null : ($output is 'ARRAY_N' ? array<int, mixed>|\WP_Error|null : \WP_Term|\WP_Error|null))
     */
    function get_category($category, $output = \OBJECT, $filter = 'raw')
    {
    }
    /**
     * Retrieves a category based on URL containing the category slug.
     *
     * Breaks the $category_path parameter up to get the category slug.
     *
     * Tries to find the child path and will return it. If it doesn't find a
     * match, then it will return the first category matching slug, if $full_match,
     * is set to false. If it does not, then it will return null.
     *
     * It is also possible that it will return a WP_Error object on failure. Check
     * for it when using this function.
     *
     * @since 2.1.0
     *
     * @param string $category_path URL containing category slugs.
     * @param bool   $full_match    Optional. Whether full path should be matched.
     * @param string $output        Optional. The required return type. One of OBJECT, ARRAY_A, or ARRAY_N, which
     *                              correspond to a WP_Term object, an associative array, or a numeric array,
     *                              respectively. Default OBJECT.
     * @return WP_Term|array|WP_Error|null Type is based on $output value.
     * @phpstan-param 'OBJECT'|'ARRAY_A'|'ARRAY_N' $output
     * @phpstan-return ($output is 'ARRAY_A' ? array<string, mixed>|\WP_Error|null : ($output is 'ARRAY_N' ? array<int, mixed>|\WP_Error|null : \WP_Term|\WP_Error|null))
     */
    function get_category_by_path($category_path, $full_match = \true, $output = \OBJECT)
    {
    }
    /**
     * Retrieves all post tags.
     *
     * @since 2.3.0
     *
     * @param string|array $args {
     *     Optional. Arguments to retrieve tags. See get_terms() for additional options.
     *
     *     @type string $taxonomy Taxonomy to retrieve terms for. Default 'post_tag'.
     * }
     * @return WP_Term[]|int|WP_Error Array of 'post_tag' term objects, a count thereof,
     *                                or WP_Error if any of the taxonomies do not exist.
     * @phpstan-param array{
     *   taxonomy?: string,
     * } $args
     * @phpstan-return ($args is array{fields: 'count', ...} ? numeric-string : ($args is array{fields: 'names'|'slugs', ...} ? list<string> : ($args is array{fields: 'id=>name'|'id=>slug', ...} ? array<int, string> : ($args is array{fields: 'id=>parent', ...} ? array<int, int> : ($args is array{fields: 'ids'|'tt_ids', ...} ? list<int> : array<int, \WP_Term>)))))|\WP_Error
     */
    function get_tags($args = '')
    {
    }
    /**
     * Generates a string of attributes by applying to the current block being
     * rendered all of the features that the block supports.
     *
     * @since 5.6.0
     *
     * @param string[] $extra_attributes Optional. Array of extra attributes to render on the block wrapper.
     * @return string String of HTML attributes.
     * @phpstan-param array<string, string> $extra_attributes
     * @phpstan-return ($extra_attributes is empty ? string : non-falsy-string)
     */
    function get_block_wrapper_attributes($extra_attributes = array())
    {
    }
    /**
     * Generates semantic classes for each comment element.
     *
     * @since 2.7.0
     * @since 4.4.0 Added the ability for `$comment` to also accept a WP_Comment object.
     *
     * @param string|string[] $css_class Optional. One or more classes to add to the class list.
     *                                   Default empty.
     * @param int|WP_Comment  $comment   Optional. Comment ID or WP_Comment object. Default current comment.
     * @param int|WP_Post     $post      Optional. Post ID or WP_Post object. Default current post.
     * @param bool            $display   Optional. Whether to print or return the output.
     *                                   Default true.
     * @return void|string Void if `$display` argument is true, comment classes if `$display` is false.
     * @phpstan-return ($display is true ? void : string)
     */
    function comment_class($css_class = '', $comment = \null, $post = \null, $display = \true)
    {
    }
    /**
     * Retrieves the status of a comment by comment ID.
     *
     * @since 1.0.0
     *
     * @param int|WP_Comment $comment_id Comment ID or WP_Comment object
     * @return string|false Status might be 'trash', 'approved', 'unapproved', 'spam'. False on failure.
     * @phpstan-return 'approved'|'spam'|'trash'|'unapproved'|false
     */
    function wp_get_comment_status($comment_id)
    {
    }
    /**
     * Sets the status of a comment.
     *
     * The {@see 'wp_set_comment_status'} action is called after the comment is handled.
     * If the comment status is not in the list, then false is returned.
     *
     * @since 1.0.0
     *
     * @global wpdb $wpdb WordPress database abstraction object.
     *
     * @param int|WP_Comment $comment_id     Comment ID or WP_Comment object.
     * @param string         $comment_status New comment status, either 'hold', 'approve', 'spam', or 'trash'.
     * @param bool           $wp_error       Whether to return a WP_Error object if there is a failure. Default false.
     * @return bool|WP_Error True on success, false or WP_Error on failure.
     * @phpstan-param 'hold'|'approve'|'spam'|'trash' $comment_status
     * @phpstan-return ($wp_error is false ? bool : true|\WP_Error)
     */
    function wp_set_comment_status($comment_id, $comment_status, $wp_error = \false)
    {
    }
    /**
     * Updates an existing comment in the database.
     *
     * Filters the comment and makes sure certain fields are valid before updating.
     *
     * @since 2.0.0
     * @since 4.9.0 Add updating comment meta during comment update.
     * @since 5.5.0 The `$wp_error` parameter was added.
     * @since 5.5.0 The return values for an invalid comment or post ID
     *              were changed to false instead of 0.
     *
     * @global wpdb $wpdb WordPress database abstraction object.
     *
     * @param array $commentarr Contains information on the comment.
     * @param bool  $wp_error   Optional. Whether to return a WP_Error on failure. Default false.
     * @return int|false|WP_Error The value 1 if the comment was updated, 0 if not updated.
     *                            False or a WP_Error object on failure.
     * @phpstan-return ($wp_error is false ? 0|1|false : 0|1|\WP_Error)
     */
    function wp_update_comment($commentarr, $wp_error = \false)
    {
    }
    /**
     * Schedules an event to run only once.
     *
     * Schedules a hook which will be triggered by WordPress at the specified UTC time.
     * The action will trigger when someone visits your WordPress site if the scheduled
     * time has passed.
     *
     * Note that scheduling an event to occur within 10 minutes of an existing event
     * with the same action hook will be ignored unless you pass unique `$args` values
     * for each scheduled event.
     *
     * Use wp_next_scheduled() to prevent duplicate events.
     *
     * Use wp_schedule_event() to schedule a recurring event.
     *
     * @since 2.1.0
     * @since 5.1.0 Return value modified to boolean indicating success or failure,
     *              {@see 'pre_schedule_event'} filter added to short-circuit the function.
     * @since 5.7.0 The `$wp_error` parameter was added.
     *
     * @link https://developer.wordpress.org/reference/functions/wp_schedule_single_event/
     *
     * @param int    $timestamp  Unix timestamp (UTC) for when to next run the event.
     * @param string $hook       Action hook to execute when the event is run.
     * @param array  $args       Optional. Array containing arguments to pass to the
     *                           hook's callback function. Each value in the array
     *                           is passed to the callback as an individual parameter.
     *                           The array keys are ignored. Default empty array.
     *
     *                           These arguments are used to uniquely identify the
     *                           scheduled event and must match those used when the
     *                           event was originally scheduled. If the arguments
     *                           do not match exactly, WordPress will treat the
     *                           event as different, which can lead to duplicate
     *                           cron events being scheduled unintentionally,
     *                           excessive growth of the 'cron' option, and
     *                           database performance issues.
     * @param bool   $wp_error   Optional. Whether to return a WP_Error on failure. Default false.
     * @return bool|WP_Error True if event successfully scheduled. False or WP_Error on failure.
     * @phpstan-param list<mixed> $args
     * @phpstan-return ($wp_error is false ? bool : true|\WP_Error)
     */
    function wp_schedule_single_event($timestamp, $hook, $args = array(), $wp_error = \false)
    {
    }
    /**
     * Schedules a recurring event.
     *
     * Schedules a hook which will be triggered by WordPress at the specified interval.
     * The action will trigger when someone visits your WordPress site if the scheduled
     * time has passed.
     *
     * Valid values for the recurrence are 'hourly', 'twicedaily', 'daily', and 'weekly'.
     * These can be extended using the {@see 'cron_schedules'} filter in wp_get_schedules().
     *
     * Use wp_next_scheduled() to prevent duplicate events.
     *
     * Use wp_schedule_single_event() to schedule a non-recurring event.
     *
     * @since 2.1.0
     * @since 5.1.0 Return value modified to boolean indicating success or failure,
     *              {@see 'pre_schedule_event'} filter added to short-circuit the function.
     * @since 5.7.0 The `$wp_error` parameter was added.
     *
     * @link https://developer.wordpress.org/reference/functions/wp_schedule_event/
     *
     * @param int    $timestamp  Unix timestamp (UTC) for when to next run the event.
     * @param string $recurrence How often the event should subsequently recur.
     *                           See wp_get_schedules() for accepted values.
     * @param string $hook       Action hook to execute when the event is run.
     * @param array  $args       Optional. Array containing arguments to pass to the
     *                           hook's callback function. Each value in the array
     *                           is passed to the callback as an individual parameter.
     *                           The array keys are ignored. Default empty array.
     *
     *                           These arguments are used to uniquely identify the
     *                           scheduled event and must match those used when the
     *                           event was originally scheduled. If the arguments
     *                           do not match exactly, WordPress will treat the
     *                           event as different, which can lead to duplicate
     *                           cron events being scheduled unintentionally,
     *                           excessive growth of the 'cron' option, and
     *                           database performance issues.
     * @param bool   $wp_error   Optional. Whether to return a WP_Error on failure. Default false.
     * @return bool|WP_Error True if event successfully scheduled. False or WP_Error on failure.
     * @phpstan-param list<mixed> $args
     * @phpstan-return ($wp_error is false ? bool : true|\WP_Error)
     */
    function wp_schedule_event($timestamp, $recurrence, $hook, $args = array(), $wp_error = \false)
    {
    }
    /**
     * Reschedules a recurring event.
     *
     * Mainly for internal use, this takes the Unix timestamp (UTC) of a previously run
     * recurring event and reschedules it for its next run.
     *
     * To change upcoming scheduled events, use wp_schedule_event() to
     * change the recurrence frequency.
     *
     * @since 2.1.0
     * @since 5.1.0 Return value modified to boolean indicating success or failure,
     *              {@see 'pre_reschedule_event'} filter added to short-circuit the function.
     * @since 5.7.0 The `$wp_error` parameter was added.
     *
     * @param int    $timestamp  Unix timestamp (UTC) for when the event was scheduled.
     * @param string $recurrence How often the event should subsequently recur.
     *                           See wp_get_schedules() for accepted values.
     * @param string $hook       Action hook to execute when the event is run.
     * @param array  $args       Optional. Array containing arguments to pass to the
     *                           hook's callback function. Each value in the array
     *                           is passed to the callback as an individual parameter.
     *                           The array keys are ignored. Default empty array.
     *
     *                           These arguments are used to uniquely identify the
     *                           scheduled event and must match those used when the
     *                           event was originally scheduled. If the arguments
     *                           do not match exactly, WordPress will treat the
     *                           event as different, which can lead to duplicate
     *                           cron events being scheduled unintentionally,
     *                           excessive growth of the 'cron' option, and
     *                           database performance issues.
     * @param bool   $wp_error   Optional. Whether to return a WP_Error on failure. Default false.
     * @return bool|WP_Error True if event successfully rescheduled. False or WP_Error on failure.
     * @phpstan-param list<mixed> $args
     * @phpstan-return ($wp_error is false ? bool : true|\WP_Error)
     */
    function wp_reschedule_event($timestamp, $recurrence, $hook, $args = array(), $wp_error = \false)
    {
    }
    /**
     * Unschedules a previously scheduled event.
     *
     * The `$timestamp` and `$hook` parameters are required so that the event can be
     * identified.
     *
     * @since 2.1.0
     * @since 5.1.0 Return value modified to boolean indicating success or failure,
     *              {@see 'pre_unschedule_event'} filter added to short-circuit the function.
     * @since 5.7.0 The `$wp_error` parameter was added.
     *
     * @param int    $timestamp Unix timestamp (UTC) of the event.
     * @param string $hook      Action hook of the event.
     * @param array  $args      Optional. Array containing each separate argument to pass to the hook's callback function.
     *                          Although not passed to a callback, these arguments are used to uniquely identify the
     *                          event, so they must match those used when originally scheduling the event. If the
     *                          arguments do not match exactly, the event will not be found. Default empty array.
     * @param bool   $wp_error  Optional. Whether to return a WP_Error on failure. Default false.
     * @return bool|WP_Error True if event successfully unscheduled. False or WP_Error on failure.
     * @phpstan-param list<mixed> $args
     * @phpstan-return ($wp_error is false ? bool : true|\WP_Error)
     */
    function wp_unschedule_event($timestamp, $hook, $args = array(), $wp_error = \false)
    {
    }
    /**
     * Unschedules all events attached to the hook with the specified arguments.
     *
     * Warning: This function may return boolean false, but may also return a non-boolean
     * value which evaluates to false. For information about casting to booleans see the
     * {@link https://www.php.net/manual/en/language.types.boolean.php PHP documentation}. Use
     * the `===` operator for testing the return value of this function.
     *
     * @since 2.1.0
     * @since 5.1.0 Return value modified to indicate success or failure,
     *              {@see 'pre_clear_scheduled_hook'} filter added to short-circuit the function.
     * @since 5.7.0 The `$wp_error` parameter was added.
     *
     * @param string $hook     Action hook, the execution of which will be unscheduled.
     * @param array  $args     Optional. Array containing each separate argument to pass to the hook's callback function.
     *                         Although not passed to a callback, these arguments are used to uniquely identify the
     *                         event, so they must match those used when originally scheduling the event. If the
     *                         arguments do not match exactly, the event will not be found. Default empty array.
     * @param bool   $wp_error Optional. Whether to return a WP_Error on failure. Default false.
     * @return int|false|WP_Error On success an integer indicating number of events unscheduled (0 indicates no
     *                            events were registered with the hook and arguments combination), false or WP_Error
     *                            if unscheduling one or more events fail.
     * @phpstan-param list<mixed> $args
     * @phpstan-return (int<0, max>|($wp_error is false ? false : \WP_Error))
     */
    function wp_clear_scheduled_hook($hook, $args = array(), $wp_error = \false)
    {
    }
    /**
     * Unschedules all events attached to the hook.
     *
     * Can be useful for plugins when deactivating to clean up the cron queue.
     *
     * Warning: This function may return boolean false, but may also return a non-boolean
     * value which evaluates to false. For information about casting to booleans see the
     * {@link https://www.php.net/manual/en/language.types.boolean.php PHP documentation}. Use
     * the `===` operator for testing the return value of this function.
     *
     * @since 4.9.0
     * @since 5.1.0 Return value added to indicate success or failure.
     * @since 5.7.0 The `$wp_error` parameter was added.
     *
     * @param string $hook     Action hook, the execution of which will be unscheduled.
     * @param bool   $wp_error Optional. Whether to return a WP_Error on failure. Default false.
     * @return int|false|WP_Error On success an integer indicating number of events unscheduled (0 indicates no
     *                            events were registered on the hook), false or WP_Error if unscheduling fails.
     * @phpstan-return ($wp_error is false ? int<0, max>|false : int<0, max>|\WP_Error)
     */
    function wp_unschedule_hook($hook, $wp_error = \false)
    {
    }
    /**
     * Adds slashes to a string or recursively adds slashes to strings within an array.
     *
     * This function is just a wrapper for `wp_slash()`. It was originally related to
     * magic quotes functionality which was deprecated in PHP 5.3.0 and removed in PHP 5.4.0.
     *
     * @since 0.71
     * @deprecated 7.0.0 Use wp_slash() instead.
     * @see wp_slash()
     *
     * @param string|array $gpc String or array of data to slash.
     * @return string|array Slashed `$gpc`.
     * @phpstan-pure
     * @phpstan-return ($gpc is string ? string : array)
     */
    function addslashes_gpc($gpc)
    {
    }
    /**
     * Determines the type of a string of data with the data formatted.
     *
     * Tell whether the type is text, HTML, or XHTML, per RFC 4287 section 3.1.
     *
     * In the case of WordPress, text is defined as containing no markup,
     * XHTML is defined as "well formed", and HTML as tag soup (i.e., the rest).
     *
     * Container div tags are added to XHTML values, per section 3.1.1.3.
     *
     * @link http://www.atomenabled.org/developers/syndication/atom-format-spec.php#rfc.section.3.1
     *
     * @since 2.5.0
     *
     * @param string $data Input string.
     * @return array array(type, value)
     * @phpstan-return array{'html'|'text'|'xhtml', string}
     */
    function prep_atom_text_construct($data)
    {
    }
    /**
     * Separates HTML elements and comments from the text.
     *
     * @since 4.2.4
     *
     * @param string $input The text which has to be formatted.
     * @return string[] Array of the formatted text.
     * @phpstan-return non-empty-list<string>
     */
    function wp_html_split($input)
    {
    }
    /**
     * Retrieves the regular expression for an HTML element.
     *
     * @since 4.4.0
     *
     * @return string The regular expression.
     * @phpstan-pure
     * @phpstan-return non-falsy-string
     */
    function get_html_split_regex()
    {
    }
    /**
     * Sanitizes a title, replacing whitespace and a few other characters with dashes.
     *
     * Limits the output to alphanumeric characters, underscore (_) and dash (-).
     * Whitespace becomes a dash.
     *
     * @since 1.2.0
     *
     * @param string $title     The title to be sanitized.
     * @param string $raw_title Optional. Not used. Default empty.
     * @param string $context   Optional. The operation for which the string is sanitized.
     *                          When set to 'save', additional entities are converted to hyphens
     *                          or stripped entirely. Default 'display'.
     * @return string The sanitized title.
     * @phpstan-param 'display'|'save' $context
     * @phpstan-return lowercase-string
     */
    function sanitize_title_with_dashes($title, $raw_title = '', $context = 'display')
    {
    }
    /**
     * Ensures a string is a valid SQL 'order by' clause.
     *
     * Accepts one or more columns, with or without a sort order (ASC / DESC).
     * e.g. 'column_1', 'column_1, column_2', 'column_1 ASC, column_2 DESC' etc.
     *
     * Also accepts 'RAND()'.
     *
     * @since 2.5.1
     *
     * @param string $orderby Order by clause to be validated.
     * @return string|false Returns $orderby if valid, false otherwise.
     * @phpstan-template T of string
     * @phpstan-param T $orderby
     * @phpstan-return (T is non-falsy-string ? T|false : false)
     */
    function sanitize_sql_orderby($orderby)
    {
    }
    /**
     * Add leading zeros when necessary.
     *
     * If you set the threshold to '4' and the number is '10', then you will get
     * back '0010'. If you set the threshold to '4' and the number is '5000', then you
     * will get back '5000'.
     *
     * Uses sprintf to append the amount of zeros based on the $threshold parameter
     * and the size of the number. If the number is large enough, then no zeros will
     * be appended.
     *
     * @since 0.71
     *
     * @param int $number     Number to append zeros to if not greater than threshold.
     * @param int $threshold  Digit places number needs to be to not have zeros added.
     * @return string Adds leading zeros to number if needed.
     * @phpstan-param int<0, max> $threshold
     * @phpstan-return ($threshold is 0 ? lowercase-string&non-empty-string&numeric-string : ($number is int<0, max> ? lowercase-string&non-empty-string&numeric-string : lowercase-string&non-empty-string))
     */
    function zeroise($number, $threshold)
    {
    }
    /**
     * Adds backslashes before letters and before a number at the start of a string.
     *
     * @since 0.71
     *
     * @param string $value Value to which backslashes will be added.
     * @return string String with backslashes inserted.
     * @phpstan-pure
     */
    function backslashit($value)
    {
    }
    /**
     * Appends a trailing slash.
     *
     * Will remove trailing forward and backslashes if it exists already before adding
     * a trailing forward slash. This prevents double slashing a string or path.
     *
     * The primary use of this is for paths and thus should be used for paths. It is
     * not restricted to paths and offers no specific path support.
     *
     * @since 1.2.0
     *
     * @param string $value Value to which trailing slash will be added.
     * @return string String with trailing slash added.
     * @phpstan-pure
     * @phpstan-return non-falsy-string
     */
    function trailingslashit($value)
    {
    }
    /**
     * Removes trailing forward slashes and backslashes if they exist.
     *
     * The primary use of this is for paths and thus should be used for paths. It is
     * not restricted to paths and offers no specific path support.
     *
     * @since 2.2.0
     *
     * @param string $value Value from which trailing slashes will be removed.
     * @return string String without the trailing slashes.
     * @phpstan-pure
     */
    function untrailingslashit($value)
    {
    }
    /**
     * Callback function for `stripslashes_deep()` which strips slashes from strings.
     *
     * @since 4.4.0
     *
     * @param mixed $value The array or string to be stripped.
     * @return mixed The stripped value.
     *
     * @phpstan-template T
     * @phpstan-param T $value
     * @phpstan-return (T is string ? string : T)
     * @phpstan-template T
     * @phpstan-param T|string $value
     * @phpstan-pure
     * @phpstan-return ($value is string ? ($value is '' ? '' : string) : T)
     */
    function stripslashes_from_strings_only($value)
    {
    }
    /**
     * Escapes single quotes, `"`, `<`, `>`, `&`, and fixes line endings.
     *
     * Escapes text strings for echoing in JS. It is intended to be used for inline JS
     * (in a tag attribute, for example `onclick="..."`). Note that the strings have to
     * be in single quotes. The {@see 'js_escape'} filter is also applied here.
     *
     * @since 2.8.0
     *
     * @param string $text The text to be escaped.
     * @return string Escaped text.
     */
    function esc_js($text)
    {
    }
    /**
     * Escaping for HTML blocks.
     *
     * @since 2.8.0
     *
     * @param string $text
     * @return string Escaped text.
     */
    function esc_html($text)
    {
    }
    /**
     * Escaping for HTML attributes.
     *
     * @since 2.8.0
     *
     * @param string $text
     * @return string Escaped text.
     */
    function esc_attr($text)
    {
    }
    /**
     * Adds slashes to a string or recursively adds slashes to strings within an array.
     *
     * This should be used when preparing data for core API that expects slashed data.
     * This should not be used to escape data going directly into an SQL query.
     *
     * @since 3.6.0
     * @since 5.5.0 Non-string values are left untouched.
     *
     * @param string|array $value String or array of data to slash.
     * @return string|array Slashed `$value`, in the same type as supplied.
     *
     * @phpstan-template T
     * @phpstan-param T $value
     * @phpstan-return (
     *     T is string ? string : (
     *         T is array ? array<key-of<T>, ( value-of<T> is string ? string : value-of<T> )> : T
     *     )
     * )
     * @phpstan-pure
     */
    function wp_slash($value)
    {
    }
    /**
     * Converts given MySQL date string into a different format.
     *
     *  - `$format` should be a PHP date format string.
     *  - 'U' and 'G' formats will return an integer sum of timestamp with timezone offset.
     *  - `$date` is expected to be local time in MySQL format (`Y-m-d H:i:s`).
     *
     * Historically UTC time could be passed to the function to produce Unix timestamp.
     *
     * If `$translate` is true then the given date and format string will
     * be passed to `wp_date()` for translation.
     *
     * @since 0.71
     *
     * @param string $format    Format of the date to return.
     * @param string $date      Date string to convert.
     * @param bool   $translate Whether the return date should be translated. Default true.
     * @return string|int|false Integer if `$format` is 'U' or 'G', string otherwise.
     *                          False on failure.
     * @phpstan-return ($format is 'G'|'U' ? int|false : string|false)
     */
    function mysql2date($format, $date, $translate = \true)
    {
    }
    /**
     * Retrieves the current time based on specified type.
     *
     *  - The 'mysql' type will return the time in the format for MySQL DATETIME field.
     *  - The 'timestamp' or 'U' types will return the current timestamp or a sum of timestamp
     *    and timezone offset, depending on `$gmt`.
     *  - Other strings will be interpreted as PHP date formats (e.g. 'Y-m-d').
     *
     * If `$gmt` is a truthy value then both types will use GMT time, otherwise the
     * output is adjusted with the GMT offset for the site.
     *
     * @since 1.0.0
     * @since 5.3.0 Now returns an integer if `$type` is 'U'. Previously a string was returned.
     *
     * @param string $type Type of time to retrieve. Accepts 'mysql', 'timestamp', 'U',
     *                     or PHP date format string (e.g. 'Y-m-d').
     * @param bool   $gmt  Optional. Whether to use GMT timezone. Default false.
     * @return int|string Integer if `$type` is 'timestamp' or 'U', string otherwise.
     * @phpstan-return ($type is 'timestamp'|'U' ? int : string)
     */
    function current_time($type, $gmt = \false)
    {
    }
    /**
     * Converts a number of bytes to the largest unit the bytes will fit into.
     *
     * It is easier to read 1 KB than 1024 bytes and 1 MB than 1048576 bytes. Converts
     * number of bytes to human readable number by taking the number of that unit
     * that the bytes will go into it. Supports YB value.
     *
     * Please note that integers in PHP are limited to 32 bits, unless they are on
     * 64 bit architecture, then they have 64 bit size. If you need to place the
     * larger size then what PHP integer type will hold, then use a string. It will
     * be converted to a double, which should always have 64 bit length.
     *
     * Technically the correct unit names for powers of 1024 are KiB, MiB etc.
     *
     * @since 2.3.0
     * @since 6.0.0 Support for PB, EB, ZB, and YB was added.
     *
     * @param int|string $bytes    Number of bytes. Note max integer size for integers.
     * @param int        $decimals Optional. Precision of number of decimal places. Default 0.
     * @return string|false Number string on success, false on failure.
     * @phpstan-return ($bytes is not numeric ? false : ($bytes is int<min, -1>|'0' ? false : string))
     */
    function size_format($bytes, $decimals = 0)
    {
    }
    /**
     * Serializes data, if needed.
     *
     * @since 2.0.5
     *
     * @param string|array|object $data Data that might be serialized.
     * @return mixed A scalar data.
     * @phpstan-template T of mixed
     * @phpstan-param T $data
     * @phpstan-return (T is array|object|string ? string : T)
     */
    function maybe_serialize($data)
    {
    }
    /**
     * Uses RegEx to extract URLs from arbitrary content.
     *
     * @since 3.7.0
     * @since 6.0.0 Fixes support for HTML entities (Trac 30580).
     *
     * @param string $content Content to extract URLs from.
     * @return string[] Array of URLs found in passed string.
     * @phpstan-return ($content is empty ? array{} : list<string>)
     */
    function wp_extract_urls($content)
    {
    }
    /**
     * Determines whether the publish date of the current post in the loop is different
     * from the publish date of the previous post in the loop.
     *
     * For more information on this and similar theme functions, check out
     * the {@link https://developer.wordpress.org/themes/basics/conditional-tags/
     * Conditional Tags} article in the Theme Developer Handbook.
     *
     * @since 0.71
     *
     * @global string $currentday  The day of the current post in the loop.
     * @global string $previousday The day of the previous post in the loop.
     *
     * @return int 1 when new day, 0 if not a new day.
     * @phpstan-return 0|1
     */
    function is_new_day()
    {
    }
    /**
     * Determines whether input is yes or no.
     *
     * Must be 'y' to be true.
     *
     * @since 1.0.0
     *
     * @param string $yn Character string containing either 'y' (yes) or 'n' (no).
     * @return bool True if 'y', false on anything else.
     * @phpstan-pure
     * @phpstan-return ($yn is 'y' ? true : false)
     */
    function bool_from_yn($yn)
    {
    }
    /**
     * Tests if a given filesystem path is absolute.
     *
     * For example, '/foo/bar', or 'c:\windows'.
     *
     * @since 2.5.0
     *
     * @param string $path File path.
     * @return bool True if path is absolute, false is not absolute.
     * @phpstan-assert-if-true =non-falsy-string $path
     * @phpstan-return ($path is non-falsy-string ? bool : false)
     */
    function path_is_absolute($path)
    {
    }
    /**
     * Joins two filesystem paths together.
     *
     * For example, 'give me $path relative to $base'. If the $path is absolute,
     * then it the full path is returned.
     *
     * @since 2.5.0
     *
     * @param string $base Base path.
     * @param string $path Path relative to $base.
     * @return string The path with the base or absolute path.
     * @phpstan-return non-falsy-string
     */
    function path_join($base, $path)
    {
    }
    /**
     * Retrieves uploads directory information.
     *
     * Same as wp_upload_dir() but "light weight" as it doesn't attempt to create the uploads directory.
     * Intended for use in themes, when only 'basedir' and 'baseurl' are needed, generally in all cases
     * when not uploading files.
     *
     * @since 4.5.0
     *
     * @see wp_upload_dir()
     *
     * @return array See wp_upload_dir() for description.
     * @phpstan-return array{
     *                     path: non-empty-string,
     *                     url: non-empty-string,
     *                     subdir: non-empty-string,
     *                     basedir: non-empty-string,
     *                     baseurl: non-empty-string,
     *                 }
     *                |array{ error: non-empty-string }
     * @phpstan-return array{path: string, url: string, subdir: string, basedir: string, baseurl: string, error: false|string}
     */
    function wp_get_upload_dir()
    {
    }
    /**
     * Returns an array containing the current upload directory's path and URL.
     *
     * Checks the 'upload_path' option, which should be from the web root folder,
     * and if it isn't empty it will be used. If it is empty, then the path will be
     * 'WP_CONTENT_DIR/uploads'. If the 'UPLOADS' constant is defined, then it will
     * override the 'upload_path' option and 'WP_CONTENT_DIR/uploads' path.
     *
     * The upload URL path is set either by the 'upload_url_path' option or by using
     * the 'WP_CONTENT_URL' constant and appending '/uploads' to the path.
     *
     * If the 'uploads_use_yearmonth_folders' is set to true (checkbox if checked in
     * the administration settings panel), then the time will be used. The format
     * will be year first and then month.
     *
     * If the path couldn't be created, then an error will be returned with the key
     * 'error' containing the error message. The error suggests that the parent
     * directory is not writable by the server.
     *
     * @since 2.0.0
     * @uses _wp_upload_dir()
     *
     * @param string|null $time          Optional. Time formatted in 'yyyy/mm'. Default null.
     * @param bool        $create_dir    Optional. Whether to check and create the uploads directory.
     *                                   Default true for backward compatibility.
     * @param bool        $refresh_cache Optional. Whether to refresh the cache. Default false.
     * @return array {
     *     Array of information about the upload directory.
     *
     *     @type string       $path    Base directory and subdirectory or full path to upload directory.
     *     @type string       $url     Base URL and subdirectory or absolute URL to upload directory.
     *     @type string       $subdir  Subdirectory if uploads use year/month folders option is on.
     *     @type string       $basedir Path without subdir.
     *     @type string       $baseurl URL path without subdir.
     *     @type string|false $error   False or error message.
     * }
     * @phpstan-return array{
     *                     path: non-empty-string,
     *                     url: non-empty-string,
     *                     subdir: non-empty-string,
     *                     basedir: non-empty-string,
     *                     baseurl: non-empty-string,
     *                 }
     *                |array{ error: non-empty-string }
     * @phpstan-return array{path: string, url: string, subdir: string, basedir: string, baseurl: string, error: false|string}
     */
    function wp_upload_dir($time = \null, $create_dir = \true, $refresh_cache = \false)
    {
    }
    /**
     * A non-filtered, non-cached version of wp_upload_dir() that doesn't check the path.
     *
     * @since 4.5.0
     * @access private
     *
     * @param string|null $time Optional. Time formatted in 'yyyy/mm'. Default null.
     * @return array See wp_upload_dir()
     * @phpstan-return array{path: non-falsy-string, url: non-falsy-string, subdir: string, basedir: non-falsy-string, baseurl: non-falsy-string, error: false}
     */
    function _wp_upload_dir($time = \null)
    {
    }
    /**
     * Kills WordPress execution and displays HTML page with an error message.
     *
     * This function complements the `die()` PHP function. The difference is that
     * HTML will be displayed to the user. It is recommended to use this function
     * only when the execution should not continue any further. It is not recommended
     * to call this function very often, and try to handle as many errors as possible
     * silently or more gracefully.
     *
     * As a shorthand, the desired HTTP response code may be passed as an integer to
     * the `$title` parameter (the default title would apply) or the `$args` parameter.
     *
     * @since 2.0.4
     * @since 4.1.0 The `$title` and `$args` parameters were changed to optionally accept
     *              an integer to be used as the response code.
     * @since 5.1.0 The `$link_url`, `$link_text`, and `$exit` arguments were added.
     * @since 5.3.0 The `$charset` argument was added.
     * @since 5.5.0 The `$text_direction` argument has a priority over get_language_attributes()
     *              in the default handler.
     *
     * @global WP_Query $wp_query WordPress Query object.
     *
     * @param string|WP_Error|int $message Optional. Error message. If this is a WP_Error object,
     *                                     and not an Ajax or XML-RPC request, the error's messages are used.
     *                                     An integer is echoed as the entire response body by legacy Ajax
     *                                     handlers, which use -1 for a failed nonce or capability check,
     *                                     0 for failure, and 1 for success. Default empty string.
     * @param string|int          $title   Optional. Error title. If `$message` is a `WP_Error` object,
     *                                     error data with the key 'title' may be used to specify the title.
     *                                     If `$title` is an integer, then it is treated as the response code.
     *                                     Default empty string.
     * @param string|array|int $args {
     *     Optional. Arguments to control behavior. If `$args` is an integer, then it is treated
     *     as the response code. Default empty array.
     *
     *     @type int    $response       The HTTP response code. Default 200 for Ajax requests, 500 otherwise.
     *     @type string $link_url       A URL to include a link to. Only works in combination with $link_text.
     *                                  Default empty string.
     *     @type string $link_text      A label for the link to include. Only works in combination with $link_url.
     *                                  Default empty string.
     *     @type bool   $back_link      Whether to include a link to go back. Default false.
     *     @type string $text_direction The text direction. This is only useful internally, when WordPress is still
     *                                  loading and the site's locale is not set up yet. Accepts 'rtl' and 'ltr'.
     *                                  Default is the value of is_rtl().
     *     @type string $charset        Character set of the HTML output. Default 'utf-8'.
     *     @type string $code           Error code to use. Default is 'wp_die', or the main error code if $message
     *                                  is a WP_Error.
     *     @type bool   $exit           Whether to exit the process after completion. Default true.
     * }
     * @return void Never returns if `$args['exit']` is true (the default), otherwise returns void.
     * @phpstan-param string|WP_Error|int<-1, max> $message
     * @phpstan-return ( $args is array{exit: false} ? void : never )
     * @phpstan-param int|array{
     *   response?: int,
     *   link_url?: string,
     *   link_text?: string,
     *   back_link?: bool,
     *   text_direction?: string,
     *   charset?: string,
     *   code?: string,
     *   exit?: bool,
     * } $args
     * @phpstan-return ($args is array{exit: false, ...} ? void : never)
     */
    function wp_die($message = '', $title = '', $args = array())
    {
    }
    /**
     * Determines if the variable is a numeric-indexed array.
     *
     * Note! This answers a different question than {@see array_is_list()} and is
     *       more flexible to handle situations where some numeric array indices
     *       have been removed. A numeric-indexed array is only a “list” when the
     *       array keys form a contiguous range from zero to the highest key.
     *
     * Example:
     *
     *     true  === wp_is_numeric_array( array( 1, 2, 3, 4 ) );
     *     false === wp_is_numeric_array( array( 'name' => 'WordPress' ) );
     *
     *     // All-numeric keys vs. list.
     *     $above_two   = array_filter( array( 1, 2, 8, 9 ), fn ( $v ) => $v > 2 );
     *     $above_two === array( '2' => 8, '3' => 9 );
     *     true       === wp_is_numeric_array( $above_two );
     *     false      === array_is_list( $above_two );
     *
     * @since 4.4.0
     *
     * @param mixed $data Variable to check.
     * @return bool Whether the variable is a list.
     *
     * @phpstan-assert-if-true array<int, mixed> $data
     * @phpstan-pure
     * @phpstan-return ($data is array<int, mixed> ? true : false)
     */
    function wp_is_numeric_array($data): bool
    {
    }
    /**
     * Validates a file name and path against an allowed set of rules.
     *
     * A return value of `1` means the file path contains directory traversal.
     *
     * A return value of `2` means the file path contains a Windows drive path.
     *
     * A return value of `3` means the file is not in the allowed files list.
     *
     * @since 1.2.0
     *
     * @param string   $file          File path.
     * @param string[] $allowed_files Optional. Array of allowed files. Default empty array.
     * @return int 0 means nothing is wrong, greater than 0 means something was wrong.
     * @phpstan-return ($file is '' ? 0 : ($allowed_files is empty ? 0|1|2 : 0|1|2|3))
     */
    function validate_file($file, $allowed_files = array())
    {
    }
    /**
     * Returns 0.
     *
     * Useful for returning 0 to filters easily.
     *
     * @since 3.0.0
     *
     * @return int 0.
     * @phpstan-return 0
     */
    function __return_zero()
    {
    }
    /**
     * Returns an empty array.
     *
     * Useful for returning an empty array to filters easily.
     *
     * @since 3.0.0
     *
     * @return array Empty array.
     * @phpstan-return array{}
     */
    function __return_empty_array()
    {
    }
    /**
     * Returns an empty string.
     *
     * Useful for returning an empty string to filters easily.
     *
     * @since 3.7.0
     *
     * @see __return_null()
     *
     * @return string Empty string.
     * @phpstan-return ''
     */
    function __return_empty_string()
    {
    }
    /**
     * Returns a comma-separated string or array of functions that have been called to get
     * to the current point in code.
     *
     * @since 3.4.0
     *
     * @see https://core.trac.wordpress.org/ticket/19589
     *
     * @param string $ignore_class Optional. A class to ignore all function calls within - useful
     *                             when you want to just give info about the callee. Default null.
     * @param int    $skip_frames  Optional. A number of stack frames to skip - useful for unwinding
     *                             back to the source of the issue. Default 0.
     * @param bool   $pretty       Optional. Whether you want a comma separated string instead of
     *                             the raw array returned. Default true.
     * @return string|array Either a string containing a reversed comma separated trace or an array
     *                      of individual calls.
     * @phpstan-return ($pretty is true ? string : list<string>)
     */
    function wp_debug_backtrace_summary($ignore_class = \null, $skip_frames = 0, $pretty = \true)
    {
    }
    /**
     * Tests if a given path is a stream URL
     *
     * @since 3.5.0
     *
     * @param string $path The resource path or URL.
     * @return bool True if the path is a stream URL.
     * @phpstan-assert-if-true =non-falsy-string $path
     */
    function wp_is_stream($path)
    {
    }
    /**
     * Returns RegEx body to liberally match an opening HTML tag.
     *
     * Matches an opening HTML tag that:
     * 1. Is self-closing or
     * 2. Has no body but has a closing tag of the same name or
     * 3. Contains a body and a closing tag of the same name
     *
     * Note: this RegEx does not balance inner tags and does not attempt
     * to produce valid HTML
     *
     * @since 3.6.0
     *
     * @param string $tag An HTML tag name. Example: 'video'.
     * @return string Tag RegEx.
     * @phpstan-return non-falsy-string
     */
    function get_tag_regex($tag)
    {
    }
    /**
     * Generates a random UUID (version 4).
     *
     * @since 4.7.0
     * @since 7.0.0 Uses wp_rand if available.
     *
     * @return string UUID.
     * @phpstan-impure
     * @phpstan-return lowercase-string&non-falsy-string
     */
    function wp_generate_uuid4()
    {
    }
    /**
     * Validates that a UUID is valid.
     *
     * @since 4.9.0
     *
     * @param mixed $uuid    UUID to check.
     * @param int   $version Specify which version of UUID to check against. Default is none,
     *                       to accept any UUID version. Otherwise, only version allowed is `4`.
     * @return bool The string is a valid UUID or false on failure.
     * @phpstan-param TUuid $uuid
     * @phpstan-param 4 $version
     * @phpstan-template TUuid of string
     * @phpstan-assert-if-true =TUuid&lowercase-string&non-falsy-string $uuid
     * @phpstan-pure
     * @phpstan-return ($version is 4|null ? bool : false)
     */
    function wp_is_uuid($uuid, $version = \null)
    {
    }
    /**
     * Gets unique ID.
     *
     * This is a PHP implementation of Underscore's uniqueId method. A static variable
     * contains an integer that is incremented with each call. This number is returned
     * with the optional prefix. As such the returned value is not universally unique,
     * but it is unique across the life of the PHP process.
     *
     * @since 5.0.3
     *
     * @param string $prefix Prefix for the returned ID.
     * @return string Unique ID.
     * @phpstan-impure
     * @phpstan-return ($prefix is ''|numeric-string ? numeric-string : string)&non-falsy-string&($prefix is lowercase-string ? lowercase-string : string)
     */
    function wp_unique_id($prefix = '')
    {
    }
    /**
     * Generates an incremental ID that is independent per each different prefix.
     *
     * It is similar to `wp_unique_id`, but each prefix has its own internal ID
     * counter to make each prefix independent from each other. The ID starts at 1
     * and increments on each call. The returned value is not universally unique,
     * but it is unique across the life of the PHP process and it's stable per
     * prefix.
     *
     * @since 6.4.0
     *
     * @param string $prefix Optional. Prefix for the returned ID. Default empty string.
     * @return string Incremental ID per prefix.
     * @phpstan-impure
     * @phpstan-return ($prefix is ''|numeric-string ? numeric-string : string)&non-falsy-string&($prefix is lowercase-string ? lowercase-string : string)
     */
    function wp_unique_prefixed_id($prefix = '')
    {
    }
    /**
     * Generates a unique ID based on the structure and values of a given array.
     *
     * This function serializes the array into a JSON string and generates a hash
     * that serves as a unique identifier. Optionally, a prefix can be added to
     * the generated ID for context or categorization.
     *
     * @since 6.8.0
     *
     * @param array  $data   The input array to generate an ID from.
     * @param string $prefix Optional. A prefix to prepend to the generated ID. Default empty string.
     * @return string The generated unique ID for the array.
     * @phpstan-param non-empty-array $data
     * @phpstan-return ($prefix is lowercase-string ? lowercase-string&non-falsy-string : non-falsy-string)
     */
    function wp_unique_id_from_values(array $data, string $prefix = ''): string
    {
    }
    /**
     * Returns the current WordPress version.
     *
     * Returns an unmodified value of `$wp_version`. Some plugins modify the global
     * in an attempt to improve security through obscurity. This practice can cause
     * errors in WordPress, so the ability to get an unmodified version is needed.
     *
     * @since 6.7.0
     *
     * @return string The current WordPress version.
     * @phpstan-return non-falsy-string
     */
    function wp_get_wp_version()
    {
    }
    /**
     * Returns a cryptographically secure hash of a message using a fast generic hash function.
     *
     * Use the wp_verify_fast_hash() function to verify the hash.
     *
     * This function does not salt the value prior to being hashed, therefore input to this function must originate from
     * a random generator with sufficiently high entropy, preferably greater than 128 bits. This function is used internally
     * in WordPress to hash security keys and application passwords which are generated with high entropy.
     *
     * Important:
     *
     *  - This function must not be used for hashing user-generated passwords. Use wp_hash_password() for that.
     *  - This function must not be used for hashing other low-entropy input. Use wp_hash() for that.
     *
     * The BLAKE2b algorithm is used by Sodium to hash the message.
     *
     * @since 6.8.0
     *
     * @throws TypeError Thrown by Sodium if the message is not a string.
     *
     * @param string $message The message to hash.
     * @return string The hash of the message.
     * @phpstan-return non-falsy-string
     */
    function wp_fast_hash(
        #[\SensitiveParameter]
        string $message
    ): string
    {
    }
    /**
     * Initializes $wp_scripts if it has not been set.
     *
     * @since 4.2.0
     *
     * @global WP_Scripts $wp_scripts
     *
     * @return WP_Scripts WP_Scripts instance.
     */
    function wp_scripts()
    {
    }
    /**
     * Initializes $wp_styles if it has not been set.
     *
     * @since 4.2.0
     *
     * @global WP_Styles $wp_styles
     *
     * @return WP_Styles WP_Styles instance.
     */
    function wp_styles()
    {
    }
    /**
     * Displays the Log In/Out link.
     *
     * Displays a link, which allows users to navigate to the Log In page to log in
     * or log out depending on whether they are currently logged in.
     *
     * @since 1.5.0
     *
     * @param string $redirect Optional path to redirect to on login/logout.
     * @param bool   $display  Default to echo and not return the link.
     * @return void|string Void if `$display` argument is true, log in/out link if `$display` is false.
     * @phpstan-return ($display is true ? void : string)
     */
    function wp_loginout($redirect = '', $display = \true)
    {
    }
    /**
     * Displays the Registration or Admin link.
     *
     * Display a link which allows the user to navigate to the registration page if
     * not logged in and registration is enabled or to the dashboard if logged in.
     *
     * @since 1.5.0
     *
     * @param string $before  Text to output before the link. Default `<li>`.
     * @param string $after   Text to output after the link. Default `</li>`.
     * @param bool   $display Default to echo and not return the link.
     * @return void|string Void if `$display` argument is true, registration or admin link
     *                     if `$display` is false.
     * @phpstan-return ($display is true ? void : string)
     */
    function wp_register($before = '<li>', $after = '</li>', $display = \true)
    {
    }
    /**
     * Displays or retrieves page title for all areas of blog.
     *
     * By default, the page title will display the separator before the page title,
     * so that the blog title will be before the page title. This is not good for
     * title display, since the blog title shows up on most tabs and not what is
     * important, which is the page that the user is looking at.
     *
     * There are also SEO benefits to having the blog title after or to the 'right'
     * of the page title. However, it is mostly common sense to have the blog title
     * to the right with most browsers supporting tabs. You can achieve this by
     * using the seplocation parameter and setting the value to 'right'. This change
     * was introduced around 2.5.0, in case backward compatibility of themes is
     * important.
     *
     * @since 1.0.0
     *
     * @global WP_Locale $wp_locale WordPress date and time locale object.
     *
     * @param string $sep         Optional. How to separate the various items within the page title.
     *                            Default '&raquo;'.
     * @param bool   $display     Optional. Whether to display or retrieve title. Default true.
     * @param string $seplocation Optional. Location of the separator (either 'left' or 'right').
     * @return string|null String when `$display` is false, null otherwise.
     * @phpstan-param 'left'|'right' $seplocation
     * @phpstan-return ($display is true ? void : string)
     */
    function wp_title($sep = '&raquo;', $display = \true, $seplocation = '')
    {
    }
    /**
     * Displays or retrieves page title for post.
     *
     * This is optimized for single.php template file for displaying the post title.
     *
     * It does not support placing the separator after the title, but by leaving the
     * prefix parameter empty, you can set the title separator manually. The prefix
     * does not automatically place a space between the prefix, so if there should
     * be a space, the parameter value will need to have it at the end.
     *
     * @since 0.71
     *
     * @param string $prefix  Optional. What to display before the title.
     * @param bool   $display Optional. Whether to display or retrieve title. Default true.
     * @return string|null Title when retrieving.
     * @phpstan-return ($display is true ? void : string|void)
     */
    function single_post_title($prefix = '', $display = \true)
    {
    }
    /**
     * Displays or retrieves title for a post type archive.
     *
     * This is optimized for archive.php and archive-{$post_type}.php template files
     * for displaying the title of the post type.
     *
     * @since 3.1.0
     *
     * @param string $prefix  Optional. What to display before the title.
     * @param bool   $display Optional. Whether to display or retrieve title. Default true.
     * @return string|null Title when retrieving, null when displaying or on failure.
     * @phpstan-return ($display is true ? void : string|void)
     */
    function post_type_archive_title($prefix = '', $display = \true)
    {
    }
    /**
     * Displays or retrieves page title for category archive.
     *
     * Useful for category template files for displaying the category page title.
     * The prefix does not automatically place a space between the prefix, so if
     * there should be a space, the parameter value will need to have it at the end.
     *
     * @since 0.71
     *
     * @param string $prefix  Optional. What to display before the title.
     * @param bool   $display Optional. Whether to display or retrieve title. Default true.
     * @return string|null Title when retrieving.
     * @phpstan-return ($display is true ? void : string|void)
     */
    function single_cat_title($prefix = '', $display = \true)
    {
    }
    /**
     * Displays or retrieves page title for tag post archive.
     *
     * Useful for tag template files for displaying the tag page title. The prefix
     * does not automatically place a space between the prefix, so if there should
     * be a space, the parameter value will need to have it at the end.
     *
     * @since 2.3.0
     *
     * @param string $prefix  Optional. What to display before the title.
     * @param bool   $display Optional. Whether to display or retrieve title. Default true.
     * @return string|null Title when retrieving.
     * @phpstan-return ($display is true ? void : string|void)
     */
    function single_tag_title($prefix = '', $display = \true)
    {
    }
    /**
     * Displays or retrieves page title for taxonomy term archive.
     *
     * Useful for taxonomy term template files for displaying the taxonomy term page title.
     * The prefix does not automatically place a space between the prefix, so if there should
     * be a space, the parameter value will need to have it at the end.
     *
     * @since 3.1.0
     *
     * @param string $prefix  Optional. What to display before the title.
     * @param bool   $display Optional. Whether to display or retrieve title. Default true.
     * @return string|null Title when retrieving.
     * @phpstan-return ($display is true ? void : string|void)
     */
    function single_term_title($prefix = '', $display = \true)
    {
    }
    /**
     * Displays or retrieves page title for post archive based on date.
     *
     * Useful for when the template only needs to display the month and year,
     * if either are available. The prefix does not automatically place a space
     * between the prefix, so if there should be a space, the parameter value
     * will need to have it at the end.
     *
     * @since 0.71
     *
     * @global WP_Locale $wp_locale WordPress date and time locale object.
     *
     * @param string $prefix  Optional. What to display before the title.
     * @param bool   $display Optional. Whether to display or retrieve title. Default true.
     * @return string|false|null False if there's no valid title for the month. Title when retrieving.
     * @phpstan-return ($display is true ? false|void : false|string)
     */
    function single_month_title($prefix = '', $display = \true)
    {
    }
    /**
     * Displays archive links based on type and format.
     *
     * @since 1.2.0
     * @since 4.4.0 The `$post_type` argument was added.
     * @since 5.2.0 The `$year`, `$monthnum`, `$day`, and `$w` arguments were added.
     *
     * @see get_archives_link()
     *
     * @global wpdb      $wpdb      WordPress database abstraction object.
     * @global WP_Locale $wp_locale WordPress date and time locale object.
     *
     * @param string|array $args {
     *     Default archive links arguments. Optional.
     *
     *     @type string     $type            Type of archive to retrieve. Accepts 'daily', 'weekly', 'monthly',
     *                                       'yearly', 'postbypost', or 'alpha'. Both 'postbypost' and 'alpha'
     *                                       display the same archive link list as well as post titles instead
     *                                       of displaying dates. The difference between the two is that 'alpha'
     *                                       will order by post title and 'postbypost' will order by post date.
     *                                       Default 'monthly'.
     *     @type string|int $limit           Number of links to limit the query to. Default empty (no limit).
     *     @type string     $format          Format each link should take using the $before and $after args.
     *                                       Accepts 'link' (`<link>` tag), 'option' (`<option>` tag), 'html'
     *                                       (`<li>` tag), or a custom format, which generates a link anchor
     *                                       with $before preceding and $after succeeding. Default 'html'.
     *     @type string     $before          Markup to prepend to the beginning of each link. Default empty.
     *     @type string     $after           Markup to append to the end of each link. Default empty.
     *     @type bool       $show_post_count Whether to display the post count alongside the link. Default false.
     *     @type bool|int   $echo            Whether to echo or return the links list. Default 1|true to echo.
     *     @type string     $order           Whether to use ascending or descending order. Accepts 'ASC', or 'DESC'.
     *                                       Default 'DESC'.
     *     @type string     $post_type       Post type. Default 'post'.
     *     @type string     $year            Year. Default current year.
     *     @type string     $monthnum        Month number. Default current month number.
     *     @type string     $day             Day. Default current day.
     *     @type string     $w               Week. Default current week.
     * }
     * @return void|string Void if 'echo' argument is true, archive links if 'echo' is false.
     * @phpstan-param array{
     *   type?: string,
     *   limit?: string|int,
     *   format?: string,
     *   before?: string,
     *   after?: string,
     *   show_post_count?: bool,
     *   echo?: bool|int,
     *   order?: string,
     *   post_type?: string,
     *   year?: string,
     *   monthnum?: string,
     *   day?: string,
     *   w?: string,
     * } $args
     * @phpstan-return ($args is array{echo: false|0, ...} ? string|void : void)
     */
    function wp_get_archives($args = '')
    {
    }
    /**
     * Displays calendar with days that have posts as links.
     *
     * The calendar is cached, which will be retrieved, if it exists. If there are
     * no posts for the month, then it will not be displayed.
     *
     * @since 1.0.0
     * @since 6.8.0 Added the `$args` parameter, with backward compatibility
     *              for the replaced `$initial` and `$display` parameters.
     *
     * @global wpdb      $wpdb      WordPress database abstraction object.
     * @global int       $m
     * @global int       $monthnum
     * @global int       $year
     * @global WP_Locale $wp_locale WordPress date and time locale object.
     * @global array     $posts
     *
     * @param array $args {
     *     Optional. Arguments for the `get_calendar` function.
     *
     *     @type bool   $initial   Whether to use initial calendar names. Default true.
     *     @type bool   $display   Whether to display the calendar output. Default true.
     *     @type string $post_type Optional. Post type. Default 'post'.
     * }
     * @return void|string Void if `$display` argument is true, calendar HTML if `$display` is false.
     * @phpstan-param array{
     *   initial?: bool,
     *   display?: bool,
     *   post_type?: string,
     * } $args
     * @phpstan-return ($args is array{display: false, ...} ? string : void)
     */
    function get_calendar($args = array())
    {
    }
    /**
     * Displays or retrieves the date of the post (once per date).
     *
     * Will only output the date if the current post's date is different from the
     * previous one output.
     *
     * i.e. Only one date listing will show per day worth of posts shown in the loop, even if the
     * function is called several times for each post.
     *
     * HTML output can be filtered with {@see 'the_date'}.
     * Date string output can be filtered with {@see 'get_the_date'}.
     *
     * @since 0.71
     *
     * @global string $currentday  The day of the current post in the loop.
     * @global string $previousday The day of the previous post in the loop.
     *
     * @param string $format  Optional. PHP date format. Defaults to the 'date_format' option.
     * @param string $before  Optional. Output before the date. Default empty.
     * @param string $after   Optional. Output after the date. Default empty.
     * @param bool   $display Optional. Whether to echo the date or return it. Default true.
     * @return string|null String if retrieving.
     * @phpstan-return ($display is true ? void : string)
     */
    function the_date($format = '', $before = '', $after = '', $display = \true)
    {
    }
    /**
     * Displays the date on which the post was last modified.
     *
     * @since 2.1.0
     *
     * @param string $format  Optional. PHP date format. Defaults to the 'date_format' option.
     * @param string $before  Optional. Output before the date. Default empty.
     * @param string $after   Optional. Output after the date. Default empty.
     * @param bool   $display Optional. Whether to echo the date or return it. Default true.
     * @return string|null String if retrieving.
     * @phpstan-return ($display is true ? void : string)
     */
    function the_modified_date($format = '', $before = '', $after = '', $display = \true)
    {
    }
    /**
     * Determines whether the user can access the visual editor.
     *
     * Checks if the user can access the visual editor and that it's supported by the user's browser.
     *
     * @since 2.0.0
     *
     * @global bool $wp_rich_edit Whether the user can access the visual editor.
     * @global bool $is_gecko     Whether the browser is Gecko-based.
     * @global bool $is_opera     Whether the browser is Opera.
     * @global bool $is_safari    Whether the browser is Safari.
     * @global bool $is_chrome    Whether the browser is Chrome.
     * @global bool $is_IE        Whether the browser is Internet Explorer.
     * @global bool $is_edge      Whether the browser is Microsoft Edge.
     *
     * @return bool True if the user can access the visual editor, false otherwise.
     */
    function user_can_richedit()
    {
    }
    /**
     * Retrieves paginated links for archive post pages.
     *
     * Technically, the function can be used to create paginated link list for any
     * area. The 'base' argument is used to reference the url, which will be used to
     * create the paginated links. The 'format' argument is then used for replacing
     * the page number. It is however, most likely and by default, to be used on the
     * archive post pages.
     *
     * The 'type' argument controls format of the returned value. The default is
     * 'plain', which is just a string with the links separated by a newline
     * character. The other possible values are either 'array' or 'list'. The
     * 'array' value will return an array of the paginated link list to offer full
     * control of display. The 'list' value will place all of the paginated links in
     * an unordered HTML list.
     *
     * The 'total' argument is the total amount of pages and is an integer. The
     * 'current' argument is the current page number and is also an integer.
     *
     * An example of the 'base' argument is "http://example.com/all_posts.php%_%"
     * and the '%_%' is required. The '%_%' will be replaced by the contents of in
     * the 'format' argument. An example for the 'format' argument is "?page=%#%"
     * and the '%#%' is also required. The '%#%' will be replaced with the page
     * number.
     *
     * You can include the previous and next links in the list by setting the
     * 'prev_next' argument to true, which it is by default. You can set the
     * previous text, by using the 'prev_text' argument. You can set the next text
     * by setting the 'next_text' argument.
     *
     * If the 'show_all' argument is set to true, then it will show all of the pages
     * instead of a short list of the pages near the current page. By default, the
     * 'show_all' is set to false and controlled by the 'end_size' and 'mid_size'
     * arguments. The 'end_size' argument is how many numbers on either the start
     * and the end list edges, by default is 1. The 'mid_size' argument is how many
     * numbers to either side of current page, but not including current page.
     *
     * It is possible to add query vars to the link by using the 'add_args' argument
     * and see add_query_arg() for more information.
     *
     * The 'before_page_number' and 'after_page_number' arguments allow users to
     * augment the links themselves. Typically this might be to add context to the
     * numbered links so that screen reader users understand what the links are for.
     * The text strings are added before and after the page number - within the
     * anchor tag.
     *
     * @since 2.1.0
     * @since 4.9.0 Added the `aria_current` argument.
     *
     * @global WP_Query   $wp_query   WordPress Query object.
     * @global WP_Rewrite $wp_rewrite WordPress rewrite component.
     *
     * @param string|array $args {
     *     Optional. Array or string of arguments for generating paginated links for archives.
     *
     *     @type string $base               Base of the paginated url. Default empty.
     *     @type string $format             Format for the pagination structure. Default empty.
     *     @type int    $total              The total amount of pages. Default is the value WP_Query's
     *                                      `max_num_pages` or 1.
     *     @type int    $current            The current page number. Default is 'paged' query var or 1.
     *     @type string $aria_current       The value for the aria-current attribute. Possible values are 'page',
     *                                      'step', 'location', 'date', 'time', 'true', 'false'. Default is 'page'.
     *     @type bool   $show_all           Whether to show all pages. Default false.
     *     @type int    $end_size           How many numbers on either the start and the end list edges.
     *                                      Default 1.
     *     @type int    $mid_size           How many numbers to either side of the current pages. Default 2.
     *     @type bool   $prev_next          Whether to include the previous and next links in the list. Default true.
     *     @type string $prev_text          The previous page text. Default '&laquo; Previous'.
     *     @type string $next_text          The next page text. Default 'Next &raquo;'.
     *     @type string $type               Controls format of the returned value. Possible values are 'plain',
     *                                      'array' and 'list'. Default is 'plain'.
     *     @type array  $add_args           An array of query args to add. Default false.
     *     @type string $add_fragment       A string to append to each link. Default empty.
     *     @type string $before_page_number A string to appear before the page number. Default empty.
     *     @type string $after_page_number  A string to append after the page number. Default empty.
     * }
     * @return string|string[]|null String of page links or array of page links, depending on 'type' argument.
     *                              Null if total number of pages is less than 2.
     * @phpstan-param array{
     *   base?: string,
     *   format?: string,
     *   total?: int,
     *   current?: int,
     *   aria_current?: string,
     *   show_all?: bool,
     *   end_size?: int,
     *   mid_size?: int,
     *   prev_next?: bool,
     *   prev_text?: string,
     *   next_text?: string,
     *   type?: string,
     *   add_args?: array,
     *   add_fragment?: string,
     *   before_page_number?: string,
     *   after_page_number?: string,
     * } $args
     * @phpstan-return ($args is array{total: int<min, 1>, ...} ? null : ($args is array{type: 'array', ...} ? list<string> : string))
     */
    function paginate_links($args = '')
    {
    }
    /**
     * Outputs the HTML selected attribute.
     *
     * Compares the first two arguments and if identical marks as selected.
     *
     * @since 1.0.0
     *
     * @param mixed $selected One of the values to compare.
     * @param mixed $current  Optional. The other value to compare if not just true.
     *                        Default true.
     * @param bool  $display  Optional. Whether to echo or just return the string.
     *                        Default true.
     * @return string HTML attribute or empty string.
     */
    function selected($selected, $current = \true, $display = \true)
    {
    }
    /**
     * Validates a URL as safe for use in the HTTP API.
     *
     * The only supported protocols are `http` and `https`.
     *
     * Examples of URLs that are considered unsafe:
     *
     * - `ftp://example.com/caniload.php` - Invalid protocol - only http and https are allowed.
     * - `http:///example.com/caniload.php` - Malformed URL.
     * - `http://user:pass@example.com/caniload.php` - Login information.
     * - `http://example.invalid/caniload.php` - Invalid hostname, as the IP cannot be looked up in DNS.
     *
     * Examples of URLs that are considered unsafe by default but can be allowed with filters:
     *
     * - `http://192.168.0.1/caniload.php` - IP address from LAN network.
     *   This can be changed with the {@see 'http_request_host_is_external'} filter.
     * - `http://198.143.164.252:81/caniload.php` - By default, only ports 80, 443, and 8080 are allowed.
     *   This can be changed with the {@see 'http_allowed_safe_ports'} filter.
     *
     * @since 3.5.2
     *
     * @param string $url Request URL.
     * @return string|false Returns false if the URL is not safe, or the original URL if it is safe.
     * @phpstan-template TUrl of string
     * @phpstan-param TUrl $url
     * @phpstan-return (TUrl is numeric|'' ? false : TUrl|false)
     */
    function wp_http_validate_url($url)
    {
    }
    /**
     * Retrieves the translation of $text.
     *
     * If there is no translation, or the text domain isn't loaded, the original text is returned.
     *
     * *Note:* Don't use translate() directly, use __() or related functions.
     *
     * @since 2.2.0
     * @since 5.5.0 Introduced `gettext-{$domain}` filter.
     *
     * @param string $text   Text to translate.
     * @param string $domain Optional. Text domain. Unique identifier for retrieving translated strings.
     *                       Default 'default'.
     * @return string Translated text.
     */
    function translate($text, $domain = 'default')
    {
    }
    /**
     * Displays or returns a Language selector.
     *
     * @since 4.0.0
     * @since 4.3.0 Introduced the `echo` argument.
     * @since 4.7.0 Introduced the `show_option_site_default` argument.
     * @since 5.1.0 Introduced the `show_option_en_us` argument.
     * @since 5.9.0 Introduced the `explicit_option_en_us` argument.
     *
     * @see get_available_languages()
     * @see wp_get_available_translations()
     *
     * @param string|array $args {
     *     Optional. Array or string of arguments for outputting the language selector.
     *
     *     @type string   $id                           ID attribute of the select element. Default 'locale'.
     *     @type string   $name                         Name attribute of the select element. Default 'locale'.
     *     @type string[] $languages                    List of installed languages, contain only the locales.
     *                                                  Default empty array.
     *     @type array    $translations                 List of available translations. Default result of
     *                                                  wp_get_available_translations().
     *     @type string   $selected                     Language which should be selected. Default empty.
     *     @type bool|int $echo                         Whether to echo the generated markup. Accepts 0, 1, or their
     *                                                  boolean equivalents. Default 1.
     *     @type bool     $show_available_translations  Whether to show available translations. Default true.
     *     @type bool     $show_option_site_default     Whether to show an option to fall back to the site's locale. Default false.
     *     @type bool     $show_option_en_us            Whether to show an option for English (United States). Default true.
     *     @type bool     $explicit_option_en_us        Whether the English (United States) option uses an explicit value of en_US
     *                                                  instead of an empty value. Default false.
     * }
     * @return string HTML dropdown list of languages.
     * @phpstan-param array{
     *   id?: string,
     *   name?: string,
     *   languages?: string[],
     *   translations?: array,
     *   selected?: string,
     *   echo?: bool|int,
     *   show_available_translations?: bool,
     *   show_option_site_default?: bool,
     *   show_option_en_us?: bool,
     *   explicit_option_en_us?: bool,
     * } $args
     * @phpstan-return ($args is array{id: null|'', ...} ? void : ($args is array{name: null|'', ...} ? void : string))
     */
    function wp_dropdown_languages($args = array())
    {
    }
    /**
     * Retrieves the word count type based on the locale.
     *
     * @since 6.2.0
     *
     * @global WP_Locale $wp_locale WordPress date and time locale object.
     *
     * @return string Locale-specific word count type. Possible values are `characters_excluding_spaces`,
     *                `characters_including_spaces`, or `words`. Defaults to `words`.
     * @phpstan-return 'characters_excluding_spaces'|'characters_including_spaces'|'words'
     */
    function wp_get_word_count_type()
    {
    }
    /**
     * Retrieves the full permalink for the current post or post ID.
     *
     * This function is an alias for get_permalink().
     *
     * @since 3.9.0
     *
     * @see get_permalink()
     *
     * @param int|WP_Post $post      Optional. Post ID or post object. Default is the global `$post`.
     * @param bool        $leavename Optional. Whether to keep post name or page name. Default false.
     * @return string|false The permalink URL. False if the post does not exist.
     * @phpstan-return ($post is \WP_Post ? string : string|false)
     */
    function get_the_permalink($post = 0, $leavename = \false)
    {
    }
    /**
     * Retrieves the full permalink for the current post or post ID.
     *
     * @since 1.0.0
     *
     * @param int|WP_Post $post      Optional. Post ID or post object. Default is the global `$post`.
     * @param bool        $leavename Optional. Whether to keep post name or page name. Default false.
     * @return string|false The permalink URL. False if the post does not exist.
     * @phpstan-return ($post is \WP_Post ? string : string|false)
     */
    function get_permalink($post = 0, $leavename = \false)
    {
    }
    /**
     * Retrieves the permalink for a post of a custom post type.
     *
     * @since 3.0.0
     * @since 6.1.0 Returns false if the post does not exist.
     *
     * @global WP_Rewrite $wp_rewrite WordPress rewrite component.
     *
     * @param int|WP_Post $post      Optional. Post ID or post object. Default is the global `$post`.
     * @param bool        $leavename Optional. Whether to keep post name. Default false.
     * @param bool        $sample    Optional. Is it a sample permalink. Default false.
     * @return string|false The post permalink URL. False if the post does not exist.
     * @phpstan-return ($post is \WP_Post ? string : string|false)
     */
    function get_post_permalink($post = 0, $leavename = \false, $sample = \false)
    {
    }
    /**
     * Displays or retrieves the edit term link with formatting.
     *
     * @since 3.1.0
     *
     * @param string           $link    Optional. Anchor text. If empty, default is 'Edit This'. Default empty.
     * @param string           $before  Optional. Display before edit link. Default empty.
     * @param string           $after   Optional. Display after edit link. Default empty.
     * @param int|WP_Term|null $term    Optional. Term ID or object. If null, the queried object will be inspected. Default null.
     * @param bool             $display Optional. Whether or not to echo the return. Default true.
     * @return string|null HTML content.
     * @phpstan-return ($display is true ? void : string)|null
     */
    function edit_term_link($link = '', $before = '', $after = '', $term = \null, $display = \true)
    {
    }
    /**
     * Displays or retrieves the next posts page link.
     *
     * @since 0.71
     *
     * @param int  $max_page Optional. Max pages. Default 0.
     * @param bool $display  Optional. Whether to echo the link. Default true.
     * @return string|null The link URL for next posts page if `$display = false`.
     * @phpstan-return ($display is true ? void : string)
     */
    function next_posts($max_page = 0, $display = \true)
    {
    }
    /**
     * Displays or retrieves the previous posts page link.
     *
     * @since 0.71
     *
     * @param bool $display Optional. Whether to echo the link. Default true.
     * @return string|null The previous posts page link if `$display = false`.
     * @phpstan-return ($display is true ? void : string)
     */
    function previous_posts($display = \true)
    {
    }
    /**
     * Returns an array of URL hosts which are considered to be internal hosts.
     *
     * By default the list of internal hosts is comprised of the host name of
     * the site's home_url() (as parsed by wp_parse_url()).
     *
     * This list is used when determining if a specified URL is a link to a page on
     * the site itself or a link offsite (to an external host). This is used, for
     * example, when determining if the "nofollow" attribute should be applied to a
     * link.
     *
     * @see wp_is_internal_link
     *
     * @since 6.2.0
     *
     * @return string[] An array of URL hosts.
     * @phpstan-return array<lowercase-string>
     */
    function wp_internal_hosts()
    {
    }
    /**
     * Returns the HTTP protocol sent by the server.
     *
     * @since 4.4.0
     *
     * @return string The HTTP protocol. Default: HTTP/1.0.
     * @phpstan-return 'HTTP/1.0'|'HTTP/1.1'|'HTTP/2'|'HTTP/2.0'|'HTTP/3'
     */
    function wp_get_server_protocol()
    {
    }
    /**
     * Converts a value to non-negative integer.
     *
     * @since 2.5.0
     *
     * @param mixed $maybeint Data you wish to have converted to a non-negative integer.
     * @return int A non-negative integer.
     * @phpstan-template T of int<0, max>
     * @phpstan-param T|int<min,-1>|scalar|array|resource|null $maybeint
     * @phpstan-pure
     * @phpstan-return ($maybeint is T ? T : ($maybeint is int<min, -1> ? int<1, max> : ($maybeint is empty ? 0 : ($maybeint is numeric-string ? int<0, max> : ($maybeint is string ? 0 : ($maybeint is true|non-empty-array ? 1 : ($maybeint is bool ? 0|1 : int<0, max>)))))))
     */
    function absint($maybeint)
    {
    }
    /**
     * Retrieves the current site ID.
     *
     * @since 3.1.0
     *
     * @global int $blog_id
     *
     * @return int Site ID.
     * @phpstan-return int<0, max>
     */
    function get_current_blog_id()
    {
    }
    /**
     * Checks whether the given variable is a WordPress Error.
     *
     * Returns whether `$thing` is an instance of the `WP_Error` class.
     *
     * @since 2.1.0
     *
     * @param mixed $thing The variable to check.
     * @return bool Whether the variable is an instance of WP_Error.
     *
     * @phpstan-assert-if-true WP_Error $thing
     * @phpstan-return ($thing is \WP_Error ? true : false)
     */
    function is_wp_error($thing)
    {
    }
    /**
     * Retrieves taxonomies attached to given the attachment.
     *
     * @since 2.5.0
     * @since 4.7.0 Introduced the `$output` parameter.
     *
     * @param int|array|object $attachment Attachment ID, data array, or data object.
     * @param string           $output     Output type. 'names' to return an array of taxonomy names,
     *                                     or 'objects' to return an array of taxonomy objects.
     *                                     Default is 'names'.
     * @return string[]|WP_Taxonomy[] List of taxonomies or taxonomy names. Empty array on failure.
     * @phpstan-return ($output is 'names' ? array<int, string> : array<string, \WP_Taxonomy>)
     */
    function get_attachment_taxonomies($attachment, $output = 'names')
    {
    }
    /**
     * Retrieves all of the taxonomies that are registered for attachments.
     *
     * Handles mime-type-specific taxonomies such as attachment:image and attachment:video.
     *
     * @since 3.5.0
     *
     * @see get_taxonomies()
     *
     * @param string $output Optional. The type of taxonomy output to return. Accepts 'names' or 'objects'.
     *                       Default 'names'.
     * @return string[]|WP_Taxonomy[] Array of names or objects of registered taxonomies for attachments.
     * @phpstan-param 'names'|'objects' $output
     * @phpstan-return ($output is 'names' ? array<int, string> : array<string, \WP_Taxonomy>)
     */
    function get_taxonomies_for_attachments($output = 'names')
    {
    }
    /**
     * Retrieves a list of sites matching requested arguments.
     *
     * @since 4.6.0
     * @since 4.8.0 Introduced the 'lang_id', 'lang__in', and 'lang__not_in' parameters.
     *
     * @see WP_Site_Query::parse_query()
     *
     * @param string|array $args Optional. Array or string of arguments. See WP_Site_Query::__construct()
     *                           for information on accepted arguments. Default empty array.
     * @return WP_Site[]|int[]|int List of WP_Site objects, a list of site IDs when 'fields' is set to 'ids',
     *                             or the number of sites when 'count' is passed as a query var.
     * @phpstan-param array{
     *   site__in?: int[],
     *   site__not_in?: int[],
     *   count?: bool,
     *   date_query?: array,
     *   fields?: string,
     *   ID?: int,
     *   number?: int,
     *   offset?: int,
     *   no_found_rows?: bool,
     *   orderby?: string|array,
     *   order?: string,
     *   network_id?: int,
     *   network__in?: int[],
     *   network__not_in?: int[],
     *   domain?: string,
     *   domain__in?: string[],
     *   domain__not_in?: string[],
     *   path?: string,
     *   path__in?: string[],
     *   path__not_in?: string[],
     *   public?: int,
     *   archived?: int,
     *   mature?: int,
     *   spam?: int,
     *   deleted?: int,
     *   lang_id?: int,
     *   lang__in?: string[],
     *   lang__not_in?: string[],
     *   search?: string,
     *   search_columns?: string[],
     *   update_site_cache?: bool,
     *   update_site_meta_cache?: bool,
     *   meta_key?: string|string[],
     *   meta_value?: string|string[],
     *   meta_compare?: string,
     *   meta_compare_key?: string,
     *   meta_type?: string,
     *   meta_type_key?: string,
     *   meta_query?: array,
     * } $args See WP_Site_Query::__construct()
     * @phpstan-return ($args is array{count: true, ...} ? int : ($args is array{fields: 'ids', ...} ? array<int, int> : array<int, \WP_Site>))
     */
    function get_sites($args = array())
    {
    }
    /**
     * Retrieves user info by a given field.
     *
     * @since 2.8.0
     * @since 4.4.0 Added 'ID' as an alias of 'id' for the `$field` parameter.
     *
     * @global WP_User $current_user The current user object which holds the user data.
     *
     * @param string     $field The field to retrieve the user with. id | ID | slug | email | login.
     * @param int|string $value A value for $field. A user ID, slug, email address, or login name.
     * @return WP_User|false WP_User object on success, false on failure.
     * @phpstan-return ($field is 'id'|'ID' ? ($value is int<min, 0> ? false : \WP_User|false) : \WP_User|false)
     */
    function get_user_by($field, $value)
    {
    }
    /**
     * Ensures intent by verifying that a user was referred from another admin page with the correct security nonce.
     *
     * This function ensures the user intends to perform a given action, which helps protect against clickjacking style
     * attacks. It verifies intent, not authorization, therefore it does not verify the user's capabilities. This should
     * be performed with `current_user_can()` or similar.
     *
     * If the nonce value is invalid, the function will exit with an "Are You Sure?" style message.
     *
     * @since 1.2.0
     * @since 2.5.0 The `$query_arg` parameter was added.
     *
     * @param int|string $action    The nonce action.
     * @param string     $query_arg Optional. Key to check for nonce in `$_REQUEST`. Default '_wpnonce'.
     * @return int|false 1 if the nonce is valid and generated between 0-12 hours ago,
     *                   2 if the nonce is valid and generated between 12-24 hours ago.
     *                   False if the nonce is invalid. Only possible when `$action` is -1,
     *                   as the function otherwise exits rather than returning false.
     * @phpstan-return ( $action is -1 ? int|false : int )
     * @phpstan-param string $action
     * @phpstan-return ($action is -1 ? 1|2|false : 1|2)
     */
    function check_admin_referer($action = -1, $query_arg = '_wpnonce')
    {
    }
    /**
     * Verifies the Ajax request to prevent processing requests external of the blog.
     *
     * @since 2.0.3
     *
     * @param int|string   $action    Action nonce.
     * @param false|string $query_arg Optional. Key to check for the nonce in `$_REQUEST` (since 2.5). If false,
     *                                `$_REQUEST` values will be evaluated for '_ajax_nonce', and '_wpnonce'
     *                                (in that order). Default false.
     * @param bool         $stop      Optional. Whether to stop early when the nonce cannot be verified.
     *                                Default true.
     * @return int|false 1 if the nonce is valid and generated between 0-12 hours ago,
     *                   2 if the nonce is valid and generated between 12-24 hours ago.
     *                   False if the nonce is invalid. Only possible when `$stop` is false,
     *                   as the function otherwise exits rather than returning false.
     * @phpstan-return ( $stop is true ? int : int|false )
     * @phpstan-param string $action
     * @phpstan-return ($stop is true ? 1|2 : 1|2|false)
     */
    function check_ajax_referer($action = -1, $query_arg = \false, $stop = \true)
    {
    }
    /**
     * Returns the time-dependent variable for nonce creation.
     *
     * A nonce has a lifespan of two ticks. Nonces in their second tick may be
     * updated, e.g. by autosave.
     *
     * @since 2.5.0
     * @since 6.1.0 Added `$action` argument.
     *
     * @param string|int $action Optional. The nonce action. Default -1.
     * @return float Float value rounded up to the next highest integer.
     * @phpstan-param -1|string $action
     * @phpstan-impure
     */
    function wp_nonce_tick($action = -1)
    {
    }
    /**
     * Creates a cryptographic token tied to a specific action, user, user session,
     * and window of time.
     *
     * @since 2.0.3
     * @since 4.0.0 Session tokens were integrated with nonce creation.
     *
     * @param string|int $action Scalar value to add context to the nonce.
     * @return string The token.
     * @phpstan-param -1|string $action
     * @phpstan-return lowercase-string&non-falsy-string
     */
    function wp_create_nonce($action = -1)
    {
    }
    /**
     * Gets the hash of the given string.
     *
     * The default algorithm is md5 but can be changed to any algorithm supported by
     * `hash_hmac()`. Use the `hash_hmac_algos()` function to check the supported
     * algorithms.
     *
     * @since 2.0.3
     * @since 6.8.0 The `$algo` parameter was added.
     *
     * @throws InvalidArgumentException if the hashing algorithm is not supported.
     *
     * @param string $data   Plain text to hash.
     * @param string $scheme Authentication scheme (auth, secure_auth, logged_in, nonce).
     * @param string $algo   Hashing algorithm to use. Default: 'md5'.
     * @return string Hash of $data.
     * @phpstan-param 'auth'|'logged_in'|'nonce'|'secure_auth' $scheme
     * @phpstan-return lowercase-string&non-falsy-string
     */
    function wp_hash($data, $scheme = 'auth', $algo = 'md5')
    {
    }
    /**
     * Generates a random password drawn from the defined set of characters.
     *
     * Uses wp_rand() to create passwords with far less predictability
     * than similar native PHP functions like `rand()` or `mt_rand()`.
     *
     * @since 2.5.0
     *
     * @param int  $length              Optional. The length of password to generate. Default 12.
     * @param bool $special_chars       Optional. Whether to include standard special characters.
     *                                  Default true.
     * @param bool $extra_special_chars Optional. Whether to include other special characters.
     *                                  Used when generating secret keys and salts. Default false.
     * @return string The random password.
     * @phpstan-impure
     */
    function wp_generate_password($length = 12, $special_chars = \true, $extra_special_chars = \false)
    {
    }
    /**
     * Generates a random non-negative number.
     *
     * @since 2.6.2
     * @since 4.4.0 Uses PHP7 random_int() or the random_compat library if available.
     * @since 6.1.0 Returns zero instead of a random number if both `$min` and `$max` are zero.
     *
     * @global string $rnd_value
     *
     * @param int $min Optional. Lower limit for the generated number.
     *                 Accepts positive integers or zero. Defaults to 0.
     * @param int $max Optional. Upper limit for the generated number.
     *                 Accepts positive integers. Defaults to 4294967295.
     * @return int A random non-negative number between min and max.
     * @phpstan-impure
     */
    function wp_rand($min = \null, $max = \null)
    {
    }
    /**
     * Checks if any filter has been registered for a hook.
     *
     * When using the `$callback` argument, this function may return a non-boolean value
     * that evaluates to false (e.g. 0), so use the `===` operator for testing the return value.
     *
     * @since 2.5.0
     * @since 6.9.0 Added the `$priority` parameter.
     *
     * @global WP_Hook[] $wp_filter Stores all of the filters and actions.
     *
     * @param string                      $hook_name The name of the filter hook.
     * @param callable|string|array|false $callback  Optional. The callback to check for.
     *                                               This function can be called unconditionally to speculatively check
     *                                               a callback that may or may not exist. Default false.
     * @param int|false                   $priority  Optional. The specific priority at which to check for the callback.
     *                                               Default false.
     * @return bool|int If `$callback` is omitted, returns boolean for whether the hook has
     *                  anything registered. When checking a specific function, the priority
     *                  of that hook is returned, or false if the function is not attached.
     *                  If `$callback` and `$priority` are both provided, a boolean is returned
     *                  for whether the specific function is registered at that priority.
     * @phpstan-return ($callback is false ? bool : false|int)
     */
    function has_filter($hook_name, $callback = \false, $priority = \false)
    {
    }
    /**
     * Retrieves the name of the current filter hook.
     *
     * @since 2.5.0
     *
     * @global string[] $wp_current_filter Stores the list of current filters with the current one last
     *
     * @return string|false Hook name of the current filter, false if no filter is running.
     * @phpstan-return non-empty-string|false
     */
    function current_filter()
    {
    }
    /**
     * Retrieves the number of times a filter has been applied during the current request.
     *
     * @since 6.1.0
     *
     * @global int[] $wp_filters Stores the number of times each filter was triggered.
     *
     * @param string $hook_name The name of the filter hook.
     * @return int The number of times the filter hook has been applied.
     * @phpstan-return int<0, max>
     */
    function did_filter($hook_name)
    {
    }
    /**
     * Checks if any action has been registered for a hook.
     *
     * When using the `$callback` argument, this function may return a non-boolean value
     * that evaluates to false (e.g. 0), so use the `===` operator for testing the return value.
     *
     * @since 2.5.0
     * @since 6.9.0 Added the `$priority` parameter.
     *
     * @see has_filter() This function is an alias of has_filter().
     *
     * @param string                      $hook_name The name of the action hook.
     * @param callable|string|array|false $callback  Optional. The callback to check for.
     *                                               This function can be called unconditionally to speculatively check
     *                                               a callback that may or may not exist. Default false.
     * @param int|false                   $priority  Optional. The specific priority at which to check for the callback.
     *                                               Default false.
     * @return bool|int If `$callback` is omitted, returns boolean for whether the hook has
     *                  anything registered. When checking a specific function, the priority
     *                  of that hook is returned, or false if the function is not attached.
     *                  If `$callback` and `$priority` are both provided, a boolean is returned
     *                  for whether the specific function is registered at that priority.
     * @phpstan-return ($callback is false ? bool : false|int)
     */
    function has_action($hook_name, $callback = \false, $priority = \false)
    {
    }
    /**
     * Retrieves the name of the current action hook.
     *
     * @since 3.9.0
     *
     * @return string|false Hook name of the current action, false if no action is running.
     * @phpstan-return non-empty-string|false
     */
    function current_action()
    {
    }
    /**
     * Retrieves the number of times an action has been fired during the current request.
     *
     * @since 2.1.0
     *
     * @global int[] $wp_actions Stores the number of times each action was triggered.
     *
     * @param string $hook_name The name of the action hook.
     * @return int The number of times the action hook has been fired.
     * @phpstan-return int<0, max>
     */
    function did_action($hook_name)
    {
    }
    /**
     * Displays or retrieves the current post title with optional markup.
     *
     * @since 0.71
     *
     * @param string $before  Optional. Markup to prepend to the title. Default empty.
     * @param string $after   Optional. Markup to append to the title. Default empty.
     * @param bool   $display Optional. Whether to echo or return the title. Default true for echo.
     * @return void|string Void if `$display` argument is true or the title is empty,
     *                     current post title if `$display` is false.
     * @phpstan-return ($display is true ? void : string|void)
     */
    function the_title($before = '', $after = '', $display = \true)
    {
    }
    /**
     * Retrieves or displays a list of pages (or hierarchical post type items) in list (li) format.
     *
     * @since 1.5.0
     * @since 4.7.0 Added the `item_spacing` argument.
     *
     * @see get_pages()
     *
     * @global WP_Query $wp_query WordPress Query object.
     *
     * @param array|string $args {
     *     Optional. Array or string of arguments to generate a list of pages. See get_pages() for additional arguments.
     *
     *     @type int          $child_of     Display only the sub-pages of a single page by ID. Default 0 (all pages).
     *     @type string       $authors      Comma-separated list of author IDs. Default empty (all authors).
     *     @type string       $date_format  PHP date format to use for the listed pages. Relies on the 'show_date' parameter.
     *                                      Default is the value of 'date_format' option.
     *     @type int          $depth        Number of levels in the hierarchy of pages to include in the generated list.
     *                                      Accepts -1 (any depth), 0 (all pages), 1 (top-level pages only), and n (pages to
     *                                      the given n depth). Default 0.
     *     @type bool         $echo         Whether or not to echo the list of pages. Default true.
     *     @type string       $exclude      Comma-separated list of page IDs to exclude. Default empty.
     *     @type array        $include      Comma-separated list of page IDs to include. Default empty.
     *     @type string       $link_after   Text or HTML to follow the page link label. Default null.
     *     @type string       $link_before  Text or HTML to precede the page link label. Default null.
     *     @type string       $post_type    Post type to query for. Default 'page'.
     *     @type string|array $post_status  Comma-separated list or array of post statuses to include. Default 'publish'.
     *     @type string       $show_date    Whether to display the page publish or modified date for each page. Accepts
     *                                      'modified' or any other value. An empty value hides the date. Default empty.
     *     @type string       $sort_column  Comma-separated list of column names to sort the pages by. Accepts 'post_author',
     *                                      'post_date', 'post_title', 'post_name', 'post_modified', 'post_modified_gmt',
     *                                      'menu_order', 'post_parent', 'ID', 'rand', or 'comment_count'. Default 'post_title'.
     *     @type string       $title_li     List heading. Passing a null or empty value will result in no heading, and the list
     *                                      will not be wrapped with unordered list `<ul>` tags. Default 'Pages'.
     *     @type string       $item_spacing Whether to preserve whitespace within the menu's HTML. Accepts 'preserve' or 'discard'.
     *                                      Default 'preserve'.
     *     @type Walker       $walker       Walker instance to use for listing pages. Default empty which results in a
     *                                      Walker_Page instance being used.
     * }
     * @return void|string Void if 'echo' argument is true, HTML list of pages if 'echo' is false.
     * @phpstan-param array{
     *   child_of?: int,
     *   authors?: string,
     *   date_format?: string,
     *   depth?: int,
     *   echo?: bool,
     *   exclude?: string,
     *   include?: array,
     *   link_after?: string,
     *   link_before?: string,
     *   post_type?: string,
     *   post_status?: string|array,
     *   show_date?: string,
     *   sort_column?: string,
     *   title_li?: string,
     *   item_spacing?: string,
     *   walker?: Walker,
     *   sort_order?: string,
     *   hierarchical?: bool,
     *   meta_key?: string,
     *   meta_value?: string,
     *   parent?: int,
     *   exclude_tree?: string|int[],
     *   number?: int,
     *   offset?: int,
     * } $args
     * @phpstan-return ($args is array{echo: false, ...} ? string : void)
     */
    function wp_list_pages($args = '')
    {
    }
    /**
     * Retrieves post data given a post ID or post object.
     *
     * See sanitize_post() for optional $filter values. Also, the parameter
     * `$post`, must be given as a variable, since it is passed by reference.
     *
     * @since 1.5.1
     *
     * @global WP_Post $post Global post object.
     *
     * @param int|object|null  $post   Optional. Post ID or post object. `null`, `false`, `0` and other PHP falsey values
     *                                 return the current global post inside the loop. A numerically valid post ID that
     *                                 points to a non-existent post returns `null`. Defaults to global $post.
     * @param string           $output Optional. The required return type. One of OBJECT, ARRAY_A, or ARRAY_N, which
     *                                 correspond to a WP_Post object, an associative array, or a numeric array,
     *                                 respectively. Default OBJECT.
     * @param string           $filter Optional. Type of filter to apply. Accepts 'raw', 'edit', 'db',
     *                                 or 'display'. Default 'raw'.
     * @return WP_Post|array|null Type corresponding to $output on success or null on failure.
     *                            When $output is OBJECT, a `WP_Post` instance is returned.
     *
     * @phpstan-param int|numeric-string|WP_Post|null $post
     * @phpstan-param 'OBJECT'|'ARRAY_A'|'ARRAY_N' $output
     * @phpstan-param 'raw'|'edit'|'db'|'display' $filter
     * @phpstan-return (
     *     $output is 'ARRAY_A' ? non-empty-array<string, mixed>|null : (
     *         $output is 'ARRAY_N' ? non-empty-array<int, mixed>|null : (
     *             WP_Post|null
     *         )
     *     )
     * )
     */
    function get_post($post = \null, $output = \OBJECT, $filter = 'raw')
    {
    }
    /**
     * Gets a list of post statuses.
     *
     * @since 3.0.0
     *
     * @see register_post_status()
     *
     * @global stdClass[] $wp_post_statuses List of post statuses.
     *
     * @param array|string $args     Optional. Array or string of post status arguments to compare against
     *                               properties of the global `$wp_post_statuses objects`. Default empty array.
     * @param string       $output   Optional. The type of output to return, either 'names' or 'objects'. Default 'names'.
     * @param string       $operator Optional. The logical operation to perform. 'or' means only one element
     *                               from the array needs to match; 'and' means all elements must match.
     *                               Default 'and'.
     * @return string[]|stdClass[] A list of post status names or objects.
     * @phpstan-param 'names'|'objects' $output
     * @phpstan-return ($output is 'names' ? array<string, string> : array<string, \stdClass>)
     */
    function get_post_stati($args = array(), $output = 'names', $operator = 'and')
    {
    }
    /**
     * Gets a list of all registered post type objects.
     *
     * @since 2.9.0
     *
     * @see register_post_type() for accepted arguments.
     *
     * @global array $wp_post_types List of post types.
     *
     * @param array|string $args     Optional. An array of key => value arguments to match against
     *                               the post type objects. Default empty array.
     * @param string       $output   Optional. The type of output to return. Either 'names'
     *                               or 'objects'. Default 'names'.
     * @param string       $operator Optional. The logical operation to perform. 'or' means only one
     *                               element from the array needs to match; 'and' means all elements
     *                               must match; 'not' means no elements may match. Default 'and'.
     * @return string[]|WP_Post_Type[] An array of post type names or objects.
     * @phpstan-return ( $output is 'names' ? string[] : WP_Post_Type[] )
     * @phpstan-param 'names'|'objects' $output
     * @phpstan-return ($output is 'names' ? array<string, string> : array<string, \WP_Post_Type>)
     */
    function get_post_types($args = array(), $output = 'names', $operator = 'and')
    {
    }
    /**
     * Retrieves an array of the latest posts, or posts matching the given criteria.
     *
     * For more information on the accepted arguments, see the
     * {@link https://developer.wordpress.org/reference/classes/wp_query/
     * WP_Query} documentation in the Developer Handbook.
     *
     * The `$ignore_sticky_posts` and `$no_found_rows` arguments are ignored by
     * this function and both are set to `true`.
     *
     * The defaults are as follows:
     *
     * @since 1.2.0
     *
     * @see WP_Query
     * @see WP_Query::parse_query()
     *
     * @param array|string $args {
     *     Optional. Array or query string of arguments to retrieve posts.
     *     See WP_Query::parse_query() for all available arguments.
     *
     *     @type int        $numberposts      Total number of posts to retrieve. Is an alias of `$posts_per_page`
     *                                        in WP_Query. Accepts -1 for all. Default 5.
     *     @type int|string $category         Category ID or comma-separated list of IDs (this or any children).
     *                                        Is an alias of `$cat` in WP_Query. Default 0.
     *     @type int[]      $include          An array of post IDs to retrieve, sticky posts will be included.
     *                                        Is an alias of `$post__in` in WP_Query. Default empty array.
     *     @type int[]      $exclude          An array of post IDs not to retrieve. Default empty array.
     *     @type bool       $suppress_filters Whether to suppress filters. Default true.
     * }
     * @return WP_Post[]|int[] Array of post objects or post IDs.
     *
     * @phpstan-return (
     *     $args is array{ fields: 'ids', ... } ? int[] : WP_Post[]
     * )
     * @phpstan-param array{
     *   numberposts?: int,
     *   category?: int|string,
     *   include?: int[],
     *   exclude?: int[],
     *   suppress_filters?: bool,
     *   attachment_id?: int,
     *   author?: int|string,
     *   author_name?: string,
     *   author__in?: int[],
     *   author__not_in?: int[],
     *   cache_results?: bool,
     *   cat?: int|string,
     *   category__and?: int[],
     *   category__in?: int[],
     *   category__not_in?: int[],
     *   category_name?: string,
     *   comment_count?: array|int,
     *   comment_status?: string,
     *   comments_per_page?: int,
     *   date_query?: array,
     *   day?: int,
     *   exact?: bool,
     *   fields?: string,
     *   hour?: int,
     *   ignore_sticky_posts?: int|bool,
     *   m?: int,
     *   meta_key?: string|string[],
     *   meta_value?: string|string[],
     *   meta_compare?: string,
     *   meta_compare_key?: string,
     *   meta_type?: string,
     *   meta_type_key?: string,
     *   meta_query?: array,
     *   menu_order?: int,
     *   minute?: int,
     *   monthnum?: int,
     *   name?: string,
     *   nopaging?: bool,
     *   no_found_rows?: bool,
     *   offset?: int,
     *   order?: string,
     *   orderby?: string|array,
     *   p?: int,
     *   page?: int,
     *   paged?: int,
     *   page_id?: int,
     *   pagename?: string,
     *   perm?: string,
     *   ping_status?: string,
     *   post__in?: int[],
     *   post__not_in?: int[],
     *   post_mime_type?: string,
     *   post_name__in?: string[],
     *   post_parent?: int,
     *   post_parent__in?: int[],
     *   post_parent__not_in?: int[],
     *   post_type?: string|string[],
     *   post_status?: string|string[],
     *   posts_per_page?: int,
     *   posts_per_archive_page?: int,
     *   s?: string,
     *   search_columns?: string[],
     *   second?: int,
     *   sentence?: bool,
     *   tag?: string,
     *   tag__and?: int[],
     *   tag__in?: int[],
     *   tag__not_in?: int[],
     *   tag_id?: int,
     *   tag_slug__and?: string[],
     *   tag_slug__in?: string[],
     *   tax_query?: array,
     *   title?: string,
     *   update_post_meta_cache?: bool,
     *   update_post_term_cache?: bool,
     *   update_menu_item_cache?: bool,
     *   lazy_load_term_meta?: bool,
     *   w?: int,
     *   year?: int,
     * } $args
     * @phpstan-return ($args is array{fields: 'id=>parent'|'ids', ...} ? array<int, int> : array<int, \WP_Post>)
     */
    function get_posts($args = \null)
    {
    }
    /**
     * Sanitizes a post field based on context.
     *
     * Possible context values are:  'raw', 'edit', 'db', 'display', 'attribute' and
     * 'js'. The 'display' context is used by default. 'attribute' and 'js' contexts
     * are treated like 'display' when calling filters. The 'sample' value is used
     * for permalink previewing.
     *
     * @since 2.3.0
     * @since 4.4.0 Like `sanitize_post()`, `$context` defaults to 'display'.
     *
     * @param string $field   The Post Object field name.
     * @param mixed  $value   The Post Object value.
     * @param int    $post_id Post ID.
     * @param string $context Optional. How to sanitize the field. Possible values are 'raw', 'edit', 'db', 'display',
     *                        'attribute' and 'js'. The 'sample' value is used for permalink previewing. Default 'display'.
     * @return mixed Sanitized value.
     *
     * @phpstan-param 'raw'|'edit'|'db'|'display'|'attribute'|'js'|'sample' $context
     * @phpstan-return (
     *     $field is 'ID'|'post_parent'|'menu_order' ? int : (
     *         $field is 'ancestors' ? non-negative-int[] : string
     *     )
     * )
     * @phpstan-template T of mixed
     * @phpstan-param T $value
     * @phpstan-return ($field is 'ID'|'post_parent'|'menu_order' ? (T is int ? T : int) : ($field is 'ancestors' ? (T is array<int<0, max>>|list<int<0, max>> ? T : (T is list ? list<int<0, max>> : array<int<0, max>>)) : ($context is 'raw' ? T : ($context is 'attribute'|'js' ? string : ($context is 'edit' ? ($field is not 'post_content' ? string : mixed) : mixed)))))
     */
    function sanitize_post_field($field, $value, $post_id, $context = 'display')
    {
    }
    /**
     * Retrieves the list of categories for a post.
     *
     * Compatibility layer for themes and plugins. Also an easy layer of abstraction
     * away from the complexity of the taxonomy layer.
     *
     * @since 2.1.0
     *
     * @see wp_get_object_terms()
     *
     * @param int   $post_id Optional. The Post ID. Does not default to the ID of the
     *                       global $post. Default 0.
     * @param array $args    Optional. Category query parameters. Default empty array.
     *                       See WP_Term_Query::__construct() for supported arguments.
     * @return array|WP_Error List of categories. If the `$fields` argument passed via `$args` is 'all' or
     *                        'all_with_object_id', an array of WP_Term objects will be returned. If `$fields`
     *                        is 'ids', an array of category IDs. If `$fields` is 'names', an array of category names.
     *                        WP_Error object if 'category' taxonomy doesn't exist.
     * @phpstan-param array{
     *   taxonomy?: string|string[],
     *   object_ids?: int|int[],
     *   orderby?: string,
     *   order?: string,
     *   hide_empty?: bool|int,
     *   include?: int[]|string,
     *   exclude?: int[]|string,
     *   exclude_tree?: int[]|string,
     *   number?: int|string,
     *   offset?: int,
     *   fields?: string,
     *   name?: string|string[],
     *   slug?: string|string[],
     *   term_taxonomy_id?: int|int[],
     *   hierarchical?: bool,
     *   search?: string,
     *   name__like?: string,
     *   description__like?: string,
     *   pad_counts?: bool,
     *   get?: string,
     *   child_of?: int,
     *   parent?: int,
     *   childless?: bool,
     *   cache_domain?: string,
     *   cache_results?: bool,
     *   update_term_meta_cache?: bool,
     *   meta_key?: string|string[],
     *   meta_value?: string|string[],
     *   meta_compare?: string,
     *   meta_compare_key?: string,
     *   meta_type?: string,
     *   meta_type_key?: string,
     *   meta_query?: array,
     * } $args See WP_Term_Query::__construct()
     * @phpstan-return ($post_id is 0 ? array{} : (($args is array{fields: 'names'|'slugs', ...} ? list<string> : ($args is array{fields: 'id=>name'|'id=>slug', ...} ? array<int, string> : ($args is array{fields: 'id=>parent', ...} ? array<int, int> : ($args is array{fields: 'all'|'all_with_object_id', ...} ? array<int, \WP_Term> : ($args is array{fields: 'count', ...} ? numeric-string : list<int>)))))|\WP_Error))
     */
    function wp_get_post_categories($post_id = 0, $args = array())
    {
    }
    /**
     * Retrieves the tags for a post.
     *
     * There is only one default for this function, called 'fields' and by default
     * is set to 'all'. There are other defaults that can be overridden in
     * wp_get_object_terms().
     *
     * @since 2.3.0
     *
     * @param int   $post_id Optional. The Post ID. Does not default to the ID of the
     *                       global $post. Default 0.
     * @param array $args    Optional. Tag query parameters. Default empty array.
     *                       See WP_Term_Query::__construct() for supported arguments.
     * @return array|WP_Error Array of WP_Term objects on success or empty array if no tags were found.
     *                        WP_Error object if 'post_tag' taxonomy doesn't exist.
     * @phpstan-param array{
     *   taxonomy?: string|string[],
     *   object_ids?: int|int[],
     *   orderby?: string,
     *   order?: string,
     *   hide_empty?: bool|int,
     *   include?: int[]|string,
     *   exclude?: int[]|string,
     *   exclude_tree?: int[]|string,
     *   number?: int|string,
     *   offset?: int,
     *   fields?: string,
     *   name?: string|string[],
     *   slug?: string|string[],
     *   term_taxonomy_id?: int|int[],
     *   hierarchical?: bool,
     *   search?: string,
     *   name__like?: string,
     *   description__like?: string,
     *   pad_counts?: bool,
     *   get?: string,
     *   child_of?: int,
     *   parent?: int,
     *   childless?: bool,
     *   cache_domain?: string,
     *   cache_results?: bool,
     *   update_term_meta_cache?: bool,
     *   meta_key?: string|string[],
     *   meta_value?: string|string[],
     *   meta_compare?: string,
     *   meta_compare_key?: string,
     *   meta_type?: string,
     *   meta_type_key?: string,
     *   meta_query?: array,
     * } $args See WP_Term_Query::__construct()
     * @phpstan-return ($post_id is 0 ? array{} : (($args is array{fields: 'names'|'slugs', ...} ? list<string> : ($args is array{fields: 'id=>name'|'id=>slug', ...} ? array<int, string> : ($args is array{fields: 'id=>parent', ...} ? array<int, int> : ($args is array{fields: 'ids'|'tt_ids', ...} ? list<int> : ($args is array{fields: 'count', ...} ? numeric-string : array<int, \WP_Term>)))))|\WP_Error))
     */
    function wp_get_post_tags($post_id = 0, $args = array())
    {
    }
    /**
     * Retrieves the terms for a post.
     *
     * @since 2.8.0
     *
     * @param int             $post_id  Optional. The Post ID. Does not default to the ID of the
     *                                  global $post. Default 0.
     * @param string|string[] $taxonomy Optional. The taxonomy slug or array of slugs for which
     *                                  to retrieve terms. Default 'post_tag'.
     * @param array           $args     {
     *     Optional. Term query parameters. See WP_Term_Query::__construct() for supported arguments.
     *
     *     @type string $fields Term fields to retrieve. Default 'all'.
     * }
     * @return array|WP_Error Array of WP_Term objects on success or empty array if no terms were found.
     *                        WP_Error object if `$taxonomy` doesn't exist.
     * @phpstan-param array{
     *   fields?: string,
     *   taxonomy?: string|string[],
     *   object_ids?: int|int[],
     *   orderby?: string,
     *   order?: string,
     *   hide_empty?: bool|int,
     *   include?: int[]|string,
     *   exclude?: int[]|string,
     *   exclude_tree?: int[]|string,
     *   number?: int|string,
     *   offset?: int,
     *   name?: string|string[],
     *   slug?: string|string[],
     *   term_taxonomy_id?: int|int[],
     *   hierarchical?: bool,
     *   search?: string,
     *   name__like?: string,
     *   description__like?: string,
     *   pad_counts?: bool,
     *   get?: string,
     *   child_of?: int,
     *   parent?: int,
     *   childless?: bool,
     *   cache_domain?: string,
     *   cache_results?: bool,
     *   update_term_meta_cache?: bool,
     *   meta_key?: string|string[],
     *   meta_value?: string|string[],
     *   meta_compare?: string,
     *   meta_compare_key?: string,
     *   meta_type?: string,
     *   meta_type_key?: string,
     *   meta_query?: array,
     * } $args
     * @phpstan-return ($post_id is 0 ? array{} : ($taxonomy is empty ? array{} : (($args is array{fields: 'names'|'slugs', ...} ? list<string> : ($args is array{fields: 'id=>name'|'id=>slug', ...} ? array<int, string> : ($args is array{fields: 'id=>parent', ...} ? array<int, int> : ($args is array{fields: 'ids'|'tt_ids', ...} ? list<int> : ($args is array{fields: 'count', ...} ? numeric-string : array<int, \WP_Term>)))))|\WP_Error)))
     */
    function wp_get_post_terms($post_id = 0, $taxonomy = 'post_tag', $args = array())
    {
    }
    /**
     * Inserts or updates a post in the database.
     *
     * If the `$postarr` parameter contains an 'ID', the corresponding post will be updated.
     * If not, a new post will be created using the values provided.
     *
     * You can set the post date manually, by setting the values for 'post_date'
     * and 'post_date_gmt' keys. You can close the comments or open the comments by
     * setting the value for 'comment_status' key.
     *
     * @since 1.0.0
     * @since 2.6.0 Added the `$wp_error` parameter to allow a WP_Error to be returned on failure.
     * @since 4.2.0 Support was added for encoding emoji in the post title, content, and excerpt.
     * @since 4.4.0 A 'meta_input' array can now be passed to `$postarr` to add post meta data.
     * @since 5.6.0 Added the `$fire_after_hooks` parameter.
     *
     * @see sanitize_post()
     * @global wpdb $wpdb WordPress database abstraction object.
     *
     * @param array $postarr {
     *     An array of elements that make up a post to update or insert.
     *
     *     @type int    $ID                    The post ID. If equal to something other than 0,
     *                                         the post with that ID will be updated. Default 0.
     *     @type int    $post_author           The ID of the user who added the post. Default is
     *                                         the current user ID.
     *     @type string $post_date             The date of the post. Default is the current time.
     *     @type string $post_date_gmt         The date of the post in the GMT timezone. Default is
     *                                         the value of `$post_date`.
     *     @type string $post_content          The post content. Default empty.
     *     @type string $post_content_filtered The filtered post content. Default empty.
     *     @type string $post_title            The post title. Default empty.
     *     @type string $post_excerpt          The post excerpt. Default empty.
     *     @type string $post_status           The post status. Default 'draft'.
     *     @type string $post_type             The post type. Default 'post'.
     *     @type string $comment_status        Whether the post can accept comments. Accepts 'open' or 'closed'.
     *                                         Default is the value of 'default_comment_status' option.
     *     @type string $ping_status           Whether the post can accept pings. Accepts 'open' or 'closed'.
     *                                         Default is the value of 'default_ping_status' option.
     *     @type string $post_password         The password to access the post. Default empty.
     *     @type string $post_name             The post name. Default is the sanitized post title
     *                                         when creating a new post.
     *     @type string $to_ping               Space or carriage return-separated list of URLs to ping.
     *                                         Default empty.
     *     @type string $pinged                Space or carriage return-separated list of URLs that have
     *                                         been pinged. Default empty.
     *     @type int    $post_parent           Set this for the post it belongs to, if any. Default 0.
     *     @type int    $menu_order            The order the post should be displayed in. Default 0.
     *     @type string $post_mime_type        The mime type of the post. Default empty.
     *     @type string $guid                  Global Unique ID for referencing the post. Default empty.
     *     @type int    $import_id             The post ID to be used when inserting a new post.
     *                                         If specified, must not match any existing post ID. Default 0.
     *     @type int[]  $post_category         Array of category IDs.
     *                                         Defaults to value of the 'default_category' option.
     *     @type array  $tags_input            Array of tag names, slugs, or IDs. Default empty.
     *     @type array  $tax_input             An array of taxonomy terms keyed by their taxonomy name.
     *                                         If the taxonomy is hierarchical, the term list needs to be
     *                                         either an array of term IDs or a comma-separated string of IDs.
     *                                         If the taxonomy is non-hierarchical, the term list can be an array
     *                                         that contains term names or slugs, or a comma-separated string
     *                                         of names or slugs. This is because, in hierarchical taxonomy,
     *                                         child terms can have the same names with different parent terms,
     *                                         so the only way to connect them is using ID. Default empty.
     *     @type array  $meta_input            Array of post meta values keyed by their post meta key. Default empty.
     *     @type string $page_template         Page template to use.
     * }
     * @param bool  $wp_error         Optional. Whether to return a WP_Error on failure. Default false.
     * @param bool  $fire_after_hooks Optional. Whether to fire the after insert hooks. Default true.
     * @return int|WP_Error The post ID on success. The value 0 or WP_Error on failure.
     *
     * @phpstan-return (
     *     $wp_error is false ? int : int|WP_Error
     * )
     * @phpstan-param array{
     *   ID?: int,
     *   post_author?: int,
     *   post_date?: string,
     *   post_date_gmt?: string,
     *   post_content?: string,
     *   post_content_filtered?: string,
     *   post_title?: string,
     *   post_excerpt?: string,
     *   post_status?: string,
     *   post_type?: string,
     *   comment_status?: string,
     *   ping_status?: string,
     *   post_password?: string,
     *   post_name?: string,
     *   to_ping?: string,
     *   pinged?: string,
     *   post_parent?: int,
     *   menu_order?: int,
     *   post_mime_type?: string,
     *   guid?: string,
     *   import_id?: int,
     *   post_category?: int[],
     *   tags_input?: array,
     *   tax_input?: array,
     *   meta_input?: array,
     *   page_template?: string,
     * } $postarr
     * @phpstan-return ($wp_error is false ? int<0, max> : int<1, max>|\WP_Error)
     */
    function wp_insert_post($postarr, $wp_error = \false, $fire_after_hooks = \true)
    {
    }
    /**
     * Updates a post with new post data.
     *
     * The date does not have to be set for drafts. You can set the date and it will
     * not be overridden.
     *
     * @since 1.0.0
     * @since 3.5.0 Added the `$wp_error` parameter to allow a WP_Error to be returned on failure.
     * @since 5.6.0 Added the `$fire_after_hooks` parameter.
     *
     * @param array|object $postarr          Optional. Post data. Arrays are expected to be escaped,
     *                                       objects are not. See wp_insert_post() for accepted arguments.
     *                                       Default array.
     * @param bool         $wp_error         Optional. Whether to return a WP_Error on failure. Default false.
     * @param bool         $fire_after_hooks Optional. Whether to fire the after insert hooks. Default true.
     * @return int|WP_Error The post ID on success. The value 0 or WP_Error on failure.
     *
     * @phpstan-return (
     *     $wp_error is false ? int : int|WP_Error
     * )
     * @phpstan-param array{
     *   ID?: int,
     *   post_author?: int,
     *   post_date?: string,
     *   post_date_gmt?: string,
     *   post_content?: string,
     *   post_content_filtered?: string,
     *   post_title?: string,
     *   post_excerpt?: string,
     *   post_status?: string,
     *   post_type?: string,
     *   comment_status?: string,
     *   ping_status?: string,
     *   post_password?: string,
     *   post_name?: string,
     *   to_ping?: string,
     *   pinged?: string,
     *   post_parent?: int,
     *   menu_order?: int,
     *   post_mime_type?: string,
     *   guid?: string,
     *   import_id?: int,
     *   post_category?: int[],
     *   tags_input?: array,
     *   tax_input?: array,
     *   meta_input?: array,
     *   page_template?: string,
     * } $postarr See wp_insert_post()
     * @phpstan-return ($wp_error is false ? int<0, max> : int<1, max>|\WP_Error)
     */
    function wp_update_post($postarr = array(), $wp_error = \false, $fire_after_hooks = \true)
    {
    }
    /**
     * Inserts an attachment.
     *
     * If you set the 'ID' in the $args parameter, it will mean that you are
     * updating and attempt to update the attachment. You can also set the
     * attachment name or title by setting the key 'post_name' or 'post_title'.
     *
     * You can set the dates for the attachment manually by setting the 'post_date'
     * and 'post_date_gmt' keys' values.
     *
     * By default, the comments will use the default settings for whether the
     * comments are allowed. You can close them manually or keep them open by
     * setting the value for the 'comment_status' key.
     *
     * @since 2.0.0
     * @since 4.7.0 Added the `$wp_error` parameter to allow a WP_Error to be returned on failure.
     * @since 5.6.0 Added the `$fire_after_hooks` parameter.
     *
     * @see wp_insert_post()
     *
     * @param string|array $args             Arguments for inserting an attachment.
     * @param string|false $file             Optional. Filename. Default false.
     * @param int          $parent_post_id   Optional. Parent post ID or 0 for no parent. Default 0.
     * @param bool         $wp_error         Optional. Whether to return a WP_Error on failure. Default false.
     * @param bool         $fire_after_hooks Optional. Whether to fire the after insert hooks. Default true.
     * @return int|WP_Error The attachment ID on success. The value 0 or WP_Error on failure.
     *
     * @phpstan-return (
     *     $wp_error is false ? int : int|WP_Error
     * )
     * @phpstan-return ($wp_error is false ? int<0, max> : int<1, max>|\WP_Error)
     */
    function wp_insert_attachment($args, $file = \false, $parent_post_id = 0, $wp_error = \false, $fire_after_hooks = \true)
    {
    }
    /**
     * Determines whether current WordPress query has posts to loop over.
     *
     * @since 1.5.0
     *
     * @global WP_Query $wp_query WordPress Query object.
     *
     * @return bool True if posts are available, false if end of the loop.
     * @phpstan-impure
     */
    function have_posts()
    {
    }
    /**
     * Ensures a REST response is a response object (for consistency).
     *
     * This implements WP_REST_Response, allowing usage of `set_status`/`header`/etc
     * without needing to double-check the object. Will also allow WP_Error to indicate error
     * responses, so users should immediately check for this value.
     *
     * @since 4.4.0
     *
     * @param WP_REST_Response|WP_Error|WP_HTTP_Response|mixed $response Response to check.
     * @return WP_REST_Response|WP_Error If response generated an error, WP_Error, if response
     *                                   is already an instance, WP_REST_Response, otherwise
     *                                   returns a new WP_REST_Response instance.
     * @phpstan-return ($response is \WP_Error ? \WP_Error : \WP_REST_Response)
     */
    function rest_ensure_response($response)
    {
    }
    /**
     * Returns a contextual HTTP error code for authorization failure.
     *
     * @since 4.7.0
     *
     * @return int 401 if the user is not logged in, 403 if the user is logged in.
     * @phpstan-return 401|403
     */
    function rest_authorization_required_code()
    {
    }
    /**
     * Changes a boolean-like value into the proper boolean value.
     *
     * @since 4.7.0
     *
     * @param bool|string|int $value The value being evaluated.
     * @return bool Returns the proper associated boolean value.
     * @phpstan-template T of bool|string|int
     * @phpstan-param T $value
     * @phpstan-pure
     * @phpstan-return (T is bool ? T : (T is ''|'false'|'FALSE'|'0'|0 ? false : true))
     */
    function rest_sanitize_boolean($value)
    {
    }
    /**
     * Determines if the specified post is a revision.
     *
     * @since 2.6.0
     *
     * @param int|WP_Post $post Post ID or post object.
     * @return int|false ID of revision's parent on success, false if not a revision.
     * @phpstan-return ($post is \WP_Post ? false|int<0, max> : ($post is int<min, 0> ? false : false|int<0, max>))
     */
    function wp_is_post_revision($post)
    {
    }
    /**
     * Adds a new feed type like /atom1/.
     *
     * @since 2.1.0
     *
     * @global WP_Rewrite $wp_rewrite WordPress rewrite component.
     *
     * @param string   $feedname Feed name. Should not start with '_'.
     * @param callable $callback Callback to run on feed display.
     * @return string Feed action name.
     * @phpstan-param callable(bool, string): void $callback
     * @phpstan-return non-falsy-string
     */
    function add_feed($feedname, $callback)
    {
    }
    /**
     * Adds `noindex` to the robots meta tag if required by the site configuration.
     *
     * If a blog is marked as not being public then noindex will be output to
     * tell web robots not to index the page content. Add this to the
     * {@see 'wp_robots'} filter.
     *
     * Typical usage is as a {@see 'wp_robots'} callback:
     *
     *     add_filter( 'wp_robots', 'wp_robots_noindex' );
     *
     * @since 5.7.0
     *
     * @see wp_robots_no_robots()
     *
     * @param array $robots Associative array of robots directives.
     * @return array Filtered robots directives.
     * @phpstan-param array<string, bool|string> $robots
     * @phpstan-return array<string, bool|string>
     */
    function wp_robots_noindex(array $robots)
    {
    }
    /**
     * Adds `noindex` to the robots meta tag for embeds.
     *
     * Typical usage is as a {@see 'wp_robots'} callback:
     *
     *     add_filter( 'wp_robots', 'wp_robots_noindex_embeds' );
     *
     * @since 5.7.0
     *
     * @see wp_robots_no_robots()
     *
     * @param array $robots Associative array of robots directives.
     * @return array Filtered robots directives.
     * @phpstan-param array<string, bool|string> $robots
     * @phpstan-return array<string, bool|string>
     */
    function wp_robots_noindex_embeds(array $robots)
    {
    }
    /**
     * Adds `noindex` to the robots meta tag if a search is being performed.
     *
     * If a search is being performed then noindex will be output to
     * tell web robots not to index the page content. Add this to the
     * {@see 'wp_robots'} filter.
     *
     * Typical usage is as a {@see 'wp_robots'} callback:
     *
     *     add_filter( 'wp_robots', 'wp_robots_noindex_search' );
     *
     * @since 5.7.0
     *
     * @see wp_robots_no_robots()
     *
     * @param array $robots Associative array of robots directives.
     * @return array Filtered robots directives.
     * @phpstan-param array<string, bool|string> $robots
     * @phpstan-return array<string, bool|string>
     */
    function wp_robots_noindex_search(array $robots)
    {
    }
    /**
     * Adds `noindex` to the robots meta tag.
     *
     * This directive tells web robots not to index the page content.
     *
     * Typical usage is as a {@see 'wp_robots'} callback:
     *
     *     add_filter( 'wp_robots', 'wp_robots_no_robots' );
     *
     * @since 5.7.0
     *
     * @param array $robots Associative array of robots directives.
     * @return array Filtered robots directives.
     * @phpstan-param array<string, bool|string> $robots
     * @phpstan-return array<string, bool|string>
     */
    function wp_robots_no_robots(array $robots)
    {
    }
    /**
     * Adds `noindex` and `noarchive` to the robots meta tag.
     *
     * This directive tells web robots not to index or archive the page content and
     * is recommended to be used for sensitive pages.
     *
     * Typical usage is as a {@see 'wp_robots'} callback:
     *
     *     add_filter( 'wp_robots', 'wp_robots_sensitive_page' );
     *
     * @since 5.7.0
     *
     * @param array $robots Associative array of robots directives.
     * @return array Filtered robots directives.
     * @phpstan-param array<string, bool|string> $robots
     * @phpstan-return array<string, bool|string>
     */
    function wp_robots_sensitive_page(array $robots)
    {
    }
    /**
     * Adds `max-image-preview:large` to the robots meta tag.
     *
     * This directive tells web robots that large image previews are allowed to be
     * displayed, e.g. in search engines, unless the blog is marked as not being public.
     *
     * Typical usage is as a {@see 'wp_robots'} callback:
     *
     *     add_filter( 'wp_robots', 'wp_robots_max_image_preview_large' );
     *
     * @since 5.7.0
     *
     * @param array $robots Associative array of robots directives.
     * @return array Filtered robots directives.
     * @phpstan-param array<string, bool|string> $robots
     * @phpstan-return array<string, bool|string>
     */
    function wp_robots_max_image_preview_large(array $robots)
    {
    }
    /**
     * Returns the suffix that can be used for the scripts.
     *
     * There are two suffix types, the normal one and the dev suffix.
     *
     * @since 5.0.0
     *
     * @param string $type The type of suffix to retrieve.
     * @return string The script suffix.
     * @phpstan-return ''|'.min'
     */
    function wp_scripts_get_suffix($type = '')
    {
    }
    /**
     * Formats `<script>` loader tags.
     *
     * It is possible to inject attributes in the `<script>` tag via the {@see 'wp_script_attributes'} filter.
     * Automatically injects type attribute if needed.
     *
     * @since 5.7.0
     *
     * @param array<string, string|bool> $attributes Key-value pairs representing `<script>` tag attributes.
     * @return string String containing `<script>` opening and closing tags.
     * @phpstan-param array<string, string|true> $attributes
     * @phpstan-return non-falsy-string
     */
    function wp_get_script_tag($attributes)
    {
    }
    /**
     * Constructs an inline script tag.
     *
     * It is possible to inject attributes in the `<script>` tag via the {@see 'wp_inline_script_attributes'} filter.
     *
     * If the `$data` is unsafe to embed in a `<script>` tag, an empty script tag with the provided
     * attributes will be returned. JavaScript and JSON contents can be escaped, so this is only likely
     * to be a problem with unusual content types.
     *
     * Example:
     *
     *     // The dangerous JavaScript in this example will be safely escaped.
     *     // A string with the script tag and the desired contents will be returned.
     *     wp_get_inline_script_tag( 'console.log( "</script>" );' );
     *
     *     // This data is unsafe and `text/plain` cannot be escaped.
     *     // The following will return `""` to indicate failure:
     *     wp_get_inline_script_tag( '</script>', array( 'type' => 'text/plain' ) );
     *
     * @since 5.7.0
     * @since 7.0.0 Returns an empty string if the data cannot be safely embedded in a script tag.
     *
     * @param string                     $data       Data for script tag: JavaScript, importmap, speculationrules, etc.
     * @param array<string, string|bool> $attributes Optional. Key-value pairs representing `<script>` tag attributes.
     * @return string HTML script tag containing the provided $data or the empty string `""` if the data cannot be safely embedded in a script tag.
     * @phpstan-param array<string, string|true> $attributes
     * @phpstan-return non-falsy-string
     */
    function wp_get_inline_script_tag($data, $attributes = array())
    {
    }
    /**
     * Determines whether a registered shortcode exists named $tag.
     *
     * @since 3.6.0
     *
     * @global array $shortcode_tags List of shortcode tags and their callback hooks.
     *
     * @param string $tag Shortcode tag to check.
     * @return bool Whether the given shortcode exists.
     * @phpstan-assert-if-true =non-empty-string $tag
     */
    function shortcode_exists($tag)
    {
    }
    /**
     * Determines whether the passed content contains the specified shortcode.
     *
     * @since 3.6.0
     *
     * @global array $shortcode_tags
     *
     * @param string $content Content to search for shortcodes.
     * @param string $tag     Shortcode tag to check.
     * @return bool Whether the passed content contains the given shortcode.
     * @phpstan-assert-if-true =non-falsy-string $content
     * @phpstan-assert-if-true =non-empty-string $tag
     * @phpstan-return ($tag is empty ? false : ($content is empty ? false : bool))
     */
    function has_shortcode($content, $tag)
    {
    }
    /**
     * Returns a list of registered shortcode names found in the given content.
     *
     * Example usage:
     *
     *     get_shortcode_tags_in_content( '[audio src="file.mp3"][/audio] [foo] [gallery ids="1,2,3"]' );
     *     // array( 'audio', 'gallery' )
     *
     * @since 6.3.2
     *
     * @param string $content The content to check.
     * @return string[] An array of registered shortcode names found in the content.
     * @phpstan-return list<non-empty-string>
     */
    function get_shortcode_tags_in_content($content)
    {
    }
    /**
     * Retrieves the shortcode regular expression for searching.
     *
     * The regular expression combines the shortcode tags in the regular expression
     * in a regex class.
     *
     * The regular expression contains 6 different sub matches to help with parsing.
     *
     * 1 - An extra [ to allow for escaping shortcodes with double [[]]
     * 2 - The shortcode name
     * 3 - The shortcode argument list
     * 4 - The self closing /
     * 5 - The content of a shortcode when it wraps some content.
     * 6 - An extra ] to allow for escaping shortcodes with double [[]]
     *
     * @since 2.5.0
     * @since 4.4.0 Added the `$tagnames` parameter.
     *
     * @global array $shortcode_tags
     *
     * @param array $tagnames Optional. List of shortcodes to find. Defaults to all registered shortcodes.
     * @return string The shortcode search regular expression.
     * @phpstan-return non-falsy-string
     */
    function get_shortcode_regex($tagnames = \null)
    {
    }
    /**
     * Retrieves the shortcode attributes regex.
     *
     * @since 4.4.0
     *
     * @return string The shortcode attribute regular expression.
     * @phpstan-pure
     * @phpstan-return non-falsy-string
     */
    function get_shortcode_atts_regex()
    {
    }
    /**
     * Retrieves a list of registered taxonomy names or objects.
     *
     * @since 3.0.0
     *
     * @global WP_Taxonomy[] $wp_taxonomies The registered taxonomies.
     *
     * @param array  $args     Optional. An array of `key => value` arguments to match against the taxonomy objects.
     *                         Default empty array.
     * @param string $output   Optional. The type of output to return in the array. Either 'names'
     *                         or 'objects'. Default 'names'.
     * @param string $operator Optional. The logical operation to perform. Accepts 'and' or 'or'. 'or' means only
     *                         one element from the array needs to match; 'and' means all elements must match.
     *                         Default 'and'.
     * @return string[]|WP_Taxonomy[] An array of taxonomy names or objects.
     * @phpstan-param 'names'|'objects' $output
     * @phpstan-param 'and'|'or' $operator
     * @phpstan-return ($output is 'names' ? array<string, string> : array<string, \WP_Taxonomy>)
     */
    function get_taxonomies($args = array(), $output = 'names', $operator = 'and')
    {
    }
    /**
     * Returns the names or objects of the taxonomies which are registered for the requested object or object type,
     * such as a post object or post type name.
     *
     * Example:
     *
     *     $taxonomies = get_object_taxonomies( 'post' );
     *
     * This results in:
     *
     *     Array( 'category', 'post_tag' )
     *
     * @since 2.3.0
     *
     * @global WP_Taxonomy[] $wp_taxonomies The registered taxonomies.
     *
     * @param string|string[]|WP_Post $object_type Name of the type of taxonomy object, or an object (row from posts).
     * @param string                  $output      Optional. The type of output to return in the array. Accepts either
     *                                             'names' or 'objects'. Default 'names'.
     * @return string[]|WP_Taxonomy[] The names or objects of all taxonomies of `$object_type`.
     * @phpstan-param 'names'|'objects' $output
     * @phpstan-return ($output is 'names' ? array<int, string> : array<string, \WP_Taxonomy>)
     */
    function get_object_taxonomies($object_type, $output = 'names')
    {
    }
    /**
     * Determines whether the taxonomy name exists.
     *
     * Formerly is_taxonomy(), introduced in 2.3.0.
     *
     * For more information on this and similar theme functions, check out
     * the {@link https://developer.wordpress.org/themes/basics/conditional-tags/
     * Conditional Tags} article in the Theme Developer Handbook.
     *
     * @since 3.0.0
     *
     * @global WP_Taxonomy[] $wp_taxonomies The registered taxonomies.
     *
     * @param string $taxonomy Name of taxonomy object.
     * @return bool Whether the taxonomy exists.
     * @phpstan-assert-if-true =non-falsy-string $taxonomy
     * @phpstan-return ($taxonomy is non-falsy-string ? bool : false)
     */
    function taxonomy_exists($taxonomy)
    {
    }
    /**
     * Gets all term data from database by term ID.
     *
     * The usage of the get_term function is to apply filters to a term object. It
     * is possible to get a term object from the database before applying the
     * filters.
     *
     * $term ID must be part of $taxonomy, to get from the database. Failure, might
     * be able to be captured by the hooks. Failure would be the same value as $wpdb
     * returns for the get_row method.
     *
     * There are two hooks, one is specifically for each term, named 'get_term', and
     * the second is for the taxonomy name, 'term_$taxonomy'. Both hooks gets the
     * term object, and the taxonomy name as parameters. Both hooks are expected to
     * return a term object.
     *
     * {@see 'get_term'} hook - Takes two parameters the term Object and the taxonomy name.
     * Must return term object. Used in get_term() as a catch-all filter for every
     * $term.
     *
     * {@see 'get_$taxonomy'} hook - Takes two parameters the term Object and the taxonomy
     * name. Must return term object. $taxonomy will be the taxonomy name, so for
     * example, if 'category', it would be 'get_category' as the filter name. Useful
     * for custom taxonomies or plugging into default taxonomies.
     *
     * @todo Better formatting for DocBlock
     *
     * @since 2.3.0
     * @since 4.4.0 Converted to return a WP_Term object if `$output` is `OBJECT`.
     *              The `$taxonomy` parameter was made optional.
     *
     * @see sanitize_term_field() The $context param lists the available values for get_term_by() $filter param.
     *
     * @param int|WP_Term|object $term     If integer, term data will be fetched from the database,
     *                                     or from the cache if available.
     *                                     If stdClass object (as in the results of a database query),
     *                                     will apply filters and return a `WP_Term` object with the `$term` data.
     *                                     If `WP_Term`, will return `$term`.
     * @param string             $taxonomy Optional. Taxonomy name that `$term` is part of.
     * @param string             $output   Optional. The required return type. One of OBJECT, ARRAY_A, or ARRAY_N, which
     *                                     correspond to a WP_Term object, an associative array, or a numeric array,
     *                                     respectively. Default OBJECT.
     * @param string             $filter   Optional. How to sanitize term fields. Default 'raw'.
     * @return WP_Term|array|WP_Error|null WP_Term instance (or array) on success, depending on the `$output` value.
     *                                     WP_Error if `$taxonomy` does not exist. Null for miscellaneous failure.
     * @phpstan-param 'OBJECT'|'ARRAY_A'|'ARRAY_N' $output
     * @phpstan-return ($output is 'ARRAY_A' ? array<string, string|int>|\WP_Error|null : ($output is 'ARRAY_N' ? list<string|int>|\WP_Error|null : \WP_Term|\WP_Error|null))
     */
    function get_term($term, $taxonomy = '', $output = \OBJECT, $filter = 'raw')
    {
    }
    /**
     * Gets all term data from database by term field and data.
     *
     * Warning: $value is not escaped for 'name' $field. You must do it yourself, if
     * required.
     *
     * The default $field is 'id', therefore it is possible to also use null for
     * field, but not recommended that you do so.
     *
     * If $value does not exist, the return value will be false. If $taxonomy exists
     * and $field and $value combinations exist, the term will be returned.
     *
     * This function will always return the first term that matches the `$field`-
     * `$value`-`$taxonomy` combination specified in the parameters. If your query
     * is likely to match more than one term (as is likely to be the case when
     * `$field` is 'name', for example), consider using get_terms() instead; that
     * way, you will get all matching terms, and can provide your own logic for
     * deciding which one was intended.
     *
     * @todo Better formatting for DocBlock.
     *
     * @since 2.3.0
     * @since 4.4.0 `$taxonomy` is optional if `$field` is 'term_taxonomy_id'. Converted to return
     *              a WP_Term object if `$output` is `OBJECT`.
     * @since 5.5.0 Added 'ID' as an alias of 'id' for the `$field` parameter.
     *
     * @see sanitize_term_field() The $context param lists the available values for get_term_by() $filter param.
     *
     * @param string     $field    Either 'slug', 'name', 'term_id' (or 'id', 'ID'), or 'term_taxonomy_id'.
     * @param string|int $value    Search for this term value.
     * @param string     $taxonomy Taxonomy name. Optional, if `$field` is 'term_taxonomy_id'.
     * @param string     $output   Optional. The required return type. One of OBJECT, ARRAY_A, or ARRAY_N, which
     *                             correspond to a WP_Term object, an associative array, or a numeric array,
     *                             respectively. Default OBJECT.
     * @param string     $filter   Optional. How to sanitize term fields. Default 'raw'.
     * @return WP_Term|array|false WP_Term instance (or array) on success, depending on the `$output` value.
     *                             False if `$taxonomy` does not exist or `$term` was not found.
     * @phpstan-return false|($output is 'ARRAY_A' ? array<string, string|int> : ($output is 'ARRAY_N' ? list<string|int> : \WP_Term))
     */
    function get_term_by($field, $value, $taxonomy = '', $output = \OBJECT, $filter = 'raw')
    {
    }
    /**
     * Retrieves the terms in a given taxonomy or list of taxonomies.
     *
     * You can fully inject any customizations to the query before it is sent, as
     * well as control the output with a filter.
     *
     * The return type varies depending on the value passed to `$args['fields']`. See
     * WP_Term_Query::get_terms() for details. In all cases, a `WP_Error` object will
     * be returned if an invalid taxonomy is requested.
     *
     * The {@see 'get_terms'} filter will be called when the cache has the term and will
     * pass the found term along with the array of $taxonomies and array of $args.
     * This filter is also called before the array of terms is passed and will pass
     * the array of terms, along with the $taxonomies and $args.
     *
     * The {@see 'list_terms_exclusions'} filter passes the compiled exclusions along with
     * the $args.
     *
     * The {@see 'get_terms_orderby'} filter passes the `ORDER BY` clause for the query
     * along with the $args array.
     *
     * Taxonomy or an array of taxonomies should be passed via the 'taxonomy' argument
     * in the `$args` array:
     *
     *     $terms = get_terms( array(
     *         'taxonomy'   => 'post_tag',
     *         'hide_empty' => false,
     *     ) );
     *
     * Prior to 4.5.0, taxonomy was passed as the first parameter of `get_terms()`.
     *
     * {@internal The `$deprecated` parameter is parsed for backward compatibility only.}
     *
     * @since 2.3.0
     * @since 4.2.0 Introduced 'name' and 'childless' parameters.
     * @since 4.4.0 Introduced the ability to pass 'term_id' as an alias of 'id' for the `orderby` parameter.
     *              Introduced the 'meta_query' and 'update_term_meta_cache' parameters. Converted to return
     *              a list of WP_Term objects.
     * @since 4.5.0 Changed the function signature so that the `$args` array can be provided as the first parameter.
     *              Introduced 'meta_key' and 'meta_value' parameters. Introduced the ability to order results by metadata.
     * @since 4.8.0 Introduced 'suppress_filter' parameter.
     *
     * @param array|string $args       Optional. Array or string of arguments. See WP_Term_Query::__construct()
     *                                 for information on accepted arguments. Default empty array.
     * @param array|string $deprecated Optional. Argument array, when using the legacy function parameter format.
     *                                 If present, this parameter will be interpreted as `$args`, and the first
     *                                 function parameter will be parsed as a taxonomy or array of taxonomies.
     *                                 Default empty.
     * @return WP_Term[]|int[]|string[]|string|WP_Error Array of terms, a count thereof as a numeric string,
     *                                                  or WP_Error if any of the taxonomies do not exist.
     *                                                  See the function description for more information.
     * @phpstan-param array{
     *   taxonomy?: string|string[],
     *   object_ids?: int|int[],
     *   orderby?: string,
     *   order?: string,
     *   hide_empty?: bool|int,
     *   include?: int[]|string,
     *   exclude?: int[]|string,
     *   exclude_tree?: int[]|string,
     *   number?: int|string,
     *   offset?: int,
     *   fields?: string,
     *   name?: string|string[],
     *   slug?: string|string[],
     *   term_taxonomy_id?: int|int[],
     *   hierarchical?: bool,
     *   search?: string,
     *   name__like?: string,
     *   description__like?: string,
     *   pad_counts?: bool,
     *   get?: string,
     *   child_of?: int,
     *   parent?: int,
     *   childless?: bool,
     *   cache_domain?: string,
     *   cache_results?: bool,
     *   update_term_meta_cache?: bool,
     *   meta_key?: string|string[],
     *   meta_value?: string|string[],
     *   meta_compare?: string,
     *   meta_compare_key?: string,
     *   meta_type?: string,
     *   meta_type_key?: string,
     *   meta_query?: array,
     * } $args See WP_Term_Query::__construct()
     * @phpstan-param '' $deprecated
     * @phpstan-return ($args is array{fields: 'count', ...} ? 0|numeric-string : ($args is array{fields: 'names'|'slugs', ...} ? list<string> : ($args is array{fields: 'id=>name'|'id=>slug', ...} ? array<int, string> : ($args is array{fields: 'id=>parent', ...} ? array<int, int> : ($args is array{fields: 'ids'|'tt_ids', ...} ? list<int> : array<int, \WP_Term>)))))|\WP_Error
     */
    function get_terms($args = array(), $deprecated = '')
    {
    }
    /**
     * Determines whether a taxonomy term exists.
     *
     * Formerly is_term(), introduced in 2.3.0.
     *
     * For more information on this and similar theme functions, check out
     * the {@link https://developer.wordpress.org/themes/basics/conditional-tags/
     * Conditional Tags} article in the Theme Developer Handbook.
     *
     * @since 3.0.0
     * @since 6.0.0 Converted to use `get_terms()`.
     *
     * @global bool $_wp_suspend_cache_invalidation
     *
     * @param int|string|null $term        The term to check. Accepts term ID, slug, or name.
     * @param string          $taxonomy    Optional. The taxonomy name to use.
     * @param int             $parent_term Optional. ID of parent term under which to confine the exists search.
     * @return mixed Returns null if the term does not exist.
     *               Returns the term ID if no taxonomy is specified and the term ID exists.
     *               Returns an array of the term ID and the term taxonomy ID if the taxonomy is specified and the pairing exists.
     *               Returns 0 if term ID 0 is passed to the function.
     *
     * @phpstan-return (
     *     $term is null ? null : (
     *         $term is 0 ? 0 : (
     *             $taxonomy is '' ? int|null : (
     *                 array{
     *                     term_id: numeric-string,
     *                     term_taxonomy_id: numeric-string,
     *                 }|null
     *             )
     *         )
     *     )
     * )
     * @phpstan-return ($term is 0 ? 0 : ($term is '' ? null : ($taxonomy is '' ? string|null : array{term_id: string, term_taxonomy_id: string}|null)))
     */
    function term_exists($term, $taxonomy = '', $parent_term = \null)
    {
    }
    /**
     * Sanitizes the field value in the term based on the context.
     *
     * Passing a term field value through the function should be assumed to have
     * cleansed the value for whatever context the term field is going to be used.
     *
     * If no context or an unsupported context is given, then default filters will
     * be applied.
     *
     * There are enough filters for each context to support a custom filtering
     * without creating your own filter function. Simply create a function that
     * hooks into the filter you need.
     *
     * @since 2.3.0
     *
     * @param string $field    Term field to sanitize.
     * @param string $value    Search for this term value.
     * @param int    $term_id  Term ID.
     * @param string $taxonomy Taxonomy name.
     * @param string $context  Context in which to sanitize the term field.
     *                         Accepts 'raw', 'edit', 'db', 'display', 'rss',
     *                         'attribute', or 'js'.
     * @return mixed Sanitized field.
     * @phpstan-param 'raw'|'edit'|'db'|'display'|'rss'|'attribute'|'js' $context
     * @phpstan-template T of string
     * @phpstan-param T $value
     * @phpstan-return ($field is 'parent'|'term_id'|'count'|'term_group'|'term_taxonomy_id'|'object_id' ? int<0, max> : ($context is 'raw' ? T : ($context is 'attribute'|'edit'|'js' ? string : mixed)))
     */
    function sanitize_term_field($field, $value, $term_id, $taxonomy, $context)
    {
    }
    /**
     * Counts how many terms are in taxonomy.
     *
     * Default $args is 'hide_empty' which can be 'hide_empty=true' or array('hide_empty' => true).
     *
     * {@internal The `$deprecated` parameter is parsed for backward compatibility only.}
     *
     * @since 2.3.0
     * @since 5.6.0 Changed the function signature so that the `$args` array can be provided as the first parameter.
     *
     * @param array|string $args       Optional. Array or string of arguments. See WP_Term_Query::__construct()
     *                                 for information on accepted arguments. Default empty array.
     * @param array|string $deprecated Optional. Argument array, when using the legacy function parameter format.
     *                                 If present, this parameter will be interpreted as `$args`, and the first
     *                                 function parameter will be parsed as a taxonomy or array of taxonomies.
     *                                 Default empty.
     * @return string|WP_Error Numeric string containing the number of terms in that
     *                         taxonomy or WP_Error if the taxonomy does not exist.
     * @phpstan-param array{
     *   taxonomy?: string|string[],
     *   object_ids?: int|int[],
     *   orderby?: string,
     *   order?: string,
     *   hide_empty?: bool|int,
     *   include?: int[]|string,
     *   exclude?: int[]|string,
     *   exclude_tree?: int[]|string,
     *   number?: int|string,
     *   offset?: int,
     *   fields?: string,
     *   name?: string|string[],
     *   slug?: string|string[],
     *   term_taxonomy_id?: int|int[],
     *   hierarchical?: bool,
     *   search?: string,
     *   name__like?: string,
     *   description__like?: string,
     *   pad_counts?: bool,
     *   get?: string,
     *   child_of?: int,
     *   parent?: int,
     *   childless?: bool,
     *   cache_domain?: string,
     *   cache_results?: bool,
     *   update_term_meta_cache?: bool,
     *   meta_key?: string|string[],
     *   meta_value?: string|string[],
     *   meta_compare?: string,
     *   meta_compare_key?: string,
     *   meta_type?: string,
     *   meta_type_key?: string,
     *   meta_query?: array,
     * } $args See WP_Term_Query::__construct()
     * @phpstan-param '' $deprecated
     * @phpstan-return 0|numeric-string|\WP_Error
     */
    function wp_count_terms($args = array(), $deprecated = '')
    {
    }
    /**
     * Retrieves the terms associated with the given object(s), in the supplied taxonomies.
     *
     * @since 2.3.0
     * @since 4.2.0 Added support for 'taxonomy', 'parent', and 'term_taxonomy_id' values of `$orderby`.
     *              Introduced `$parent` argument.
     * @since 4.4.0 Introduced `$meta_query` and `$update_term_meta_cache` arguments. When `$fields` is 'all' or
     *              'all_with_object_id', an array of `WP_Term` objects will be returned.
     * @since 4.7.0 Refactored to use WP_Term_Query, and to support any WP_Term_Query arguments.
     * @since 6.3.0 Passing `update_term_meta_cache` argument value false by default resulting in get_terms() to not
     *              prime the term meta cache.
     *
     * @param int|int[]       $object_ids The ID(s) of the object(s) to retrieve.
     * @param string|string[] $taxonomies The taxonomy names to retrieve terms from.
     * @param array|string    $args       See WP_Term_Query::__construct() for supported arguments.
     * @return WP_Term[]|int[]|string[]|string|WP_Error Array of terms, a count thereof as a numeric string,
     *                                                  or WP_Error if any of the taxonomies do not exist.
     *                                                  See WP_Term_Query::get_terms() for more information.
     * @phpstan-param array{
     *   taxonomy?: string|string[],
     *   object_ids?: int|int[],
     *   orderby?: string,
     *   order?: string,
     *   hide_empty?: bool|int,
     *   include?: int[]|string,
     *   exclude?: int[]|string,
     *   exclude_tree?: int[]|string,
     *   number?: int|string,
     *   offset?: int,
     *   fields?: string,
     *   name?: string|string[],
     *   slug?: string|string[],
     *   term_taxonomy_id?: int|int[],
     *   hierarchical?: bool,
     *   search?: string,
     *   name__like?: string,
     *   description__like?: string,
     *   pad_counts?: bool,
     *   get?: string,
     *   child_of?: int,
     *   parent?: int,
     *   childless?: bool,
     *   cache_domain?: string,
     *   cache_results?: bool,
     *   update_term_meta_cache?: bool,
     *   meta_key?: string|string[],
     *   meta_value?: string|string[],
     *   meta_compare?: string,
     *   meta_compare_key?: string,
     *   meta_type?: string,
     *   meta_type_key?: string,
     *   meta_query?: array,
     * } $args See WP_Term_Query::__construct()
     * @phpstan-return ($object_ids is empty ? array{} : ($taxonomies is empty ? array{} : (($args is array{fields: 'names'|'slugs', ...} ? list<string> : ($args is array{fields: 'id=>name'|'id=>slug', ...} ? array<int, string> : ($args is array{fields: 'id=>parent', ...} ? array<int, int> : ($args is array{fields: 'ids'|'tt_ids', ...} ? list<int> : ($args is array{fields: 'count', ...} ? numeric-string : array<int, \WP_Term>)))))|\WP_Error)))
     */
    function wp_get_object_terms($object_ids, $taxonomies, $args = array())
    {
    }
    /**
     * Gets the current user's ID.
     *
     * @since MU (3.0.0)
     *
     * @return int The current user's ID, or 0 if no user is logged in.
     * @phpstan-return int<0, max>
     */
    function get_current_user_id()
    {
    }
    /**
     * Retrieves user info by user ID.
     *
     * @since 6.7.0
     *
     * @param int $user_id User ID.
     *
     * @return WP_User|false WP_User object on success, false on failure.
     * @phpstan-return ($user_id is int<min, 0> ? false : \WP_User|false)
     */
    function get_user($user_id)
    {
    }
    /**
     * Creates, stores, then returns a password reset key for user.
     *
     * @since 4.4.0
     *
     * @param WP_User $user User to retrieve password reset key for.
     * @return string|WP_Error Password reset key on success. WP_Error on error.
     * @phpstan-impure
     */
    function get_password_reset_key($user)
    {
    }
    /**
     * Processes RSS feed widget data and optionally retrieve feed items.
     *
     * The feed widget can not have more than 20 items or it will reset back to the
     * default, which is 10.
     *
     * The resulting array has the feed title, feed url, feed link (from channel),
     * feed items, error (if any), and whether to show summary, author, and date.
     * All respectively in the order of the array elements.
     *
     * @since 2.5.0
     *
     * @param array $widget_rss RSS widget feed data. Expects unescaped data.
     * @param bool  $check_feed Optional. Whether to check feed for errors. Default true.
     * @return array
     * @phpstan-return array{title: string, url: string, link: string, items: int<1, 20>, error: string|false, show_summary: int, show_author: int, show_date: int}
     */
    function wp_widget_rss_process($widget_rss, $check_feed = \true)
    {
    }
}
