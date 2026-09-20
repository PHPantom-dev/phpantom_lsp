<?php
/**
 * Laravel Demo Classes for PHPantom LSP
 *
 * Open any method and trigger completion inside it.
 * Requires a real Laravel installation via `composer install`.
 */

namespace App;

use App\Facades\Oven;
use App\Facades\PastryOven;
use App\Http\Controllers\BakeryController;
use App\Mail\OrderShipped;
use App\Http\Requests\StoreBakeryRequest;
use App\Http\Requests\UpdateBakeryRequest;
use App\Models\Baker;
use App\Models\Bakery;
use App\Models\BlogAuthor;
use App\Models\BlogPost;
use App\Models\Customer;
use App\Models\Loaf;
use App\Models\PostCollection;
use App\Models\Review;
use App\Models\ReviewCollection;
use Database\Factories\AnnotatedPostFactory;
use Database\Factories\BlogAuthorFactory;
use Database\Factories\EditorialFactory;
use Illuminate\Container\Attributes\Auth as InjectAuth;
use Illuminate\Container\Attributes\Authenticated as InjectAuthenticated;
use Illuminate\Container\Attributes\Cache as InjectCache;
use Illuminate\Container\Attributes\Database as InjectDatabase;
use Illuminate\Container\Attributes\Log as InjectLog;
use Illuminate\Container\Attributes\Storage as InjectStorage;
use Illuminate\Contracts\Filesystem\Filesystem;
use Illuminate\Database\Eloquent\Relations\Relation;
use Illuminate\Http\Client\Factory as HttpFactory;
use Illuminate\Http\Client\PendingRequest;
use Illuminate\Http\Request;
use Carbon\CarbonImmutable;
use Illuminate\Support\Collection;
use Illuminate\Support\Env;
use Illuminate\Support\Facades\App;
use Illuminate\Support\Facades\Artisan;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Broadcast;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\Config;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Gate;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Lang;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Mail;
use Illuminate\Support\Facades\Queue;
use Illuminate\Support\Facades\Redirect;
use Illuminate\Support\Facades\Redis;
use Illuminate\Support\Facades\Response;
use Illuminate\Support\Facades\Route;
use Illuminate\Support\Facades\Schedule;
use Illuminate\Support\Facades\Storage;
use Illuminate\Support\Facades\URL;
use Illuminate\Support\Facades\View;
use Illuminate\View\Factory as ViewFactory;

class Demo
{
    // ── Eloquent Virtual Properties ─────────────────────────────────────────
    // Alphabetical — every property a through w should appear in order.
    // Trigger completion on `$bakery->` and scan the list.

    public function eloquentProperty(): void
    {
        $bakery = new Bakery();

        $bakery->apricot;             // $casts 'boolean'           → bool
        $bakery->baguettes;           // relationship HasMany       → Collection<Loaf>
        $bakery->baguettes_count;     // relationship count         → int
        $bakery->croissant;           // $attributes default        → string
        $bakery->defrosted_at;        // $dates (deprecated)        → Carbon\Carbon
        $bakery->dough_temp;          // $casts 'float'             → float
        $bakery->egg_count;           // $attributes default        → int
        $bakery->flour;               // $fillable (no cast/attr)   → mixed
        $bakery->freshlyBaked();      // #[Scope] attribute method  → Builder
        $bakery->gluten_free;         // $attributes default        → bool
        $bakery->headBaker;           // relationship HasOne        → Baker
        $bakery->head_baker_count;    // relationship count         → int
        $bakery->icing;               // $casts custom class        → ?Frosting
        $bakery->id;                  // implicit primary key       → int
        $bakery->jam_flavor;          // $casts enum                → JamFlavor
        $bakery->kitchen_id;          // $guarded (no cast/attr)    → mixed
        $bakery->loaf_name;           // legacy accessor            → string
        $bakery->masterRecipe;        // relationship BelongsToMany → Collection<BakeryRecipe>
        $bakery->master_recipe_count; // relationship count         → int
        $bakery->notes;               // $casts 'array'             → array
        $bakery->oven_code;           // $hidden (no cast/attr)     → mixed
        $bakery->proved_at;           // $casts 'datetime'          → \Carbon\Carbon
        $bakery->quality;             // casts() method 'float'     → float
        $bakery->rye_blend;           // $visible (no cast/attr)    → mixed
        $bakery->sprinkle;            // modern accessor Attribute  → string
        $bakery->topping('choc');     // scope method               → Builder
        $bakery->unbaked();           // scope method               → Builder
        $bakery->vendor;              // body-inferred morphTo      → Model
        $bakery->vendor_count;        // relationship count         → int
        $bakery->warmth;              // $appends (no cast/attr)    → mixed
        // MUST NOT appear: secret_ingredient (private $attributes field)

        // Relation properties resolve case-insensitively, matching Laravel's
        // magic accessor, which dispatches relations through a
        // case-insensitive method lookup. Both spellings resolve to the same
        // relationship, so a differently-cased access is not flagged.
        $bakery->headbaker->getName(); // HasOne (lower-case)       → Baker
        $bakery->MasterRecipe;         // BelongsToMany (mixed)     → Collection<BakeryRecipe>

        // Pivot accessors are attached to models that are the *target* of a
        // many-to-many relationship (belongsToMany/morphToMany). BakeryRecipe
        // is the target of Bakery::masterRecipe(), which declares a custom
        // pivot via ->using(RecipeIngredient::class), so its $pivot is typed as
        // that class. When the relationship annotation carries a third generic
        // (BelongsToMany<Related, $this, Pivot>), that type is used instead.
        $bakery->masterRecipe->first()->pivot;                      // custom pivot → RecipeIngredient
        $bakery->masterRecipe->first()->pivot->getQuantityLabel(); // pivot method → string
        // Hover over masterRecipe() also lists the ->withPivot() columns.

        // ->as('ingredient') renames the accessor, so the same pivot arrives as
        // $ingredient rather than $pivot. Bakery::seasonalRecipes() configures
        // it in the body and repeats it in the annotation's fourth generic
        // (BelongsToMany<Related, $this, Pivot, 'accessor'>); either alone is
        // enough. BakeryRecipe is the target of both relationships, so it
        // carries both accessors.
        $bakery->seasonalRecipes->first()->ingredient;                      // renamed pivot → RecipeIngredient
        $bakery->seasonalRecipes->first()->ingredient->getQuantityLabel(); // pivot method  → string
        // Hover over seasonalRecipes() also names the accessor it configures.

        // BelongsTo relationship property + method call with covariant $this
        $post = new BlogPost();
        $post->author;                // relationship BelongsTo     → BlogAuthor
        $post->author()->associate($post->author); // associate() on BelongsTo
    }


    // ── Eloquent Query Builder ──────────────────────────────────────────────

    public function eloquentQuery(): void
    {
        // Builder-as-static forwarding
        BlogAuthor::where('active', true);
        BlogAuthor::where('active', 1)->get();     // → Collection<BlogAuthor>
        BlogAuthor::where('active', 1)->first();   // → BlogAuthor|null
        BlogAuthor::orderBy('name')->limit(10)->get();
        BlogAuthor::whereIn('id', [1, 2])->groupBy('genre')->get();
        BlogAuthor::where('active', 1)->first()->profile->getBio();

        // Model @method tags available on Builder (e.g. SoftDeletes withTrashed)
        BlogAuthor::where('active', 1)->withTrashed()->first();
        BlogAuthor::groupBy('genre')->onlyTrashed()->get();

        // Scope methods — instance and static
        $author = new BlogAuthor();
        $author->active();
        $author->ofGenre('fiction');
        BlogAuthor::active();
        BlogAuthor::ofGenre('fiction');

        // Scopes on Builder instances (convention and #[Scope] attribute)
        BlogAuthor::where('active', 1)->active()->ofGenre('sci-fi')->get();
        Bakery::where('open', true)->freshlyBaked()->get();
        $query = BlogAuthor::where('genre', 'fiction');
        $query->active();
        $query->orderBy('name')->get();

        // where{PropertyName}() dynamic methods (from $fillable, $casts, etc.)
        Bakery::whereFlour('whole wheat');           // from $fillable
        Bakery::whereApricot(true);                  // from $casts
        Bakery::whereDefrostedAt('2024-01-01');      // from $dates
        Bakery::whereCroissant('almond');             // from $attributes
        Bakery::whereKitchenId(42);                   // from $guarded
        Bakery::whereOvenCode('X9');                  // from $hidden
        Bakery::whereFlour('rye')->whereApricot(true)->get();
        Bakery::where('open', true)->whereFlour('spelt')->freshlyBaked()->first();

        // Conditionable when()/unless() chain continuation
        BlogAuthor::where('active', 1)->when(true, fn($q) => $q)->get();
        BlogAuthor::where('active', 1)->unless(false, fn($q) => $q)->first();

        // Custom builders keep the model they were built for, whether or
        // not the builder class declares generics. LoafBuilder declares
        // none (see app/Models/LoafBuilder.php), BakerBuilder does.
        Loaf::query()->stale()->where('crust', 'sourdough')->firstOrFail()->getWeight();
        Loaf::query()->whereKey(1)->first()->getWeight();     // → Loaf|null
        Loaf::query()->stale()->get();                        // → Collection<Loaf>
        Baker::query()->active()->firstOrFail()->getName();   // → Baker

        // Paginators carry the model element type through foreach
        foreach (BlogAuthor::where('active', 1)->paginate() as $author) {
            $author->profile->getBio();       // → BlogAuthor
        }
        foreach (BlogAuthor::simplePaginate() as $author) {
            $author->posts();                 // → BlogAuthor
        }
        foreach (BlogAuthor::cursorPaginate() as $author) {
            $author->ofGenre('fiction');      // → BlogAuthor
        }
    }


    // ── Model Factories (has*/for*/trashed dynamic methods) ─────────────────
    // Laravel's Factory::__call() resolves has{Relationship}(),
    // for{Relationship}(), and trashed() at runtime. PHPantom derives the
    // model from the concrete factory's naming convention (BlogAuthorFactory
    // → App\Models\BlogAuthor), even through the shared BaseFactory parent.
    // A factory's explicit $model property takes priority when its name is not
    // conventional. PHPantom synthesizes the dynamic methods from whichever
    // model the factory names. Each returns the factory (static), so the fluent
    // chain continues into the build methods, which return the model.

    public function factories(): void
    {
        // Single-model build methods return the associated model.
        BlogAuthor::factory()->create();                 // → BlogAuthor
        BlogAuthor::factory()->make()->displayName;      // make() → BlogAuthor
        BlogAuthor::factory()->makeOne()->displayName;   // makeOne() → BlogAuthor

        // EditorialFactory does not match the model's name. Its protected
        // $model property takes precedence over the naming convention.
        EditorialFactory::new()->makeOne()->displayName;         // → BlogAuthor
        EditorialFactory::new()->count(2)->make()->emails();     // → AuthorCollection

        // has{Relationship}() — one per relationship on the model.
        BlogAuthor::factory()->hasPosts(3);         // HasMany posts   → factory
        BlogAuthor::factory()->hasProfile();        // HasOne profile  → factory

        // The chain stays on the factory, so create() still returns the model.
        // hasPosts(3) counts *related* models, not the ones being built.
        BlogAuthor::factory()->hasPosts(3)->create();            // → BlogAuthor

        // for{Relationship}() — for the inverse (BelongsTo) side.
        BlogPost::factory()->forAuthor(['name' => 'Ada']);   // BelongsTo author → factory
        BlogPost::factory()->forAuthor()->create();          // → BlogPost

        // trashed() is synthesized only because BlogPost uses SoftDeletes.
        BlogPost::factory()->trashed();                        // → factory
        BlogPost::factory()->forAuthor()->trashed()->create(); // → BlogPost

        // AnnotatedPostFactory carries `@extends Factory<BlogPost>`, the
        // shape `make:factory` generates. The generic binding resolves
        // create()/make() on its own; forAuthor()/trashed() still appear
        // because they come from BlogPost's relationships and SoftDeletes
        // trait, not from the generics system.
        AnnotatedPostFactory::new()->forAuthor()->create();     // → BlogPost
        AnnotatedPostFactory::new()->trashed()->make();         // → BlogPost
    }

    // The declared return type is the check: makeOne() is inherited through
    // BaseFactory, and it has to come back as BlogAuthor rather than the base
    // Model for this to return without a type error.
    protected function makeOneAuthor(BlogAuthor $author): BlogAuthor
    {
        return BlogAuthor::factory()->makeOne(
            $author->only(['name', 'email']),
        );
    }


    // ── Factory count state ─────────────────────────────────────────────────
    // create()/make() build one model, or a Collection of them once the
    // chain sets a count — through count(), times(), or an integer argument
    // to factory(). count(null) clears it again.

    public function factoryCounts(): void
    {
        // No count → a single model.
        BlogAuthor::factory()->create()->displayName;         // → BlogAuthor
        BlogAuthor::factory(['name' => 'Ada'])->make();       // array is state → BlogAuthor

        // Count set → a collection of them.  BlogAuthor declares
        // #[CollectedBy(AuthorCollection::class)], so that is what comes back.
        BlogAuthor::factory(3)->create()->first();            // → BlogAuthor|null
        BlogAuthor::factory()->count(2)->create()->emails();  // → array<string>
        BlogAuthor::factory()->times(2)->make()->byName();    // → AuthorCollection
        BlogAuthorFactory::times(2)->create()->active();      // → AuthorCollection

        // The count carries past intervening calls, and count(null) clears it.
        BlogAuthor::factory()->count(2)->hasPosts(3)->create()->first(); // → BlogAuthor|null
        BlogAuthor::factory(3)->count(null)->create()->displayName;      // → BlogAuthor

        foreach (BlogAuthor::factory(3)->create() as $author) {
            $author->displayName;                             // → BlogAuthor
        }
    }


    // ── Factory count through a variable ────────────────────────────────────
    // The count travels with the factory, so a chain built up over several
    // statements still knows what its create() builds.

    public function factoryInAVariable(int $count): void
    {
        $factory = BlogAuthor::factory();
        $factory = $factory->hasPosts(3);
        $factory->create()->displayName;                      // → BlogAuthor

        $many = BlogAuthor::factory()->count(2);
        $many->create()->emails();                            // → AuthorCollection

        // An argument typed as a number is a count, the same as writing one.
        BlogAuthor::factory($count)->create()->first();       // → BlogAuthor|null
    }


    // ── Custom Eloquent Collections ─────────────────────────────────────────

    public function customCollection(): void
    {
        // Builder chain → custom collection via #[CollectedBy]
        $reviews = Review::where('published', true)->get();
        $top = $reviews->topRated();           // custom method from ReviewCollection
        $avg = $reviews->averageRating();       // custom method from ReviewCollection
        $reviews->first();                // inherited — returns Review|null
        echo count($top), $avg;

        // Relationship properties also use the custom collection
        $review = new Review();
        $review->replies->topRated();     // HasMany<Review> → ReviewCollection
    }

    /**
     * A builder chain is typed as the model's collection, not the base
     * `Illuminate\Database\Eloquent\Collection`, so declaring the custom
     * class as the return type checks out.
     */
    public function publishedReviews(): ReviewCollection
    {
        return Review::where('published', true)->get();
    }

    /** A self-referential `HasMany<Review, $this>` is a ReviewCollection too. */
    public function repliesTo(Review $review): ReviewCollection
    {
        return $review->replies;
    }

    /** So is the relation's own `get()`. */
    public function fetchedReplies(Review $review): ReviewCollection
    {
        return $review->replies()->get();
    }

    /**
     * `keyBy()` re-keys the collection, so the key type comes from the
     * callback instead of staying the `int` a freshly fetched collection
     * starts with.  A `static` callback binds it just the same.
     */
    public function reKeyedReviews(): void
    {
        $byTitle = Review::query()->get()
            ->keyBy(fn (Review $review): string => $review->getTitle());
        $byTitle->get('Sourdough');         // → ReviewCollection<string, Review>

        Review::query()->get()
            ->keyBy(static fn (Review $review): string => $review->getTitle())
            ->get('Sourdough');             // → ReviewCollection<string, Review>

        // Keying by a column name can only promise an array key.
        Review::query()->get()
            ->keyBy('title')
            ->get('Sourdough');             // → ReviewCollection<array-key, Review>
    }

    /**
     * `flatMap()` declares two templates and types its callback as
     * returning either a collection or an array of them, so each template
     * has to bind to its own part of that shape.  Binding both to the
     * callback's whole return type leaves the chunked wrapper in place and
     * reports the flattened result as the wrong type.
     *
     * @param  array<int, string>  $titles
     * @return array<int, string>
     */
    public function flatMappedTitles(array $titles): array
    {
        return collect($titles)
            ->chunk(10)
            ->flatMap(fn ($chunk) => $this->normaliseTitles($chunk->all()))
            ->all();                        // → array<int, string>
    }

    /**
     * @param  array<int, string>  $titles
     * @return array<int, string>
     */
    private function normaliseTitles(array $titles): array
    {
        return array_map(strtolower(...), $titles);
    }


    // ── Higher-Order Collection Proxies ─────────────────────────────────────

    /**
     * `$reviews->map->getTitle()` is shorthand for
     * `$reviews->map(fn ($r) => $r->getTitle())`, so the result is typed as
     * whatever the proxied collection method returns for that member.
     *
     * @param  Collection<int, Review>  $reviews
     */
    public function higherOrderProxies(Collection $reviews): void
    {
        $reviews->map->getTitle();          // → Collection<int, string>
        $reviews->map->getRating();         // → Collection<int, int>
        $reviews->flatMap->replies;         // → Collection<array-key, Review>

        $reviews->filter->getRating();      // → Collection<int, Review>
        $reviews->reject->getRating();      // → Collection<int, Review>
        $reviews->each->getTitle();         // → Collection<int, Review>
        $reviews->sortBy->getRating();      // → Collection<int, Review>
        $reviews->unique->getTitle();       // → Collection<int, Review>

        $reviews->keyBy->getTitle();        // → Collection<array-key, Review>
        $reviews->groupBy->getTitle();      // → Collection<array-key, Collection<int, Review>>
        $reviews->partition->getRating();   // → Collection<int, Collection<int, Review>>

        $reviews->first->getRating();       // → Review|null
        $reviews->contains->getRating();    // → bool
        $reviews->every->getRating();       // → bool
        $reviews->sum->getRating();         // → int
        $reviews->avg->getRating();         // → int|float|null

        // The result is an ordinary collection, so the chain continues.
        $reviews->map->getTitle()->implode(', ');   // → string
        $reviews->filter->getRating()->count();     // → int
    }

    /**
     * The proxy remembers which collection it came from, so a method that
     * returns `static` stays on the custom collection.  Mapping to a scalar
     * cannot, because `ReviewCollection` only holds models — it falls back
     * to the base `Illuminate\Support\Collection`, exactly as Eloquent does
     * at runtime.
     */
    public function higherOrderProxyOnCustomCollection(): string
    {
        $reviews = Review::where('published', true)->get();

        $reviews->filter->getRating()->topRated();   // → ReviewCollection

        // `groupBy` is not overridden by Eloquent, so it keeps the custom
        // collection; `partition` is, with an explicit `->toBase()`.
        $reviews->groupBy->getTitle()->topRated();   // → ReviewCollection
        $reviews->partition->getRating();            // → Collection<int, ReviewCollection>

        return $reviews->map->getTitle()->implode(', ');   // → Collection<int, string>
    }


    // ── Eloquent Closure Parameter Inference ────────────────────────────────

    public function eloquentClosure(): void
    {
        // Eloquent chunk — $orders inferred as Collection
        BlogAuthor::where('active', true)->chunk(100, function ($orders) {
            $count = $orders->count();    // resolves to Eloquent Collection
            echo $count;
        });

        // Explicit bare type hint inherits inferred generic args for foreach
        BlogAuthor::where('active', true)->chunk(100, function (Collection $authors) {
            foreach ($authors as $author) {
                $author->posts();           // resolves to BlogAuthor via Collection<int, BlogAuthor>
            }
        });

        // Eloquent whereHas — $query inferred as Builder<BlogPost> (the related model)
        BlogAuthor::whereHas('posts', function ($query) {
            $query->where('published', true); // resolves to Builder<BlogPost>
        });

        // Dot-notation relation chain
        BlogPost::whereHas('author', function ($q) {
            $q->where('active', true);    // resolves to Builder<BlogAuthor>
        });
    }


    // ── Laravel Config & Env Navigation ─────────────────────────────────────

    /**
     * "Go to Definition" and "Find All References" for config keys and env vars.
     *
     * Try:
     *  1. Ctrl+Click "app.name" to jump to config/app.php.
     *  2. Ctrl+Click "app.key" to jump to config/app.php, then Ctrl+Click env('APP_KEY') to .env.
     *  3. "Find All References" on "app.name" to see all usage sites (including Blade views).
     *  4. "Find All References" on "DB_PASSWORD" to see every read of it,
     *     including the one in config/database.php and the .env line itself.
     *  5. Hover "APP_NAME" to see the value .env sets it to, and type inside
     *     an empty env('') to complete every variable the project declares.
     */
    public function laravelConfigEnv(): void
    {
        // Global helper
        config('app.name');

        // Facade methods
        Config::get('app.name');
        Config::set('app.env', 'production');

        // getMany() reads a list rather than a single key, in either of the
        // two spellings: a bare entry names the key, and a key/value entry
        // names it on the left with the default on the right.
        Config::getMany(['app.name', 'app.env' => 'production']);

        // Config keys that use env() — Ctrl+Click jumps to the config file,
        // then Ctrl+Click the env() call there to jump to .env
        config('app.key');                // uses env('APP_KEY')
        config('database.connections.mysql.password'); // uses env('DB_PASSWORD')

        // Environment variables are indexed wherever they are read, so
        // Ctrl+Click, hover, completion, and Find All References work the
        // same on both spellings of the read.
        env('APP_NAME');                  // hover shows: PHPantom
        env('DB_PASSWORD');               // a name that reads as a credential
                                          // hovers as set, without the value
        Env::get('APP_KEY');
    }


    // ── Laravel Path Helpers ────────────────────────────────────────────────

    /**
     * Path helpers anchor their argument to a conventional directory under the
     * project root, so the file each one names is navigable and the segments
     * leading to it complete from the real directory listing.
     *
     * Try:
     *  1. Ctrl+Click "routes/web.php" to jump to that file.
     *  2. Trigger completion inside `resource_path('|')` — offers the entries
     *     of resources/, directories first; pick one and keep typing to walk
     *     down into it.
     *  3. Note that `database_path('factories')` is not a link: an editor
     *     cannot open a folder as a document. Directories complete, they
     *     just do not jump.
     */
    public function laravelPathHelpers(): void
    {
        base_path('routes/web.php');              // → routes/web.php
        app_path('Models/Bakery.php');            // → app/Models/Bakery.php
        config_path('app.php');                   // → config/app.php
        lang_path('en/messages.php');             // → lang/en/messages.php
        resource_path('views/welcome.blade.php'); // → resources/views/welcome.blade.php

        // public_path() and storage_path() work the same way; this demo
        // project ships no public/ or storage/ directory to point them at.
        database_path('factories');               // a directory — completes, no link
    }


    // ── Laravel View, Route & Translation Navigation ───────────────────────

    /**
     * "Go to Definition" and "Find All References" for Laravel identifiers.
     *
     * Try:
     *  1. Ctrl+Click "welcome" to jump to resources/views/welcome.blade.php.
     *  2. Ctrl+Click "admin.users.index" to jump to the view.
     *  3. Ctrl+Click "home" to jump to the ->name('home') declaration in routes/web.php.
     *  4. Ctrl+Click "auth.failed" to jump to lang/en/auth.php.
     *  5. Ctrl+Click "theme.dashboard" to jump to a view under the custom
     *     path registered in config/view.php (resources/theme/views).
     *  6. Type a quote inside route('bakeries.show', [ … ]) to complete the
     *     route's URI parameter names.
     *  7. Do the same inside route('bakeries.ovens.show', [ … ]) — the
     *     parameters of a Route::resource() route are derived from its name.
     */
    public function laravelNavigation(): void
    {
        // Blade Views — passing typed data for in-template completion.
        // welcome.blade.php declares a @bladestan-signature, so this call
        // is checked against it the way a function call is checked against
        // its parameters.
        $posts = BlogPost::where('published', true)->get();
        $user = BlogAuthor::first();
        view('welcome', compact('posts', 'user'));

        // admin/users/index.blade.php declares $users and @extends('welcome'),
        // and a layout renders from the same data array as its child — so the
        // layout's $user and $posts are this call's to supply too.
        View::make('admin.users.index', [
            'users' => BlogAuthor::all(),
            'posts' => $posts,
            'user' => $user,
        ]);
        View::exists('emails.blog_published');

        // Response::view() renders a template straight into an HTTP
        // response, and the factory's own first() renders whichever
        // candidate exists — a candidate that names nothing is the point of
        // the shape rather than a typo. Both are checked against
        // welcome.blade.php's signature the way view() is.
        Response::view('welcome', compact('posts', 'user'));
        View::first(['welcome_override', 'welcome'], compact('posts', 'user'));

        // A mailable names its template through the Content it returns, or
        // through $this->view() in the older build() shape — see
        // app/Mail/OrderShipped.php and app/Mail/BlogPublished.php.

        // A data argument that writes no keys down still passes what its
        // type spells out: welcomeData() returns an array shape, so both
        // names reach welcome.blade.php and this call satisfies its
        // signature.  Hover $posts inside the template to see the type this
        // shape gives it.
        view('welcome', $this->welcomeData());

        // The factory converts an Arrayable before rendering, so what
        // WelcomeData::toArray() returns is what the template receives.
        view('welcome', new WelcomeData());

        // A declared variable this call leaves out → `missing_view_variable`,
        // and a key nothing in welcome.blade.php (or anything it @includes)
        // reads → `unused_view_variable`.  Both are intentional.
        view('welcome', ['user' => $user, 'pots' => $posts]);

        // View under a custom path from config/view.php (resources/theme/views).
        // theme/dashboard.blade.php declares no @var signature, so the
        // variables passed here are inferred inside the template from
        // this call site ($theme completes as string, $author as BlogAuthor).
        view('theme.dashboard', ['theme' => 'dark'])->with('author', new BlogAuthor());

        // Named Routes
        route('home');
        route('admin.users.index');

        // Route registered from a provider via Route::…->group(base_path(…)),
        // with the route file under app/Modules instead of the routes/ dir.
        route('reviews.update');

        // Route declared in routes/api/v1.php, which routes/web.php pulls in
        // by a path it keeps in a variable rather than by a closure.
        route('api.v1.users.index');

        // Routes a router macro registers.  routes/web.php only calls
        // Route::bakeryAuth(); the names come from the macro body in
        // RouteServiceProvider, including the ones a nested macro adds.
        route('login');
        route('password.update');

        // Route parameters — the keys of the second argument are the
        // {parameters} of the route's URI (here bakeries/{bakery}, whose
        // prefix comes from the enclosing Route::prefix('bakeries') group).
        route('bakeries.show', ['bakery' => 1]);
        route('bakeries.cancel', ['bakery' => 1]);

        // Route::resource() writes no URI of its own, so the parameters come
        // from the one Laravel derives from the resource name — each segment
        // of bakeries.ovens singularized into bakeries/{bakery}/ovens/{oven}.
        route('bakeries.ovens.show', ['bakery' => 1, 'oven' => 2]);

        // routes/web.php registers these from a nested foreach over a literal
        // array and names each one by interpolation.  The loop is unrolled
        // statically, so they resolve like any written-out name.
        route('campaigns.black-friday.landing');
        route('campaigns.valentines.gifts');

        // A route name is written at more than the route() helper: the
        // signed-URL builders, the redirect facades, and the helper chains
        // that reach the same objects all name one.
        URL::signedRoute('bakeries.show', ['bakery' => 1]);
        URL::temporarySignedRoute('bakeries.cancel', 60, ['bakery' => 1]);
        Redirect::route('home');
        Response::redirectToRoute('bakeries.index');
        redirect()->route('admin.users.index');
        response()->redirectToRoute('home');

        // The "is the current route named …?" checks take a list of
        // patterns, where * stands for any run of characters — so
        // "campaigns.*" names every route the campaign loop registers.
        Route::is('home', 'campaigns.*');
        Route::currentRouteNamed('bakeries.index');
        request()->routeIs('bakeries.*');

        // Translation Keys
        __('messages.welcome');
        trans('auth.failed');
        trans_choice('messages.notifications', 5);
        Lang::get('pagination.next');
        Lang::has('validation.required');
        Lang::hasForLocale('validation.required', 'en');

        // The framework declares string|array|null for all three helpers,
        // because a key may name a whole group and the keyless form hands
        // its own null back.  The key at the call site settles which of
        // those a call really returns — hover each one to see it.
        __('messages.welcome');     // string, a single line
        __('validation.between');   // array, the lines under it
        __();                       // null, no key to look up
    }

    /**
     * Render sites whose receiver only a *type* settles.
     *
     * Nothing about `$views->make(…)` or `$mail->view(…)` says it renders
     * a template — a `make()` is a `make()` — so what makes each of these a
     * call site is what the receiver turns out to be. They navigate,
     * complete, and are checked against the template's signature exactly
     * like the spellings that announce themselves.
     *
     * Try:
     *  1. Ctrl+Click 'welcome' below to open welcome.blade.php.
     *  2. Drop a key from the compact() and the call reports the variable
     *     the template declares but no longer receives.
     *  3. Ctrl+Click 'partials.post_row' and 'partials.no_posts' — a
     *     renderEach() names both templates it can render.
     */
    public function typedRenderSites(ViewFactory $views): void
    {
        $posts = BlogPost::where('published', true)->get();
        $user = BlogAuthor::first();
        $views->make('welcome', compact('posts', 'user'));

        // A mailable held in a local names its template the same way one
        // reached through $this does.
        $mail = new OrderShipped();
        $mail->view('emails.order_shipped', ['post' => $posts->first()]);

        // renderEach() is the PHP spelling of @each, down to the argument
        // order: the partial is rendered once per entry with the entry
        // under the name the third argument spells and $key beside it, and
        // the fourth template only when the collection is empty.
        $views->renderEach('partials.post_row', $posts, 'post', 'partials.no_posts');
    }

    /**
     * The data welcome.blade.php is rendered from, as one array shape.
     *
     * A call site that hands this straight to view() writes neither key
     * down, so the shape is where both names and both types come from.
     *
     * @return array{user: ?BlogAuthor, posts: PostCollection}
     */
    private function welcomeData(): array
    {
        return [
            'user' => BlogAuthor::first(),
            'posts' => BlogPost::where('published', true)->get(),
        ];
    }


    // ── Artisan Command Names & Signatures ─────────────────────────────────

    /**
     * Command names declared by a command class's `$signature` / `$name` /
     * `#[Signature]` / `#[AsCommand]` are recoverable statically, so
     * referencing them as a string completes, navigates, and validates.
     *
     * Try:
     *  1. Trigger completion inside `Artisan::call('|')` — offers `bakery:sync`
     *     and `reports:generate` from app/Console/Commands.
     *  2. Ctrl+Click "bakery:sync" to jump to SyncBakeryCommand.
     *  3. Hover "bakery:sync" to see its arguments and options.
     *  4. "does:not-exist" below is flagged as an unknown command.
     *  5. Trigger completion inside the parameter array of the last call —
     *     offers `bakery` and `--fresh` / `--since` from the target signature.
     *  6. Ctrl+Click "bakery:forecast" — a command registered by
     *     `withCommands()` from app/Actions, outside the conventional
     *     Console/Commands folder. "bakery:fc" is its `#[Aliases]` name and
     *     lands on the same class.
     */
    public function artisanCommands(): void
    {
        // Command-name string completion / navigation / hover.
        Artisan::call('bakery:sync');
        Artisan::queue('reports:generate');

        // Commands the framework itself ships count too, not just the
        // project's own.
        Artisan::call('queue:work');

        // Scheduled commands name the same declarations.
        Schedule::command('bakery:sync')->daily();

        // A `#[Signature]` attribute declares a command just like the
        // `$signature` property, and inline arguments after the name are
        // fine: only the leading token is the command name.
        Artisan::call('bakery:prune-stale --days=30');

        // A command `bootstrap/app.php` registers with `withCommands()` from
        // app/Actions: neither its class name nor its folder follows the
        // convention, and it still resolves.
        Artisan::call('bakery:forecast', ['region' => 'north']);

        // An alias declared by `#[Aliases]` names the same command.
        Artisan::call('bakery:fc', ['region' => 'north']);

        // Unknown command name → `invalid_laravel_command` diagnostic.
        Artisan::call('does:not-exist');

        // Second-argument array-key completion resolves against the target
        // command's parsed signature: arguments by name, options as `--name`.
        Artisan::call('bakery:sync', [
            'bakery' => 1,
            '--fresh' => true,
            '--since' => '2024-01-01',
        ]);
    }


    // ── Authorization Abilities & Policies ─────────────────────────────────

    /**
     * Authorization strings are recoverable statically: `Gate::define()`
     * registers an ability by name, and every public method of a model's
     * policy is an ability valid for that model.
     *
     * Try:
     *  1. Trigger completion inside `Gate::allows('|')` — offers
     *     `manage-bakery-network` plus every policy method.
     *  2. Ctrl+Click "manage-bakery-network" to jump to the `Gate::define()`
     *     call in DemoServiceProvider; Ctrl+Click "update" to jump to
     *     BakeryPolicy::update().
     *  3. Hover "manage-bakery-network" to see the closure it was defined
     *     with, and "update" to see the policy methods that implement it.
     *  4. Change `'publish'` below to `'update'` — the ability exists, but
     *     not for BlogPost, so it is flagged as such rather than as unknown.
     */
    public function authorizationAbilities(Review $review): void
    {
        $bakery = new Bakery();

        // Defined by Gate::define() in DemoServiceProvider, so valid for any
        // subject.
        Gate::allows('manage-bakery-network', 'north');
        Gate::authorize('manage-bakery-network', 'north');

        // BakeryPolicy is found by the naming convention
        // (App\Models\Bakery → App\Policies\BakeryPolicy).  `before()` is a
        // hook and `sameOwner()` is protected, so neither is offered.
        Gate::allows('update', $bakery);
        Gate::any(['update', 'delete'], $bakery);
        auth()->user()->can('delete', $bakery);
        auth()->user()->cannot('viewAny', Bakery::class);

        // BlogPost is bound to PublishingPolicy by Gate::policy().
        Gate::allows('publish', BlogPost::class);

        // Review points at its policy with #[UsePolicy].
        Gate::allows('moderate', $review);
    }


    // ── PHPDoc Virtual Member References & Rename ───────────────────────────
    // Try: right-click "displayName" or "bio" below and use
    //   • Find All References — includes the @property/@method declaration
    //   • Rename Symbol — renames in the docblock AND all usage sites

    public function phpdocVirtualMembers(): void
    {
        $author = new BlogAuthor();
        $author->displayName;           // @property-read on BlogAuthor
        $author->bio();                 // @method on BlogAuthor

        $found = BlogAuthor::where('active', true)->first();
        $found->displayName;
        $found->bio();
    }


    // ── Eloquent Relation & Column String Completion ────────────────────────
    // Trigger completion inside the string arguments below.

    public function eloquentStringCompletion(): void
    {
        // Relation string completion in with(), load(), has(), etc.
        BlogAuthor::with('');            // offers: posts, profile, …
        BlogPost::with('');              // offers: author, comments, …
        BlogAuthor::with('posts.');      // dot-notation: offers nested relations on BlogPost

        // Column name completion in where(), orderBy(), select(), etc.
        BlogAuthor::where('');           // offers: name, email, active, genre, …
        BlogPost::orderBy('');           // offers: title, published, author_id, …
        Bakery::select('');              // offers: flour, apricot, kitchen_id, …
    }


    // ── Eloquent Morph Aliases ──────────────────────────────────────────────

    public function morphAliases(): void
    {
        // `blog_post` and `bakery` are registered by DemoServiceProvider's
        // `Relation::morphMap()` call.  Hover shows the model each alias maps
        // to, go-to-definition offers the registration and the model, and
        // find-references links every usage of the alias.
        Review::whereHasMorph('reviewable', ['blog_post', 'bakery'])->get();

        // The same alias resolves in Relation::getMorphedModel(), where
        // completion also offers the registered aliases.
        Relation::getMorphedModel('blog_post');   // → App\Models\BlogPost
    }


    // ── Laravel Config (definition & references) ────────────────────────

    public function laravelConfig(): void
    {
        config('app.name');
        Config::get('database.default');
        Config::set('app.timezone', 'UTC');
    }


    // ── Config-backed Laravel resource names ───────────────────────────

    public function injectedNamedResources(
        #[InjectAuth(guard: 'admin')] mixed $guard,
        #[InjectAuthenticated(guard: 'admin')] mixed $user,
        #[InjectCache(store: 'memory')] mixed $cache,
        #[InjectLog(channel: 'daily')] mixed $logger,
        #[InjectStorage(disk: 'pantry')] mixed $disk,
        #[InjectDatabase(connection: 'mysql')] mixed $database,
    ): void
    {
        // Contextual-attribute arguments complete and navigate against the
        // same family-specific config entries as their facade counterparts.
    }

    public function namedLaravelResources(): void
    {
        // Hover identifies each resource family, Ctrl+Click opens its config
        // entry, and references include direct config() access to that entry.
        auth('admin');
        Auth::guard('admin');
        Cache::store('memory');
        Log::channel('daily');
        Log::stack(['daily', 'stderr']);
        Storage::disk('pantry');
        DB::connection('mysql');
        DB::connection('mysql::read');
        Queue::connection('redis');
        Mail::mailer('transactional');
        Broadcast::connection('internal');
        Route::middleware(['auth:admin']);
        config('cache.stores.memory');

        // Laravel supplies these null drivers at runtime even though no
        // matching child needs to exist in cache.php or queue.php.
        Cache::store('null');
        Queue::connection('null');
    }


    // ── Cache::remember() — closure return type binding ─────────────────

    public function cacheRemember(): void
    {
        // Cache::remember()'s TCacheValue is bound from the callback's
        // return type, even when the closure has no return annotation.
        $author = Cache::remember('author', 3600, fn () => BlogAuthor::firstOrFail());
        $author->name;                    // → BlogAuthor property (not mixed)

        $post = Cache::remember('post', 3600, function () {
            return new BlogPost();
        });
        $post->author;                    // → BlogPost relationship (block closure body)

        $forever = Cache::rememberForever('count', fn () => BlogAuthor::count());
        $forever + 1;                     // → int
    }


    // ── Auth user model from config/auth.php ────────────────────────────

    public function authUser(Request $request): void
    {
        // config/auth.php maps the default `web` guard's provider to
        // App\Models\Customer, so the authenticated user resolves to that
        // model.
        $request->user()->isPremium();    // → Customer method
        $request->user()->name;           // → Customer property

        // The no-argument entry points reach the same default guard model:
        // auth() returns the Factory contract (which forwards to the default
        // guard) and Auth::user() is declared as a facade @method tag.
        auth()->user()->isPremium();      // → Customer method
        Auth::user()->isPremium();        // → Customer method

        // Passing a guard name selects that guard's configured model.
        // The `admin` guard's provider maps to App\Models\Administrator,
        // so the user resolves to Administrator, not Customer.
        auth('admin')->user()->isSuperAdmin();          // → Administrator method
        Auth::guard('admin')->user()->isSuperAdmin();   // → Administrator method
        $request->user('admin')->isSuperAdmin();        // → Administrator method
    }


    // ── Bail-out helpers narrow the code that follows them ──────────────

    public function bailOutGuards(int $id): void
    {
        // `abort_if` only returns when its condition was false, so the null
        // is gone from the next line on: passing `$review` where a Review is
        // required is accepted here and reported without the guard above it.
        $review = Review::find($id);
        abort_if($review === null, 404);
        $this->repliesTo($review);        // → Review, no longer ?Review

        // `_unless` is the other polarity: it returns when the condition
        // held, so the check itself is what stands afterwards.
        $account = Auth::user();
        abort_unless($account instanceof Customer, 403);
        $account->isPremium();            // → Customer method

        // throw_if / throw_unless bail out the same way and prove the same
        // things, and every guard form an `if` understands works here too.
        $flour = Bakery::findOrFail($id)->flour;   // $fillable, no cast → mixed
        throw_unless(is_string($flour), \RuntimeException::class);
        strtoupper($flour);               // → string
    }

    // ── filled() and blank() rule out the empty values ──────────────────

    public function valueHelpers(?string $search): void
    {
        // `filled()` promises the value is neither null nor empty, so the
        // null is gone inside the branch and `strtoupper()` is happy with
        // it. `blank()` is the same promise read from the other side.
        if (filled($search)) {
            strtoupper($search);          // → string, no longer ?string
        }

        if (blank($search)) {
            return;
        }

        strtoupper($search);              // → string
    }


    // ── Request input keys from validation rules ────────────────────────

    public function requestInputKeys(StoreBakeryRequest $request): void
    {
        // StoreBakeryRequest::rules() names every input this request can
        // carry, so the string arguments below complete to those keys and
        // go-to-definition jumps to the rule that declares them.
        $request->input('name');          // → 'name' => 'required|string|max:255'
        $request->boolean('apricot');     // → 'apricot' => 'boolean'
        $request->has('dough_temp');      // → 'dough_temp' => 'nullable|numeric'
        $request->validated('name');      // → same keys, validated form
        $request['name'];                 // → array access completes too

        // A wildcard rule is only addressable through its root segment, so
        // `notes.*.body` offers `notes`; a plain dotted key is offered whole.
        $request->input('notes');         // → root of 'notes.*.body'
        $request->input('owner.email');   // → 'owner.email' => 'required|email'

        // safe() narrows the same rule set, whether it is chained straight
        // through or parked in a variable first.
        $request->safe()->only(['name', 'apricot']);

        $safe = $request->safe();
        $safe->except(['dough_temp']);    // → still StoreBakeryRequest's keys
    }


    // ── Request accessors answer for the call they were written as ──────
    // Each of these declares one type covering every way of calling it, so
    // the arguments at the call site are what say which way this is.

    public function requestAccessorTypes(StoreBakeryRequest $request): void
    {
        // No key at all is the whole bag; a key is the item in it.
        $request->query();                // → array<string, mixed>
        $request->header();               // → array<string, list<string|null>>

        // A header is a string, and the default is what a missing one
        // produces — so a string default leaves nothing else.
        $request->header('User-Agent');       // → string|null
        $request->header('User-Agent', '');   // → string

        // The rules say `photo` is one image and `gallery` a list of files,
        // so that is what each key hands back.
        $request->file('photo');          // → UploadedFile|null
        $request->file('gallery');        // → list<UploadedFile>|null
        $request->file();                 // → array<string, UploadedFile|list<UploadedFile>>

        $photo = $request->file('photo');
        if ($photo !== null) {
            $photo->getClientOriginalName();  // → string
        }
    }


    // ── Request input keys inherited through parent::rules() ────────────

    public function inheritedRequestInputKeys(UpdateBakeryRequest $request): void
    {
        // UpdateBakeryRequest::rules() is `array_merge(parent::rules(), […])`,
        // so its contract is both arrays.  The inherited keys complete here
        // too, and go-to-definition on one jumps to StoreBakeryRequest.
        $request->input('slug');          // → 'slug' => 'required|string'
        $request->input('name');          // → inherited from StoreBakeryRequest
        $request->boolean('apricot');     // → inherited from StoreBakeryRequest

        // An excluded field is validated and then dropped, so it is a key of
        // the request but not of the validated array.
        $data = $request->validated();

        $data['slug'];                    // → string
        $data['name'];                    // → string (inherited rule)
        $data['reason'];                  // → string, optional (exclude_if)
    }


    // ── Request input keys from an inline validate() call ───────────────

    public function inlineValidateKeys(Request $request): void
    {
        // A plain Request has no rules() to read, so the keys come from the
        // validate() call earlier in this same method.
        $request->validate([
            'headline' => 'required|string',
            'published_at' => 'nullable|date',
        ]);

        $request->input('headline');      // → from the validate() call above
        $request->filled('published_at'); // → from the validate() call above
    }


    public function branchedValidateKeys(Request $request): void
    {
        // Which arm ran is not knowable, so the keys of both describe the
        // request afterwards.
        if ($request->boolean('draft')) {
            $request->validate(['draft_note' => 'required|string']);
        } else {
            $request->validate(['published_at' => 'required|date']);
        }

        $request->input('draft_note');    // → from the if arm
        $request->input('published_at');  // → from the else arm
    }


    // ── Typed validated() array shapes from rules ───────────────────────

    public function validatedArrayShape(StoreBakeryRequest $request, string $field): void
    {
        // validated() is declared `array`, but the rules array says exactly
        // which keys it holds and what each one is, so it resolves to:
        //   array{
        //     name: string,
        //     apricot?: bool,
        //     dough_temp?: ?int|float,
        //     notes?: list<array{body: string}>,
        //     owner: array{email: string},
        //     flavor: string,
        //     batch_size: int,
        //   }
        $data = $request->validated();

        $data['name'];                    // → string
        $data['apricot'];                 // → bool ('apricot' is optional)
        $data['notes'];                   // → list<array{body: string}>
        $data['owner']['email'];          // → string

        // An enum rule types its field as the enum's backing type, because
        // the validated array holds the raw input rather than the case.
        $data['flavor'];                  // → string (JamFlavor: string)
        $data['batch_size'];              // → int (BatchSize: int)

        // A key argument returns that member's type rather than the array.
        $request->validated('name');      // → string

        // safe() narrows the same shape.
        $subset = $request->safe()->only(['name', 'apricot']);
        $subset['name'];                  // → string, and 'notes' is gone

        // A key the engine cannot read leaves the declared `array` in place,
        // rather than guessing at a narrower set than the call selects.
        $request->validated($field);      // → array (the declared type)
        $request->safe()->only([$field]); // → array (the declared type)
    }


    public function validatedShapeFromInlineRules(Request $request): void
    {
        // The rules passed to validate() type its return value directly, so
        // no FormRequest class is needed.
        $data = $request->validate([
            'headline' => 'required|string',
            'rank' => 'required|integer',
            'featured' => 'boolean',
        ]);

        $data['headline'];                // → string
        $data['rank'];                    // → int
        $data['featured'];                // → bool (optional key)
    }


    // ── @mixin of an Eloquent model exposes its virtual members ─────────

    public function mixinModel(): void
    {
        // BakeryProxy is a plain class with `@mixin Bakery`.  The model's
        // synthesized virtual members flow through the mixin, not just its
        // real declared members.
        $proxy = new BakeryProxy(new Bakery());

        $proxy->baguettes;                // relationship HasMany → Collection<Loaf>
        $proxy->headBaker;                // relationship HasOne  → Baker
        $proxy->apricot;                  // $casts 'boolean'     → bool
        $proxy->topping('choc');          // scope method         → Builder
    }


    // ── Storage::fake() resolves to the concrete adapter ────────────────

    public function storageFake(
        #[\Illuminate\Container\Attributes\Storage(disk: 'avatars')] Filesystem $avatars,
    ): void
    {
        // fake() declares the Filesystem contract but always builds a
        // FilesystemAdapter, so the adapter-only assertion helpers resolve.
        // Disk names complete from config/filesystems.php, hover with their
        // resource family, and navigate back to their declarations.
        Storage::fake(disk: 'avatars')->assertExists('me.png');
        Storage::persistentFake(disk: 'logs')->assertMissing('old.log');

        // forgetDisk() takes one name or a list of them, and tolerates a disk
        // that was never configured, so an unknown name here is not flagged.
        Storage::forgetDisk('avatars');
        Storage::forgetDisk(disk: ['avatars', 'logs']);
    }


    // ── Storage::disk() resolves to the concrete adapter ────────────────

    public function storageDisk(): void
    {
        // disk()/cloud() declare the Filesystem/Cloud contract, but every
        // disk config/filesystems.php configures ('local', 's3', ...) builds
        // a FilesystemAdapter, so adapter-only methods like download()
        // resolve on every configured disk, not just a faked one.
        Storage::disk(name: 's3')->download('report.pdf');
        Storage::disk(name: 'local')->exists('notes.txt');
        Storage::cloud()->assertExists('logo.png');

        // The 'pantry' disk uses a driver the framework does not ship.  Its
        // Storage::extend() closure in DemoServiceProvider builds a
        // FilesystemAdapter too, so a custom driver does not cost the rest of
        // the project its precise disk type.
        Storage::disk('pantry')->download('sourdough.pdf');

        // A disk configured at runtime is configured all the same: nothing in
        // config/filesystems.php declares 'ondemand' or 'scratch', and neither
        // read below is flagged because the write above it establishes the
        // disk.  Configuring one in a test's setUp() is the usual shape.
        Config::set('filesystems.disks.ondemand', ['driver' => 'local']);
        Storage::disk('ondemand')->exists('invoice.pdf');

        Storage::fake('scratch');
        Storage::disk('scratch')->exists('draft.txt');
    }


    // ── Container string aliases & global facades ───────────────────────

    public function containerAliases(): void
    {
        // Laravel binds these string keys to concrete classes in the
        // container.  PHPantom reads the framework's own alias table
        // (`Application::registerCoreContainerAliases()`), so the concrete
        // class resolves instead of `mixed`.
        resolve('blade.compiler')->compileString('<x-foo />');  // → BladeCompiler
        app('cache')->store();                                  // → CacheManager

        // A bare global facade alias resolves without an explicit import,
        // from the framework's own `Facade::defaultAliases()` table.
        \App::environment('production');                        // → App facade

        // A class-string handed to the container resolves to that class,
        // whether the result is stored first or chained inline.
        $controller = App::make(BakeryController::class);
        $controller->index();                                   // → View
        App::make(BakeryController::class)->index();            // → View
        App::makeWith(BakeryController::class, [])->index();    // → View

        // A key a project's own provider registers resolves the same way.
        // DemoServiceProvider writes neither key as a literal: both are built
        // from `static::$abstract`, declared on the base provider it extends.
        // That base provider binds `pastry.oven` to PlainOven, and
        // DemoServiceProvider replaces it, so the key resolves to the
        // replacement rather than the default it swapped out.
        app('pastry.oven')->bake('croissant');                  // → BakeryService
        app('pastry.oven.supplier')->supply(12);                // → CroissantSupplier

        // A provider may list its registrations in the `$bindings` /
        // `$singletons` arrays Laravel reads off it instead of writing them
        // out in register(), and a factory whose body builds something
        // PHPantom cannot follow still declares what it hands back.  All
        // three are in DemoServiceProvider.
        app('pastry.counter')->counted('croissant');            // → int
        app('pastry.plain-oven')->bake('rye');                  // → string
        app('pastry.tally')->tally()->counted('bun');           // → int

        // Hover any of these keys to see the class it resolves to and the
        // provider that registered it, and go-to-definition to jump to the
        // registration itself.
    }


    // ── App-defined facade without a generated docblock ────────────────

    public function appFacade(): void
    {
        // `App\Facades\Oven` declares no `@method static` tags, so its
        // members come from the class its `getFacadeAccessor()` names.
        // Trigger completion on `Oven::` and the BakeryService methods
        // are all there.
        Oven::bake('sourdough');                                // → string
        Oven::heatedTo('hot')->bake('rye');                     // → string

        // `App\Facades\PastryOven` names a container binding rather than a
        // class, the shape Laravel's own facades use.  The key is bound in
        // DemoServiceProvider, so the members come from what the container
        // resolves it to rather than from anything the facade writes.
        PastryOven::bake('brioche');                            // → string
        PastryOven::heatedTo('warm')->bake('baguette');         // → string
    }


    // ── Macro methods registered in a service provider ─────────────────

    public function collectionMacro(Collection $items): float
    {
        // `sumField` is registered via Collection::macro(...) in
        // DemoServiceProvider::boot(). PHPantom scans that registration, so
        // the macro autocompletes and resolves to the closure's return type.
        return $items->sumField('price');   // → float
    }

    public function collectionMixin(Collection $items): array
    {
        // `toAssoc` is registered via Collection::mixin(new CollectionMixin())
        // in DemoServiceProvider. A mixin registers one macro per public method,
        // each with the signature of the closure that method returns; PHPantom
        // recovers those so the macros resolve on Collection just like a
        // Collection::macro(...) registration.  `$this` inside the closures resolves via the mixin's
        // `@mixin Collection` tag.
        return $items->toAssoc('id', 'name');   // → array
    }


    // ── Carbon macro() and trait-based mixin() ───────────────────────

    public function carbonMacro(CarbonImmutable $date): string
    {
        // `diffFromYear` is registered via CarbonImmutable::macro(...)
        // in DemoServiceProvider::boot(). Carbon's macro() works the same
        // as Laravel's Macroable, so the closure's signature is recovered.
        return $date->diffFromYear(2020);   // → string
    }

    public function carbonTraitMixin(CarbonImmutable $date): \Carbon\CarbonInterface
    {
        // `toTz` and `toAppTz` come from CarbonImmutable::mixin(CarbonMixin::class)
        // in DemoServiceProvider. Carbon supports trait-based mixins: the trait's
        // own method signatures become the macro signatures directly, without
        // the closure-factory wrapper that class-based mixins require.
        $date->toAppTz();                   // → CarbonInterface
        return $date->toTz('UTC', true);    // → CarbonInterface
    }


    // ── Macros from a path-repository module ──────────────────────────

    public function moduleCollectionMacro(Collection $items): Collection
    {
        // `toUpper` is registered in Demo\Common\CommonServiceProvider (a
        // path-repository package under app-modules/common/).  PHPantom
        // discovers path-repo PSR-4 directories from installed.json,
        // so macros defined in local modules work the same as app/ macros.
        return $items->toUpper();   // → Collection
    }


    // ── Contract type-hints resolve through the concrete class ──────────

    public function viewContract(\Illuminate\Contracts\View\View $view): void
    {
        // The View contract declares only name()/with()/getData(), but the
        // object Laravel binds is the concrete Illuminate\View\View, which
        // uses the Macroable trait.  PHPantom binds the concrete to the
        // contract as a mixin, so concrete-only methods resolve and calls
        // dispatched through Macroable's __call no longer report as unknown.
        $view->getName();                 // concrete-only method → string
        $view->render();                  // concrete-only method → string
        $view->fragment('sidebar');       // concrete-only method
    }


    // ── Leading backslash bypasses a same-short-name import ────────────

    public function globalRedisOverImport(): void
    {
        // This file imports `Illuminate\Support\Facades\Redis`, but a
        // leading-backslash `\Redis` is an explicit global reference, so
        // it resolves to the PECL extension's global `\Redis` class rather
        // than the facade that shares the short name.
        /** @var \Redis $client */
        $client = new \Redis();
        $client->select(1);               // global \Redis method
        $client->connect('127.0.0.1');    // global \Redis method

        // The bare `Redis` short name still resolves to the imported facade.
        Redis::connection();              // Facades\Redis static method
    }

    public function dateHelpers(): \DateTime
    {
        // now()/today() are declared to return CarbonInterface, but they
        // instantiate the concrete Illuminate\Support\Carbon (a \DateTime),
        // so member access resolves and a chained fluent call stays concrete.
        now()->addHours(1);               // → Illuminate\Support\Carbon
        today()->startOfDay();            // → Illuminate\Support\Carbon

        // Returning the chain from a :DateTime method is not a mismatch,
        // because Illuminate\Support\Carbon extends \DateTime.
        return now()->addHours(1);
    }

    // ── URL helper conditional return ──────────────────────────────────────

    public function urlHelpers(string $path): string
    {
        // Without a path, url() returns the generator and its methods resolve.
        url()->to($path);                 // → UrlGenerator
        url(null)->to($path);             // → UrlGenerator

        // Supplying a path returns the generated URL string.
        return url($path);                // → string
    }

    // ── HTTP client sync/async template default ────────────────────────────

    public function httpClient(PendingRequest $pending, HttpFactory $factory): void
    {
        // PendingRequest is `@template TAsync of bool = false`, so a request
        // that never selects async mode carries the default and its methods
        // return the response itself rather than a promise.
        Http::get('https://example.com')->json();          // → Response
        Http::post('https://example.com')->status();       // → Response
        $pending->get('https://example.com')->body();      // → Response
        $factory->get('https://example.com')->ok();        // → Response

        // async() binds TAsync to true, so the same call returns a promise
        // whether it starts on the facade, the factory, or the request.
        Http::async()->get('https://example.com');         // → PromiseInterface
        $factory->async()->get('https://example.com');     // → PromiseInterface
        $pending->async()->get('https://example.com');     // → PromiseInterface
    }
}
