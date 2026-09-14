use std::collections::HashMap;

use crate::Backend;
use crate::atom::atom;

use super::*;

fn classes(source: &str) -> HashMap<Atom, Arc<ClassInfo>> {
    Backend::parse_php_versioned_with_namespaces(source, None)
        .into_iter()
        .map(|(class, _)| (class.name, Arc::new(class)))
        .collect()
}

#[test]
fn morph_type_columns_follow_laravel_names_and_explicit_arguments() {
    let classes = classes(
        r#"<?php
class Comment {
    public $notARelation;
    public function commentable() { return $this->morphTo(); }
    public function relatedItem() { return $this->morphTo()->withDefault(); }
    public function URLTarget() { return $this->morphTo(); }
    public function explicitName() { return $this->morphTo('imageOwner'); }
    public function explicitType() { return $this->morphTo(null, 'target_kind'); }
    public function namedType() { return $this->morphTo(type: 'named_kind'); }
    public function namedBoth() { return $this->morphTo(type: null, name: 'contentOwner'); }
    public function nulls() { return $this->morphTo(null, null); }
    public function falsey() { return $this->morphTo('', '0'); }
    public function caseInsensitive() { return $this->MORPHTO(); }
    public function parentheses() { return ($this->morphTo(name: ('linkedItem'), type: (null))); }
    public function ignoredArguments() { return $this->morphTo('link', null, 'link_id', 'key'); }
    public function namedOwnerKey() { return $this->morphTo(ownerKey: 'id'); }
    public function spacedName() { return $this->morphTo('Related Item'); }
    public function nestedBlocks() {
        if ($this->active) { return $this->morphTo('subject'); }
        return $this->morphTo('subject')->withDefault();
    }
}
"#,
    );
    let model = &classes[&atom("Comment")];
    let metadata = model.laravel().unwrap();
    let expected = [
        ("commentable", "commentable_type"),
        ("relatedItem", "related_item_type"),
        ("URLTarget", "u_r_l_target_type"),
        ("explicitName", "image_owner_type"),
        ("explicitType", "target_kind"),
        ("namedType", "named_kind"),
        ("namedBoth", "content_owner_type"),
        ("nulls", "nulls_type"),
        ("falsey", "falsey_type"),
        ("caseInsensitive", "case_insensitive_type"),
        ("parentheses", "linked_item_type"),
        ("ignoredArguments", "link_type"),
        ("namedOwnerKey", "named_owner_key_type"),
        ("spacedName", "related_item_type"),
        ("nestedBlocks", "subject_type"),
    ];
    assert_eq!(metadata.morph_type_columns.len(), expected.len());
    for (method, column) in expected {
        let explicit = matches!(
            method,
            "explicitName"
                | "explicitType"
                | "namedType"
                | "namedBoth"
                | "nestedBlocks"
                | "parentheses"
                | "ignoredArguments"
                | "spacedName"
        );
        assert!(
            metadata
                .morph_type_columns
                .contains(&(atom(method), explicit.then(|| atom(column))))
        );
        assert!(is_morph_type_column(model, column, &|name| {
            classes.get(&atom(name)).cloned()
        }));
    }
}

#[test]
fn morph_type_columns_reject_dynamic_and_unrelated_calls() {
    let classes = classes(
        r#"<?php
abstract class Comment {
    abstract public function abstractRelation();
    public function dynamicName() { return $this->morphTo($this->name); }
    public function dynamicType() { return $this->morphTo(null, $this->column); }
    public function interpolated() { return $this->morphTo(type: "{$this->column}"); }
    public function spread() { return $this->morphTo(...$this->arguments); }
    public function otherReceiver() { return $this->other->morphTo(); }
    public function argumentCall() { return wrap($this->morphTo()); }
    public function ignoredCall() { $this->morphTo(); return 1; }
    public function closureOnly() { return fn () => $this->morphTo(); }
    public function nestedFunction() {
        function nested() { return $this->morphTo(); }
        return 1;
    }
    public function conflictingReturns() {
        if ($this->active) { return $this->morphTo('subject'); }
        return $this->morphTo('owner');
    }
    public function dynamicReturn() {
        if ($this->active) { return $this->morphTo(); }
        return $this->other();
    }
    public function bareReturn() {
        if ($this->active) { return $this->morphTo(); }
        return;
    }
    public function invalidUtf8() { return $this->morphTo(type: "\x8b"); }
    public function noCalls() { return 1; }
    public function onlyIgnoredCall() { $this->morphTo(); }
    public function dynamicMethod() { return $this->{'morphTo'}(); }
}
"#,
    );
    assert!(
        classes[&atom("Comment")]
            .laravel()
            .unwrap()
            .morph_type_columns
            .is_empty()
    );
}

#[test]
fn morph_type_columns_respect_class_trait_and_parent_overrides() {
    let classes = classes(
        r#"<?php
trait ParentRelation {
    public function owner() { return $this->morphTo(null, 'parent_trait_type'); }
}
class ParentModel {
    use ParentRelation;
    public function subject() { return $this->morphTo(null, 'parent_type'); }
    public function inherited() { return $this->morphTo(); }
}
trait Relation {
    public function subject() { return $this->morphTo(null, 'trait_type'); }
}
trait NestedRelation { use Relation; }
class TraitModel extends ParentModel { use NestedRelation; }
class OwnModel extends ParentModel {
    use NestedRelation;
    public function subject() { return $this->morphTo(null, 'own_type'); }
}
class NoRelation extends ParentModel {
    use NestedRelation;
    public function SUBJECT() { return 1; }
    public function owner() { return 1; }
}
trait OtherRelation {
    public function subject() { return $this->morphTo(null, 'other_type'); }
}
class AdaptedModel {
    use Relation, OtherRelation {
        OtherRelation::subject insteadof Relation;
    }
}
class AliasedModel {
    use Relation, OtherRelation {
        OtherRelation::subject insteadof Relation;
        Relation::subject as originalSubject;
    }
}
trait DefaultRelation {
    public function subject() { return $this->morphTo(); }
}
trait ExplicitNameRelation {
    public function subject() { return $this->morphTo('subject'); }
}
class DefaultAliasedModel {
    use DefaultRelation { subject as alternateSubject; }
}
class DefaultOverriddenModel {
    use DefaultRelation { subject as alternateSubject; }
    public function subject() { return 1; }
}
class ExplicitNameAliasedModel {
    use ExplicitNameRelation { subject as alternateSubject; }
}
trait NestedAlias {
    use DefaultRelation { subject as alternateSubject; }
}
class NestedAliasedModel {
    use NestedAlias { alternateSubject as finalSubject; }
}
trait Unrelated { public function ordinary() { return 1; } }
trait RequiresSubject { abstract public function subject(); }
class AbstractRequirementModel extends ParentModel { use RequiresSubject; }
class UnqualifiedAliasModel {
    use Unrelated, DefaultRelation { subject as anotherSubject; subject as protected; }
}
class SecondTraitAliasModel {
    use Unrelated, DefaultRelation { DefaultRelation::subject as secondSubject; }
}
"#,
    );
    let loader = |name: &str| classes.get(&atom(name)).cloned();
    for (model, column, expected) in [
        ("TraitModel", "trait_type", true),
        ("TraitModel", "parent_type", false),
        ("TraitModel", "parent_trait_type", true),
        ("TraitModel", "inherited_type", true),
        ("OwnModel", "own_type", true),
        ("OwnModel", "trait_type", false),
        ("OwnModel", "parent_type", false),
        ("NoRelation", "parent_type", false),
        ("NoRelation", "trait_type", false),
        ("NoRelation", "parent_trait_type", false),
        ("NoRelation", "inherited_type", true),
        ("AdaptedModel", "trait_type", false),
        ("AdaptedModel", "other_type", true),
        ("AliasedModel", "trait_type", true),
        ("AliasedModel", "other_type", true),
        ("DefaultAliasedModel", "subject_type", true),
        ("DefaultAliasedModel", "alternate_subject_type", true),
        ("DefaultOverriddenModel", "subject_type", false),
        ("DefaultOverriddenModel", "alternate_subject_type", true),
        ("ExplicitNameAliasedModel", "subject_type", true),
        ("ExplicitNameAliasedModel", "alternate_subject_type", false),
        ("NestedAliasedModel", "subject_type", true),
        ("NestedAliasedModel", "alternate_subject_type", true),
        ("NestedAliasedModel", "final_subject_type", true),
        ("UnqualifiedAliasModel", "another_subject_type", true),
        ("UnqualifiedAliasModel", "ordinary_type", false),
        ("UnqualifiedAliasModel", "subject", false),
        ("AbstractRequirementModel", "parent_type", true),
        ("SecondTraitAliasModel", "second_subject_type", true),
    ] {
        assert_eq!(
            is_morph_type_column(&classes[&atom(model)], column, &loader),
            expected,
            "{model} discriminator {column}"
        );
    }
}

#[test]
fn morph_type_column_lookup_bounds_cyclic_inheritance() {
    let classes = classes(
        r#"<?php
class First extends Second {
    use MissingTrait { absent as alternative; }
}
class Second extends First {}
"#,
    );
    assert!(!is_morph_type_column(
        &classes[&atom("First")],
        "subject_type",
        &|name| classes.get(&atom(name)).cloned(),
    ));
}

#[test]
fn morph_type_column_lookup_uses_local_metadata_when_loader_cannot_reload() {
    let classes = classes(
        r#"<?php
class Comment {
    public function subject() { return $this->morphTo(); }
}
"#,
    );
    assert!(is_morph_type_column(
        &classes[&atom("Comment")],
        "subject_type",
        &|_| None,
    ));
    assert!(!is_morph_type_column(
        &classes[&atom("Comment")],
        "another_type",
        &|_| None,
    ));
}

#[test]
fn morph_type_column_lookup_reloads_raw_members_before_applying_overrides() {
    let classes = classes(
        r#"<?php
class BaseComment {
    public function subject() { return $this->morphTo(); }
}
class Comment extends BaseComment {}
"#,
    );
    let mut merged = classes[&atom("Comment")].as_ref().clone();
    merged.methods = classes[&atom("BaseComment")].methods.clone();
    assert!(is_morph_type_column(&merged, "subject_type", &|name| {
        classes.get(&atom(name)).cloned()
    }));
}

#[test]
fn morph_column_traits_preserve_literal_tables_and_dynamic_table_methods() {
    let classes = classes(
        r#"<?php
trait TableSource { protected $table = 'entries'; }
trait DynamicTable { public function getTable() { return tableName(); } }
trait AbstractTable { abstract public function getTable(); }
trait Unrelated { public function value() { return 'entries'; } }
trait MorphTable {
    protected $table = 'relationships';
    public function subject() { return $this->morphTo(); }
}
"#,
    );
    assert_eq!(
        classes[&atom("TableSource")]
            .laravel()
            .unwrap()
            .table_name
            .as_deref(),
        Some("entries"),
    );
    assert!(
        classes[&atom("DynamicTable")]
            .laravel()
            .unwrap()
            .has_get_table_method
    );
    assert!(classes[&atom("AbstractTable")].laravel().is_none());
    assert!(classes[&atom("Unrelated")].laravel().is_none());
    let metadata = classes[&atom("MorphTable")].laravel().unwrap();
    assert_eq!(metadata.table_name.as_deref(), Some("relationships"));
    assert_eq!(metadata.morph_type_columns, [(atom("subject"), None)]);
}

#[test]
fn morph_column_extraction_skips_bodies_outside_the_supplied_source() {
    let source = "<?php trait Subject { public function subject() { return $this->morphTo(); } }";
    crate::parser::with_parsed_program(source, "morph_missing_source", |program, _| {
        let mago_syntax::cst::Statement::Trait(trait_def) = program.statements.last().unwrap()
        else {
            panic!("expected trait fixture");
        };
        assert!(
            super::super::model_extraction::extract_morph_type_columns(
                trait_def.members.iter(),
                ""
            )
            .is_empty()
        );
    });
}
