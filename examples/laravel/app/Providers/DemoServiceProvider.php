<?php

namespace App\Providers;

use App\Models\Bakery;
use App\Models\BlogPost;
use App\Models\Customer;
use App\Policies\PublishingPolicy;
use App\Support\BakeryService;
use App\Support\CarbonMixin;
use App\Support\CollectionMixin;
use App\Support\CroissantSupplier;
use App\Support\PastryCounter;
use App\Support\PlainOven;
use App\View\Composers\SidebarComposer;
use Carbon\CarbonImmutable;
use Illuminate\Contracts\Foundation\Application;
use Illuminate\Database\Eloquent\Relations\Relation;
use Illuminate\Filesystem\FilesystemAdapter;
use Illuminate\Support\Collection;
use Illuminate\Support\Facades\Blade;
use Illuminate\Support\Facades\Gate;
use Illuminate\Support\Facades\Storage;
use Illuminate\Support\Facades\View;
use Illuminate\View\View as ViewInstance;
use League\Flysystem\Filesystem;
use League\Flysystem\Local\LocalFilesystemAdapter;

class DemoServiceProvider extends BaseDemoServiceProvider
{
    /**
     * Laravel reads these two arrays off the provider itself and applies them
     * once register() has run, so a key listed here binds exactly as a
     * `bind()` / `singleton()` call would.  Hover either key where it is
     * resolved and PHPantom reports the class; go-to-definition jumps back to
     * the entry below.
     */
    public array $bindings = [
        'pastry.counter' => PastryCounter::class,
    ];

    public array $singletons = [
        'pastry.plain-oven' => PlainOven::class,
    ];

    public function register(): void
    {
        // A container key is not always written as a literal.  This one lives
        // in a static property on the base provider, so `static::$abstract`
        // is all the subclass writes.  PHPantom folds the property against
        // this concrete provider class, which is where the parent chain is
        // known, so `app('pastry.oven')` resolves to BakeryService.
        //
        // The provider this one extends binds the same key to PlainOven.  Two
        // providers binding one key is how an implementation gets swapped out,
        // and PHPantom follows the container: the registration that replaces
        // the other is the one the key resolves to.
        $this->app->singleton(static::$abstract, fn () => new BakeryService());

        // `alias()` takes its arguments the other way round from `bind()`:
        // the second one is the new name, the first is what it stands for.
        $this->app->alias(CroissantSupplier::class, static::$abstract . '.supplier');

        // A factory that hands back whatever something else builds says
        // nothing PHPantom can follow, but it does declare what comes out.
        // The declared return type is the author's own statement of what the
        // key holds, so that is what it resolves to.
        $this->app->singleton('pastry.tally', function (Application $app): PastryCounter {
            return $app->make('pastry.counter')->tally();
        });
    }

    public function boot(): void
    {
        // The morph map replaces the model FQCN with a short alias in every
        // polymorphic `*_type` column.  PHPantom reads the registration from
        // here, so the alias strings elsewhere in the project hover with the
        // model they stand for and go-to-definition jumps to it.
        Relation::morphMap([
            'blog_post' => BlogPost::class,
            'bakery'    => Bakery::class,
        ]);

        // An ability defined here is valid for any subject.  PHPantom reads
        // the registration, so the string completes and hovers with this
        // closure's signature wherever it is checked.
        Gate::define('manage-bakery-network', function (Customer $user, string $region): bool {
            return $user->isPremium();
        });

        // An explicit policy registration wins over the naming convention:
        // BlogPost's abilities are PublishingPolicy's public methods, not
        // those of an App\Policies\BlogPostPolicy that does not exist.
        Gate::policy(BlogPost::class, PublishingPolicy::class);

        // A macro registered here becomes a real method on Collection:
        // it autocompletes, hovers with this signature, and type-checks.
        Collection::macro('sumField', function (string $field): float {
            return $this->sum($field);
        });

        // A mixin registers one macro per public method of the given object,
        // each taking the signature of the closure that method returns.
        // PHPantom recovers those from CollectionMixin's source.
        Collection::mixin(new CollectionMixin());

        // Carbon supports the same `macro()` pattern as Laravel's Macroable.
        // The closure is bound with the target as scope, so `self::`/`static::`
        // refer to CarbonImmutable and protected helpers like `self::this()`
        // (the instance the macro is called on) resolve:
        CarbonImmutable::macro('diffFromYear', function (int $year, bool $absolute = false): string {
            return self::this()->diffForHumans(
                CarbonImmutable::create($year, 1, 1),
                ['syntax' => \Carbon\CarbonInterface::DIFF_ABSOLUTE]
            );
        });

        // Carbon also supports trait-based mixins (since Carbon 2.23.0):
        // each public method of the trait becomes a method on the target,
        // using the trait method's own signature directly.
        CarbonImmutable::mixin(CarbonMixin::class);

        // `View::share()` puts a variable in *every* template's scope, and a
        // view composer puts one in the scope of the views its registration
        // targets.  Neither is written in a template or passed by any `view()`
        // call, so this provider is the only record of them: PHPantom reads
        // both from here and resolves the type from the expression itself.
        View::share('siteName', config('app.name'));
        View::share('supplier', app(CroissantSupplier::class));

        // Every view under `partials.` gets what SidebarComposer::compose()
        // writes.  See resources/views/partials/sidebar.blade.php.
        View::composer('partials.*', SidebarComposer::class);

        // A composer written inline declares its data the same way.
        View::composer('emails.*', function (ViewInstance $view) {
            $view->with('mailFooter', __('messages.welcome'));
        });

        // An anonymous component namespace points a tag prefix at a directory
        // of class-less component views: `<x-widgets::badge>` renders the
        // plain view `components.widgets.badge`.  Nothing about that view's
        // name mentions the prefix, so this registration is the only record
        // of the pairing, and without it the attributes those tags pass would
        // reach no template at all.  See
        // resources/views/components/widgets/badge.blade.php.
        Blade::anonymousComponentNamespace('components/widgets', 'widgets');

        // `Blade::directive()` declares a directive Blade itself knows
        // nothing about, so this provider is the only record that `@priceTag`
        // exists at all.  PHPantom reads the registration: the name completes
        // after an `@` in a template, and the expression the directive is
        // handed stays real PHP that is type-checked rather than being masked
        // as markup.  See resources/views/welcome.blade.php.
        Blade::directive('priceTag', function (string $expression): string {
            return "<?php echo number_format({$expression}, 2); ?>";
        });

        // `Blade::if()` is four directives rather than one: Blade synthesizes
        // `@bakeryOpen`, `@unlessbakeryOpen`, `@elsebakeryOpen` and
        // `@endbakeryOpen` from this single name, and PHPantom expands the
        // family the same way.
        Blade::if('bakeryOpen', function (Bakery $bakery): bool {
            return $bakery->baguettes() !== null;
        });

        // A custom disk driver is bound here rather than in the framework, so
        // the type it builds is only knowable from this closure.  PHPantom
        // reads it and folds the 'pantry' disk in config/filesystems.php into
        // the type Storage::disk() resolves to.
        Storage::extend('pantry', function ($app, array $config) {
            $adapter = new LocalFilesystemAdapter($config['root']);

            return new FilesystemAdapter(new Filesystem($adapter), $adapter, $config);
        });
    }
}
