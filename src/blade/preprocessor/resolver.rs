/// Resolves the class a component tag names, so the preprocessor can bind
/// `$component` to it and check the tag's attributes against the call the
/// framework makes with them.
///
/// The preprocessor never reaches into the project index itself: it runs
/// on every keystroke and from the parallel index workers, where building
/// the Blade discovery index would put a workspace walk on the edit path.
/// The caller passes in whatever index it already has, and a tag it cannot
/// answer for degrades to a comment.
pub trait ComponentResolver {
    /// The class an `<x-…>` tag names: the component class behind a
    /// class-based component, or `Illuminate\View\AnonymousComponent` for
    /// a tag that names a template with no class of its own.
    fn x_component(&self, tag: &str) -> Option<ComponentTarget>;

    /// The class a `<livewire:…>` tag names.
    fn livewire_component(&self, name: &str) -> Option<ComponentTarget>;
}

/// The class a component tag names, and what the tag's attributes are to
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentTarget {
    /// Fully qualified class name, without a leading `\`.
    pub fqn: String,
    pub binding: ComponentBinding,
}

/// How a resolved component tag reaches its class.
///
/// Laravel partitions a tag's attributes by the signature it is about to
/// call: the ones naming a parameter are its arguments and the rest go to
/// the component's attribute bag (`ComponentTagCompiler::partitionDataAndAttributes`).
/// Reproducing that split is what lets the attributes be checked as the
/// arguments they are without an attribute meant for the bag being read as
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentBinding {
    /// `$component = new \Fqn(heading: 'Latest', post: $post);` — a Blade
    /// component's attributes are its constructor's arguments.
    Construct(Vec<ComponentParameter>),
    /// `$component = new \Fqn(); $component->mount(post: $post);` — a
    /// Livewire component is built by the container and handed its
    /// attributes through `mount()`.
    Mount(Vec<ComponentParameter>),
    /// `/** @var \Fqn $component */ $component = null;` — the class is
    /// known but the tag's attributes are arguments to nothing: an
    /// anonymous component's attributes are its *view's* variables rather
    /// than a signature's.
    Declare,
}

/// One parameter a component tag's attributes can fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentParameter {
    /// The parameter name (no `$`), which an attribute has to camel-case
    /// to in order to fill it.
    pub name: String,
    /// What the call passes when no attribute fills this parameter:
    /// `null` for a nullable one and `resolve(\Foo::class)` for one the
    /// container can build, which is how Laravel itself fills a
    /// constructor the tag left incomplete.  `None` when the parameter
    /// has a default (the call just omits it) or when nothing stands in
    /// for it, which is the case Laravel fails on and the missing-argument
    /// diagnostic is right to report.
    pub fallback: Option<String>,
}
