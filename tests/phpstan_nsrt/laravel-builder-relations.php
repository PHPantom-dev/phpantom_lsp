<?php

// Expectations adapted from Larastan's custom-eloquent-builder.php and
// relationship-query-callbacks.php at c328727e6103c1147d1c64cc96b3aedfda26bc20.
// Framework declarations retain Laravel's callable signatures; none of the
// model-specific expectations are supplied by the stubs themselves.
// Arrow callbacks are covered in completion_laravel.rs because this runner replaces
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

namespace Illuminate\Contracts\Database\Eloquent {
    interface Builder {}
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
    class Builder implements \Illuminate\Contracts\Database\Eloquent\Builder {
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

        /** @return $this */
        public function withWhereRelation($relation, $column, $operator = null, $value = null) { return $this; }


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
        /**
         * @template TRelatedModel of Model
         * @param Relations\MorphTo<TRelatedModel, *>|string $relation
         * @param (\Closure(Builder<TRelatedModel>): mixed)|string $column
         * @return $this
         */
        public function whereMorphRelation($relation, $types, $column, $operator = null, $value = null) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\MorphTo<TRelatedModel, *>|string $relation
         * @param (\Closure(Builder<TRelatedModel>): mixed)|string $column
         * @return $this
         */
        public function orWhereMorphRelation($relation, $types, $column, $operator = null, $value = null) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\MorphTo<TRelatedModel, *>|string $relation
         * @param (\Closure(Builder<TRelatedModel>): mixed)|string $column
         * @return $this
         */
        public function whereMorphDoesntHaveRelation($relation, $types, $column, $operator = null, $value = null) { return $this; }

        /**
         * @template TRelatedModel of Model
         * @param Relations\MorphTo<TRelatedModel, *>|string $relation
         * @param (\Closure(Builder<TRelatedModel>): mixed)|string $column
         * @return $this
         */
        public function orWhereMorphDoesntHaveRelation($relation, $types, $column, $operator = null, $value = null) { return $this; }

    }
}

namespace Illuminate\Database\Eloquent\Relations {
    /**
     * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
     * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
     * @template TResult
     * @mixin \Illuminate\Database\Eloquent\Builder<TRelatedModel>
     */
    class Relation implements \Illuminate\Contracts\Database\Eloquent\Builder {
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
    use Illuminate\Database\Eloquent\Relations\Relation;
    use Illuminate\Contracts\Database\Eloquent\Builder as BuilderContract;
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


    class CustomArguments extends Team {
        /**
         * @param \Closure(Warehouse): void $operator
         * @param \Closure(Builder<Stock>): void $callback
         */
        public static function has($relation, $operator = null, $count = 1, $boolean = 'and', ?\Closure $callback = null) {}
    }

    function unrelatedArgument(): void {
        CustomArguments::has('stocks', operator: function ($value) {
            assertType('BuilderRelationAudit\Warehouse', $value);
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
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query);
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
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Stock>|Illuminate\Database\Eloquent\Relations\HasMany<BuilderRelationAudit\Stock, BuilderRelationAudit\Team>', $query);
        });
        Team::withWhereHas('stocks.warehouse', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>|Illuminate\Database\Eloquent\Relations\BelongsTo<BuilderRelationAudit\Warehouse, BuilderRelationAudit\Stock>', $query);
        });
        Team::withWhereHas('stocks:id', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Stock>|Illuminate\Database\Eloquent\Relations\HasMany<BuilderRelationAudit\Stock, BuilderRelationAudit\Team>', $query);
        });
        assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', Team::withWhereHas('stocks', null));
        Stock::withWhereHas(callback: function (Builder|Relation $query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>|Illuminate\Database\Eloquent\Relations\BelongsTo<BuilderRelationAudit\Team, BuilderRelationAudit\Stock>', $query);
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>|Illuminate\Database\Eloquent\Relations\BelongsTo<BuilderRelationAudit\Team, BuilderRelationAudit\Stock>', $query->where('active', true));
            assertType('BuilderRelationAudit\Team', $query->where('active', true)->firstOrFail());
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>|Illuminate\Database\Eloquent\Relations\BelongsTo<BuilderRelationAudit\Team, BuilderRelationAudit\Stock>', $query->orderBy('id'));
            assertType('int', $query->count());
        }, relation: 'team');
        Stock::withWhereHas('plainTeam', function (BuilderContract $query) {
            assertType('BuilderRelationAudit\PlainTeamBuilder<BuilderRelationAudit\PlainTeam>|Illuminate\Database\Eloquent\Relations\BelongsTo<BuilderRelationAudit\PlainTeam, BuilderRelationAudit\Stock>', $query);
        });
        Team::withWhereHas('stocks.team:id', function ($query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>|Illuminate\Database\Eloquent\Relations\BelongsTo<BuilderRelationAudit\Team, BuilderRelationAudit\Stock>', $query);
        });
        ChildTeam::withWhereHas('stocks', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Stock>|Illuminate\Database\Eloquent\Relations\HasMany<BuilderRelationAudit\Stock, BuilderRelationAudit\ChildTeam>', $query);
        });
        Stock::query()->withWhereRelation(column: function (Builder|Relation $query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>|Illuminate\Database\Eloquent\Relations\BelongsTo<BuilderRelationAudit\Team, BuilderRelationAudit\Stock>', $query);
        }, relation: 'team');
        Team::withWhereHas('stocks', function (Warehouse $query) {
            assertType('BuilderRelationAudit\Warehouse', $query);
        });
        Team::whereHas('stocks', function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Stock>', $query->where('id', 1));
        });
        assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', Team::withWhereRelation('stocks', 'id', '>', 0));

    }

    /**
     * @param class-string<Team> $teamClass
     * @param list<class-string<Team|Warehouse>> $classes
     * @param list<string> $unknownClasses
     */
    function morphCandidates(string $teamClass, array $classes, array $unknownClasses, string $unknown): void {
        Comment::whereHasMorph('commentable', $teamClass, function (Builder $query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
        });
        Comment::whereHasMorph('commentable', $classes, function (Builder $query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>|Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query);
        });
        Comment::whereHasMorph('commentable', [Team::class, $unknown], function ($query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>|Illuminate\Database\Eloquent\Builder<Illuminate\Database\Eloquent\Model>', $query);
        });
        TeamComment::whereHasMorph('commentable', $unknownClasses, function (Builder $query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
        });
        Comment::whereHasMorph('commentable', [Warehouse::class, Warehouse::class], function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query);
        });
        Comment::whereHasMorph('commentable', UnrelatedQuery::class, function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<Illuminate\Database\Eloquent\Model>', $query);
        });
        Comment::whereHasMorph('commentable', PlainTeam::class, function (Builder $query) {
            assertType('BuilderRelationAudit\PlainTeamBuilder<BuilderRelationAudit\PlainTeam>', $query);
        });
        Comment::whereHasMorph('commentable', 'BuilderRelationAudit\\Team', function ($query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
        });
        Comment::whereHasMorph('commentable', MissingModel::class, function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<Illuminate\Database\Eloquent\Model>', $query);
        });
        $assigned = Team::class;
        Comment::whereHasMorph('commentable', $assigned, function ($query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
        });
        Comment::whereHasMorph('commentable', Team::class, function (Warehouse $query) {
            assertType('BuilderRelationAudit\Warehouse', $query);
        });
        Comment::whereMorphRelation(column: function (Builder $query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
        }, types: Team::class, relation: 'commentable');
        Comment::orWhereMorphRelation(column: function (Builder $query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
        }, types: Team::class, relation: 'commentable');
        Comment::whereMorphDoesntHaveRelation(column: function (Builder $query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
        }, types: Team::class, relation: 'commentable');
        Comment::orWhereMorphDoesntHaveRelation(column: function (Builder $query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
        }, types: Team::class, relation: 'commentable');
    }

    function morphs(): void {
        Comment::whereHasMorph('commentable', Warehouse::class, function ($query, $type) {
            assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query);
            assertType('string', $type);
        });
        Comment::whereHasMorph('commentable', [Team::class, Warehouse::class], function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>|Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Warehouse>', $query);
            assertType('string', $type);
        });
        Comment::query()->orWhereHasMorph('commentable', Team::class, function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
            assertType('string', $type);
        });
        Comment::hasMorph('commentable', Team::class, '>=', 1, 'and', function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
            assertType('string', $type);
        });
        Comment::doesntHaveMorph('commentable', Team::class, 'and', function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
            assertType('string', $type);
        });
        Comment::whereDoesntHaveMorph('commentable', Team::class, function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
            assertType('string', $type);
        });
        Comment::orWhereDoesntHaveMorph('commentable', Team::class, function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
            assertType('string', $type);
        });
        Comment::whereHasMorph(callback: function ($query, $type) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
            assertType('string', $type);
        }, relation: 'commentable', types: Team::class);
        Comment::whereHasMorph('commentable', '*', function ($query, $type) {
            assertType('Illuminate\Database\Eloquent\Builder<Illuminate\Database\Eloquent\Model>', $query);
            assertType('string', $type);
        });
        Comment::whereHasMorph('commentable', [], function ($query) {
            assertType('Illuminate\Database\Eloquent\Builder<Illuminate\Database\Eloquent\Model>', $query);
        });
        TeamComment::whereHasMorph('commentable', '*', function ($query) {
            assertType('BuilderRelationAudit\TeamBuilder<BuilderRelationAudit\Team>', $query);
        });
        assertType('Illuminate\Database\Eloquent\Builder<BuilderRelationAudit\Comment>', Comment::whereHasMorph('commentable', [Team::class], null));
    }
}
