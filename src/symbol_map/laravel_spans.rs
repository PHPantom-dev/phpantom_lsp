//! The Laravel string keys a symbol map records, the spans that only
//! count while some workspace symbol is absent, and the side tables for
//! gate subjects and typed render receivers.

use crate::atom::Atom;

use super::{SubjectText, SymbolKind, SymbolMap, SymbolSpan};

/// A Laravel resource whose short name is declared under a config subtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LaravelConfigResource {
    /// A guard under `auth.guards`.
    AuthGuard,
    /// A cache store under `cache.stores`.
    CacheStore,
    /// A logging channel under `logging.channels`.
    LogChannel,
    /// A filesystem disk under `filesystems.disks`.
    StorageDisk,
    /// A database connection under `database.connections`.
    DatabaseConnection,
    /// A queue connection under `queue.connections`.
    QueueConnection,
    /// A configured mailer under `mail.mailers`.
    Mailer,
    /// A broadcast connection under `broadcasting.connections`.
    BroadcastConnection,
}

impl LaravelConfigResource {
    /// The resource's unique bit in a zero-allocation family-presence set.
    pub(crate) const fn bit(self) -> u8 {
        match self {
            Self::AuthGuard => 1 << 0,
            Self::CacheStore => 1 << 1,
            Self::LogChannel => 1 << 2,
            Self::StorageDisk => 1 << 3,
            Self::DatabaseConnection => 1 << 4,
            Self::QueueConnection => 1 << 5,
            Self::Mailer => 1 << 6,
            Self::BroadcastConnection => 1 << 7,
        }
    }
}

/// Identifies the category of a [`SymbolKind::LaravelStringKey`] span.
///
/// Adding a new Laravel navigation feature only requires adding a variant
/// here and updating the extraction and dispatch paths — the exhaustive
/// match arms in `highlight`, `hover`, `rename`, `semantic_tokens`, and
/// `type_definition` do not need to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LaravelStringKind {
    /// A `config('dot.key')` or `Config::get('dot.key')` call.
    Config,
    /// A short name whose declaration is a direct child of a known config
    /// subtree, such as a storage disk or database connection.
    ConfigResource(LaravelConfigResource),
    /// A `view('name')` or `View::make('name')` call.
    View,
    /// A `route('name')` call.
    Route,
    /// A `__('key')`, `trans('key')`, or `Lang::get('key')` call.
    Trans,
    /// An Artisan command name, e.g. `Artisan::call('app:sync')`,
    /// `Schedule::command('app:sync')`, or `$this->call('app:sync')`
    /// inside a console command.
    Command,
    /// An Eloquent morph alias: the short name a `Relation::morphMap()`
    /// registration substitutes for a model FQCN in a `*_type` column.
    /// Emitted for the registration's own keys, for
    /// `Relation::getMorphedModel('post')`, and for the `$types` argument of
    /// the `whereHasMorph()` family.
    MorphAlias,
    /// A Blade section name: the string `@yield('content')` renders and
    /// `@section('content')` fills. Emitted for the marker calls the Blade
    /// preprocessor lowers those directives to.
    Section,
    /// A Blade stack name, the `@stack`/`@push` counterpart of
    /// [`Self::Section`].
    Stack,
    /// An authorization ability name: the string a `Gate::allows()`,
    /// `$user->can()`, `$this->authorize()`, `can:` middleware parameter, or
    /// Blade `@can` directive checks.  Resolves against `Gate::define()`
    /// registrations and the methods of the relevant model's policy.
    GateAbility,
    /// A service-container binding key: the string `app('payments')`,
    /// `resolve('payments')`, `App::make('payments')`, or
    /// `$this->app->make('payments')` asks the container for, and the one a
    /// service provider registers it under.
    ContainerBinding,
    /// An environment variable name: the string `env('APP_NAME')` or
    /// `Env::get('APP_NAME')` reads, resolving to the line that declares it
    /// in the project's `.env`.
    Env,
}

impl LaravelStringKind {
    /// Whether this key resolves through Laravel's config index.
    pub(crate) const fn is_config_backed(self) -> bool {
        matches!(self, Self::Config | Self::ConfigResource(_))
    }
}

/// A workspace symbol whose presence suppresses an otherwise-valid Laravel
/// runtime fallback.
///
/// PHP first looks for a namespace-local function before falling back to a
/// global helper, and Laravel's root facade aliases only exist when no real
/// global class has claimed the same name. These dependencies are stored on
/// the handful of ambiguous string spans so edits to another file can toggle
/// the spans without reparsing either file or adding work to request paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LaravelStringDependency {
    Function(Atom),
    Class(Atom),
}

impl LaravelStringDependency {
    /// The namespace-local `auth` function that may prevent PHP from reaching
    /// Laravel's global helper.
    pub(crate) fn namespaced_auth(name: &str) -> Option<Self> {
        let (namespace, short) = name.rsplit_once('\\')?;
        (!namespace.is_empty() && short.eq_ignore_ascii_case("auth"))
            .then(|| Self::Function(crate::atom::ascii_lowercase_atom(name)))
    }

    /// A global class name Laravel may otherwise provide as a runtime facade
    /// alias. Fully-qualified facade classes are unambiguous and return none.
    pub(crate) fn root_facade(name: &str) -> Option<Self> {
        let name = name.trim_start_matches('\\');
        if name.contains('\\') {
            return None;
        }
        let is_alias = crate::symbol_map::laravel_resources::RESOURCE_FACADES
            .iter()
            .any(|alias| name.eq_ignore_ascii_case(alias))
            || match name.len() {
                3 => name.eq_ignore_ascii_case("App"),
                4 => {
                    name.eq_ignore_ascii_case("Gate")
                        || name.eq_ignore_ascii_case("Lang")
                        || name.eq_ignore_ascii_case("View")
                }
                5 => name.eq_ignore_ascii_case("Route"),
                6 => name.eq_ignore_ascii_case("Config"),
                7 => name.eq_ignore_ascii_case("Artisan"),
                8 => name.eq_ignore_ascii_case("Response") || name.eq_ignore_ascii_case("Schedule"),
                _ => false,
            };
        is_alias.then(|| Self::Class(crate::atom::ascii_lowercase_atom(name)))
    }
}

/// A Laravel string span whose meaning depends on a workspace symbol being
/// absent. The payload moves between this side table and [`SymbolMap::spans`]
/// as it becomes dormant/active, so an active candidate does not duplicate
/// its key string in memory.
#[derive(Debug, Clone)]
pub(crate) struct ConditionalLaravelStringSpan {
    pub dependency: LaravelStringDependency,
    start: u32,
    end: u32,
    dormant_span: Option<SymbolSpan>,
}

impl ConditionalLaravelStringSpan {
    pub(crate) fn new(dependency: LaravelStringDependency, span: SymbolSpan) -> Self {
        Self {
            dependency,
            start: span.start,
            end: span.end,
            dormant_span: Some(span),
        }
    }

    fn is_inserted(&self) -> bool {
        self.dormant_span.is_none()
    }

    /// Heap bytes owned by a dormant payload for the optional memory audit.
    #[cfg(feature = "mem-audit")]
    pub(crate) fn audit_heap(&self) -> usize {
        self.dormant_span
            .as_ref()
            .map_or(0, |span| span.kind.audit_heap())
    }
}

fn is_candidate_span(span: &SymbolSpan, candidate: &ConditionalLaravelStringSpan) -> bool {
    span.start == candidate.start
        && span.end == candidate.end
        && matches!(&span.kind, SymbolKind::LaravelStringKey { .. })
}

/// The model a gate check names, recorded alongside its ability span.
///
/// Kept in a side table rather than inside [`SymbolKind::GateAbility`] so
/// that the (very hot) [`SymbolKind`] enum does not grow for every span in
/// every file: only the handful of gate calls in a project contribute an
/// entry, and only the unknown-ability diagnostic reads them.
#[derive(Debug, Clone)]
pub(crate) struct GateSubject {
    /// Start offset of the [`SymbolKind::GateAbility`] span this describes.
    pub ability_start: u32,
    /// Source text of the model expression — `Post` for `Post::class`,
    /// `$post` for a variable.
    pub subject_text: SubjectText,
    /// Whether the subject is a class name (`Post::class`) rather than a
    /// runtime value, which decides how it is resolved to a type.
    pub is_static: bool,
}

// ─── Render sites behind a typed receiver ───────────────────────────────────

/// The class a receiver has to be an instance of for the call it receives
/// to name a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewReceiverClass {
    /// Laravel's view factory, whose `make()` / `first()` / `exists()` /
    /// `renderWhen()` / `renderUnless()` / `renderEach()` all name one.
    Factory,
    /// A mailable, whose `text()` names the plain-text template the message
    /// is built from.
    Mailable,
    /// Anything a mail template is set on: a mailable, or the mail message a
    /// notification's `toMail()` returns.  The two share no base class, but
    /// `view()` and `markdown()` mean the same thing on both.
    MailTemplate,
}

impl ViewReceiverClass {
    /// The fully qualified names a receiver's type must be a subtype of one
    /// of.
    pub(crate) fn fqns(self) -> &'static [&'static str] {
        match self {
            // The concrete `Illuminate\View\Factory` implements the
            // contract, so the contract covers both spellings.
            Self::Factory => &["Illuminate\\Contracts\\View\\Factory"],
            Self::Mailable => &["Illuminate\\Mail\\Mailable"],
            Self::MailTemplate => &[
                "Illuminate\\Mail\\Mailable",
                "Illuminate\\Notifications\\Messages\\MailMessage",
            ],
        }
    }
}

/// A method call that names a template but whose viewness lives in its
/// receiver's *type* rather than its spelling: a constructor-injected
/// `Factory $views` behind `$this->views->make('page')`, or a mailable
/// held in a local behind `$mail->view('emails.shipped')`.
///
/// [`extract_symbol_map`] runs during `update_ast`, before the file's
/// classes are resolved, and cannot ask the forward walker what the
/// receiver is. It records the site instead, and
/// `Backend::typed_receiver_view_spans` settles it lazily.
#[derive(Debug, Clone)]
pub(crate) struct ViewReceiverSite {
    /// Byte offset of the first character of the view-name string's
    /// contents, matching what a [`SymbolKind::LaravelStringKey`] span
    /// records.
    pub start: u32,
    /// Byte offset one past the last character of those contents.
    pub end: u32,
    /// The view name in its canonical dotted spelling.
    pub key: String,
    /// Whether the call renders whichever of several candidates exists,
    /// so a name matching no template is the shape working as intended.
    pub is_optional: bool,
    /// What the receiver has to be for this to name a template at all.
    pub receiver: ViewReceiverClass,
}

impl ViewReceiverSite {
    /// The span this site contributes once its receiver is confirmed.
    pub(crate) fn to_span(&self) -> SymbolSpan {
        SymbolSpan {
            start: self.start,
            end: self.end,
            kind: SymbolKind::LaravelStringKey {
                key: self.key.clone(),
                kind: LaravelStringKind::View,
                is_write: false,
                is_optional: self.is_optional,
            },
        }
    }
}

impl SymbolMap {
    /// Whether this map has a dormant span gated by one of `affected`.
    pub(crate) fn has_conditional_laravel_dependency(
        &self,
        affected: Option<&std::collections::HashSet<LaravelStringDependency>>,
    ) -> bool {
        self.conditional_laravel_spans.iter().any(|candidate| {
            affected.is_none_or(|dependencies| dependencies.contains(&candidate.dependency))
        })
    }

    /// Whether reconciling the selected dependencies would add or remove at
    /// least one span. This lets the publication path avoid cloning an
    /// already-correct map merely to discover that it is unchanged.
    pub(crate) fn conditional_laravel_spans_need_refresh(
        &self,
        affected: Option<&std::collections::HashSet<LaravelStringDependency>>,
        mut dependency_exists: impl FnMut(LaravelStringDependency) -> bool,
    ) -> bool {
        self.conditional_laravel_spans.iter().any(|candidate| {
            affected.is_none_or(|dependencies| dependencies.contains(&candidate.dependency))
                && candidate.is_inserted() == dependency_exists(candidate.dependency)
        })
    }

    /// Reconcile dormant Laravel spans against the current workspace symbol
    /// indexes. When `affected` is supplied, candidates with any other
    /// dependency are skipped, keeping cross-file edits proportional to the
    /// uncommon ambiguous call sites rather than to every span in a file.
    pub(crate) fn refresh_conditional_laravel_spans(
        &mut self,
        affected: Option<&std::collections::HashSet<LaravelStringDependency>>,
        mut dependency_exists: impl FnMut(LaravelStringDependency) -> bool,
    ) -> bool {
        let mut changed = false;
        let mut added = false;
        for candidate in &mut self.conditional_laravel_spans {
            if affected.is_some_and(|set| !set.contains(&candidate.dependency)) {
                continue;
            }
            let should_insert = !dependency_exists(candidate.dependency);
            if should_insert == candidate.is_inserted() {
                continue;
            }

            if should_insert {
                self.spans.push(
                    candidate
                        .dormant_span
                        .take()
                        .expect("a dormant candidate must own its span"),
                );
                added = true;
            } else {
                let existing = self
                    .spans
                    .iter()
                    .position(|span| is_candidate_span(span, candidate))
                    .expect("an active candidate must be present in the symbol map");
                candidate.dormant_span = Some(self.spans.remove(existing));
            }
            changed = true;
        }
        if added {
            self.spans.sort_by_key(|span| span.start);
        }
        if changed {
            self.member_access_indices.clear();
            for (index, span) in self.spans.iter().enumerate() {
                if let SymbolKind::MemberAccess { member_name, .. } = &span.kind {
                    self.member_access_indices
                        .entry(*member_name)
                        .or_default()
                        .push(index);
                }
            }
        }
        changed
    }
}
