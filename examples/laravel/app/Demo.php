<?php
/**
 * Laravel Demo Classes for PHPantom LSP
 *
 * Open any method and trigger completion inside it.
 * Requires a real Laravel installation via `composer install`.
 */

namespace App;

use App\Models\Bakery;
use App\Models\BlogAuthor;
use App\Models\BlogPost;
use App\Models\Review;
use Illuminate\Support\Collection;
use Illuminate\Support\Facades\Config;
use Illuminate\Support\Facades\Lang;
use Illuminate\Support\Facades\View;

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
     */
    public function laravelConfigEnv(): void
    {
        // Global helper
        config('app.name');

        // Facade methods
        Config::get('app.name');
        Config::set('app.env', 'production');

        // Config keys that use env() — Ctrl+Click jumps to the config file,
        // then Ctrl+Click the env() call there to jump to .env
        config('app.key');                // uses env('APP_KEY')
        config('database.connections.mysql.password'); // uses env('DB_PASSWORD')
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
     */
    public function laravelNavigation(): void
    {
        // Blade Views — passing typed data for in-template completion
        $posts = BlogPost::where('published', true)->get();
        view('welcome', compact('posts'));
        View::make('admin.users.index', ['users' => BlogAuthor::all()]);
        View::exists('emails.blog_published');

        // Named Routes
        route('home');
        route('admin.users.index');

        // Translation Keys
        __('messages.welcome');
        trans('auth.failed');
        trans_choice('messages.notifications', 5);
        Lang::get('pagination.next');
        Lang::has('validation.required');
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


    // ── Laravel Config (definition & references) ────────────────────────

    public function laravelConfig(): void
    {
        config('app.name');
        Config::get('database.default');
        Config::set('app.timezone', 'UTC');
    }
}
