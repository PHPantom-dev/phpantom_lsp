<?php

// Expectations adapted from Larastan's custom-eloquent-builder.php and
// relationship-query-callbacks.php at c328727e6103c1147d1c64cc96b3aedfda26bc20.
// Framework declarations retain Laravel's callable signatures; none of the
// model-specific expectations are supplied by the stubs themselves.
// Known gaps retain the desired type with the corpus's SKIP marker. Arrow
// callbacks are covered in completion_laravel.rs because this runner replaces
// the whole assertion line, which would erase an enclosing arrow expression.

namespace Illuminate\Support\Traits {
    trait ForwardsCalls {
        protected function forwardDecoratedCallTo($object, $method, $parameters) {}
    }
}

namespace Illuminate\Database\Query {
    class Builder {
        /** @return $this */
        public function orderBy($column, $direction = 'asc') { return $this; }
        /** @return $this */
        public function whereIn($column, array $values) { return $this; }
        /** @return $this */
        public function lockForUpdate() { return $this; }
        public function count(): int { return 0; }
    }
}

namespace Illuminate\Database\Eloquent {
    class Model {
        /** @return Builder<static> */
        public static function query() {}
        /** @return Builder<static> */
        public function newQuery() {}
        /** @return Builder<static> */
        public function newQueryWithoutScopes() {}
        /** @return Builder<static> */
        public function newModelQuery() {}
    }

    /**
     * @template TModel of Model
     * @mixin \Illuminate\Database\Query\Builder
     */
    class Builder {
        use \Illuminate\Support\Traits\ForwardsCalls;

        /** @return $this */
        public function where($column, $operator = null, $value = null) { return $this; }
        /** @return TModel */
        public function firstOrFail() {}
        /** @return TModel|null */
        public function first() {}
        /** @return TModel */
        public function getModel() {}
        public function exists(): bool { return false; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\Relation<TRelatedModel, *, *>|string $relation
         * @param (\Closure(Builder<TRelatedModel>): mixed)|null $callback
         * @return $this
         */
        public function has($relation, $operator = '>=', $count = 1, $boolean = 'and', ?\Closure $callback = null) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\Relation<TRelatedModel, *, *>|string $relation
         * @param (\Closure(Builder<TRelatedModel>): mixed)|null $callback
         * @return $this
         */
        public function doesntHave($relation, $boolean = 'and', ?\Closure $callback = null) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\Relation<TRelatedModel, *, *>|string $relation
         * @param (\Closure(Builder<TRelatedModel>): mixed)|null $callback
         * @return $this
         */
        public function whereHas($relation, ?\Closure $callback = null, $operator = '>=', $count = 1) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\Relation<TRelatedModel, *, *>|string $relation
         * @param (\Closure(Builder<TRelatedModel>): mixed)|null $callback
         * @return $this
         */
        public function orWhereHas($relation, ?\Closure $callback = null, $operator = '>=', $count = 1) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\Relation<TRelatedModel, *, *>|string $relation
         * @param (\Closure(Builder<TRelatedModel>): mixed)|null $callback
         * @return $this
         */
        public function whereDoesntHave($relation, ?\Closure $callback = null) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\Relation<TRelatedModel, *, *>|string $relation
         * @param (\Closure(Builder<TRelatedModel>): mixed)|null $callback
         * @return $this
         */
        public function orWhereDoesntHave($relation, ?\Closure $callback = null) { return $this; }

        /**
         * @param string $relation
         * @param (\Closure(Builder<*>|Relations\Relation<*, *, *>): mixed)|null $callback
         * @return $this
         */
        public function withWhereHas($relation, ?\Closure $callback = null, $operator = '>=', $count = 1) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\MorphTo<TRelatedModel, *>|string $relation
         * @param string|array<int, string> $types
         * @param (\Closure(Builder<TRelatedModel>, string): mixed)|null $callback
         * @return $this
         */
        public function whereHasMorph($relation, $types, ?\Closure $callback = null, $operator = '>=', $count = 1) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\MorphTo<TRelatedModel, *>|string $relation
         * @param string|array<int, string> $types
         * @param (\Closure(Builder<TRelatedModel>, string): mixed)|null $callback
         * @return $this
         */
        public function orWhereHasMorph($relation, $types, ?\Closure $callback = null, $operator = '>=', $count = 1) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\MorphTo<TRelatedModel, *>|string $relation
         * @param string|array<int, string> $types
         * @param (\Closure(Builder<TRelatedModel>, string): mixed)|null $callback
         * @return $this
         */
        public function hasMorph($relation, $types, $operator = '>=', $count = 1, $boolean = 'and', ?\Closure $callback = null) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\MorphTo<TRelatedModel, *>|string $relation
         * @param string|array<int, string> $types
         * @param (\Closure(Builder<TRelatedModel>, string): mixed)|null $callback
         * @return $this
         */
        public function doesntHaveMorph($relation, $types, $boolean = 'and', ?\Closure $callback = null) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\MorphTo<TRelatedModel, *>|string $relation
         * @param string|array<int, string> $types
         * @param (\Closure(Builder<TRelatedModel>, string): mixed)|null $callback
         * @return $this
         */
        public function whereDoesntHaveMorph($relation, $types, ?\Closure $callback = null) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\MorphTo<TRelatedModel, *>|string $relation
         * @param string|array<int, string> $types
         * @param (\Closure(Builder<TRelatedModel>, string): mixed)|null $callback
         * @return $this
         */
        public function orWhereDoesntHaveMorph($relation, $types, ?\Closure $callback = null) { return $this; }
    }
}

namespace Illuminate\Database\Eloquent\Relations {
    /**
     * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
     * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
     * @template TResult
     * @mixin \Illuminate\Database\Eloquent\Builder<TRelatedModel>
     */
    class Relation {
        use \Illuminate\Support\Traits\ForwardsCalls;
    }
    /**
     * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
     * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
     * @extends Relation<TRelatedModel, TDeclaringModel, mixed>
     */
    class HasMany extends Relation {}
    /**
     * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
     * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
     * @extends Relation<TRelatedModel, TDeclaringModel, mixed>
     */
    class BelongsTo extends Relation {}
    /**
     * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
     * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
     * @extends Relation<TRelatedModel, TDeclaringModel, mixed>
     */
    class MorphTo extends Relation {}
}

namespace BuilderRelationAudit {
    use Illuminate\Database\Eloquent\Builder;
    use Illuminate\Database\Eloquent\Builder as Query;
    use Illuminate\Database\Eloquent\Model;
    use Illuminate\Database\Eloquent\Relations\BelongsTo;
    use Illuminate\Database\Eloquent\Relations\HasMany;
    use Illuminate\Database\Eloquent\Relations\MorphTo;
    use function PHPStan\Testing\assertType;

    /**
     * @template TModel of Model
     * @extends Builder<TModel>
     */
    class TeamBuilder extends Builder {
        /** @return $this */
        public function active() { return $this; }
    }
    class Team extends Model {
        /** @return TeamBuilder<static> */
        public function newEloquentBuilder($query): TeamBuilder { return new TeamBuilder(); }
        /** @return HasMany<Stock, $this> */
        public function stocks(): HasMany {}
        public function teamName(): string { return ''; }
    }
    class Stock extends Model {
        /** @return BelongsTo<Warehouse, $this> */
        public function warehouse(): BelongsTo {}
        /** @return BelongsTo<Team, $this> */
        public function team(): BelongsTo {}
        /** @return BelongsTo<PlainTeam, $this> */
        public function plainTeam(): BelongsTo {}
    }
    class Warehouse extends Model {
        public function warehouseName(): string { return ''; }
    }
    class PlainTeamBuilder extends Builder {
        /** @return $this */
        public function active() { return $this; }
    }
    class PlainTeam extends Model {
        public function newEloquentBuilder($query): PlainTeamBuilder { return new PlainTeamBuilder(); }
        public function teamName(): string { return ''; }
    }
    class ChildTeam extends Team {}
    class OverrideTeam extends Team {
        public function newQuery(): PlainTeamBuilder { return new PlainTeamBuilder(); }
    }
    class OtherModelQuery extends Team {
        /** @return Builder<Warehouse> */
        public function newQuery(): Builder {}
    }
    class Comment extends Model {
        /** @return MorphTo<Model, $this> */
        public function commentable(): MorphTo {}
    }
    class TeamComment extends Model {
        /** @return MorphTo<Team, $this> */
        public function commentable(): MorphTo {}
    }

    function builders(Team $team, PlainTeam $plain): void {
        $builder = Team::query();
        assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $builder->where('active', true)->orderBy('id'));
        assertType('BuilderRelationAudit\Team', $builder->where('active', true)->getModel());
        assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', Team::query());
        assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', Team::where('active', true)->orderBy('id'));
        assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', Team::query()->where('active', true)->orderBy('id')->active());
        assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', Team::active()->whereIn('id', [1])->lockForUpdate());
        assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $team->newQuery()->orderBy('id'));
        assertType('BuilderRelationAudit\Team', Team::where('id', 1)->firstOrFail());
        assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $team->newQueryWithoutScopes()->active());
        assertType('BuilderRelationAudit\Team', $team->newModelQuery()->firstOrFail());
        assertType('BuilderRelationAudit\PlainTeam', $plain->newQueryWithoutScopes()->firstOrFail());
        assertType('BuilderRelationAudit\PlainTeamBuilder<BuilderRelationAudit\PlainTeam>', $plain->newModelQuery()->active());
        assertType('BuilderRelationAudit\Team|null', Team::query()->active()->first());
        assertType('bool', Team::query()->where('id', 1)->exists());
        assertType('int', Team::query()->where('id', 1)->count());
        // PHPantom retains the model argument even on a non-generic custom builder.
        assertType('BuilderRelationAudit\PlainTeamBuilder<BuilderRelationAudit\PlainTeam>', PlainTeam::query()->where('active', true)->orderBy('id')->active());
        assertType('BuilderRelationAudit\PlainTeamBuilder<BuilderRelationAudit\PlainTeam>', PlainTeam::where('active', true)->orderBy('id'));
        assertType('BuilderRelationAudit\PlainTeamBuilder<BuilderRelationAudit\PlainTeam>', $plain->newQuery()->orderBy('id'));
        assertType('BuilderRelationAudit\PlainTeam', PlainTeam::query()->active()->firstOrFail());
    }

    function inheritedFactories(ChildTeam $child, OverrideTeam $override, Warehouse $ordinary, OtherModelQuery $other): void {
        assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\ChildTeam>', $child->newQuery()->active());
        assertType('BuilderRelationAudit\PlainTeamBuilder', $override->newQuery());
        assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $ordinary->newQuery());
        assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $other->newQuery());
    }

    class UnrelatedQuery {
        /** @param \Closure(Warehouse): mixed $callback */
        public function whereHas(string $relation, \Closure $callback): void {}
    }

    function unrelated(UnrelatedQuery $query): void {
        $query->whereHas('stocks.warehouse', function ($model) {
            assertType('BuilderRelationAudit\Warehouse', $model);
        });
    }

    function relations(): void {
        Team::whereHas('stocks', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Stock>', $query);
        });
        Team::whereHas('stocks.warehouse', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query);
            assertType('BuilderRelationAudit\Warehouse', $query->firstOrFail());
        });
        Stock::query()->whereHas('warehouse', function (Builder $query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query);
        });
        Team::query()->where('active', true)->whereHas('stocks.warehouse', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query);
        });
        Team::has('stocks.warehouse', '>=', 1, 'and', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query);
        });
        Team::doesntHave('stocks', 'and', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Stock>', $query);
        });
        Team::orWhereHas('stocks.warehouse', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query);
        });
        Team::whereDoesntHave('stocks', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Stock>', $query);
        });
        Team::orWhereDoesntHave('stocks', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Stock>', $query);
        });
        Team::has(relation: 'stocks.warehouse', callback: function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query);
        });
        Team::has(callback: function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query); // SKIP: relation argument must currently come first
        }, relation: 'stocks.warehouse');
        Stock::whereHas('team', function ($query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
        });
        Stock::whereHas('team', function (Builder $query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
        });
        Stock::query()->whereHas('team', function (Query $query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query->active());
        });
        Stock::whereHas('plainTeam', function (Builder $query) {
            assertType('BuilderRelationAudit\PlainTeamBuilder<BuilderRelationAudit\PlainTeam>', $query);
        });
        Stock::whereHas('team', function (Warehouse $query) {
            assertType('BuilderRelationAudit\Warehouse', $query);
        });
    }

    function eagerRelations(): void {
        Team::withWhereHas('stocks', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Stock>|Illuminate\Database\Eloquent\Relations\HasMany<BuilderRelationAudit\Stock, BuilderRelationAudit\Team>', $query); // SKIP: eager callback needs the builder/relation union
        });
        Team::withWhereHas('stocks.warehouse', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>|Illuminate\Database\Eloquent\Relations\BelongsTo<BuilderRelationAudit\Warehouse, BuilderRelationAudit\Stock>', $query); // SKIP: eager callback needs the builder/relation union
        });
        Team::withWhereHas('stocks:id', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Stock>|Illuminate\Database\Eloquent\Relations\HasMany<BuilderRelationAudit\Stock, BuilderRelationAudit\Team>', $query); // SKIP: eager callback needs the builder/relation union
        });
        assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', Team::withWhereHas('stocks', null));
    }

    function morphs(): void {
        Comment::whereHasMorph('commentable', Warehouse::class, function ($query, $type) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query); // SKIP: morph candidate builders are not inferred
            assertType('string', $type);
        });
        Comment::whereHasMorph('commentable', [Team::class, Warehouse::class], function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>|Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query); // SKIP: morph candidate builders are not inferred
            assertType('string', $type);
        });
        Comment::query()->orWhereHasMorph('commentable', Team::class, function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query); // SKIP: morph candidate builders are not inferred
            assertType('string', $type);
        });
        Comment::hasMorph('commentable', Team::class, '>=', 1, 'and', function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query); // SKIP: morph candidate builders are not inferred
            assertType('string', $type);
        });
        Comment::doesntHaveMorph('commentable', Team::class, 'and', function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query); // SKIP: morph candidate builders are not inferred
            assertType('string', $type);
        });
        Comment::whereDoesntHaveMorph('commentable', Team::class, function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query); // SKIP: morph candidate builders are not inferred
            assertType('string', $type);
        });
        Comment::orWhereDoesntHaveMorph('commentable', Team::class, function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query); // SKIP: morph candidate builders are not inferred
            assertType('string', $type);
        });
        Comment::whereHasMorph(callback: function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query); // SKIP: morph candidate builders are not inferred
            assertType('string', $type);
        }, relation: 'commentable', types: Team::class);
        Comment::whereHasMorph('commentable', '*', function ($query, $type) {
            assertType('Illuminate\Database\Eloquent\Builder<Illuminate\Database\Eloquent\Model>', $query); // SKIP: morph candidate builders are not inferred
            assertType('string', $type);
        });
        Comment::whereHasMorph('commentable', [], function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<Illuminate\Database\Eloquent\Model>', $query); // SKIP: morph candidate builders are not inferred
        });
        TeamComment::whereHasMorph('commentable', '*', function ($query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query); // SKIP: morph candidate builders are not inferred
        });
        assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Comment>', Comment::whereHasMorph('commentable', [Team::class], null));
    }
}
