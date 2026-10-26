//! Re-diagnosing only the functions an edit touched.
//!
//! The slow pass (type resolution for every expression in the file) is
//! what keeping an open file's diagnostics current costs, and on a large
//! file most of it is spent re-deriving diagnostics nothing changed.  When
//! an edit only changes the bodies of some functions and methods, every
//! diagnostic outside them is still right, only possibly at a shifted
//! position.  A partial pass re-runs the slow collectors for the edited
//! functions and carries every other diagnostic over from the last pass.
//!
//! Every pass leaves a [`SlowBaseline`] behind: the text it diagnosed, the
//! functions it found there, what the file declared, and what it reported.
//! The next pass compares the file against it.
//!
//! # When the previous result can be kept
//!
//! A partial pass runs only when all of these hold, and a full pass runs
//! otherwise:
//!
//! - **The index describes the text.**  The file's symbol map was built
//!   from exactly the text being diagnosed, and nothing was published
//!   while the pass ran.  Only such a pass is kept as a baseline.
//! - **Nothing else changed.**  No other file was published, and nothing
//!   else that diagnostics depend on (configuration, a deleted file) moved
//!   since the baseline's pass (see [`epoch`]).
//! - **Only bodies changed.**  Every byte outside the function and method
//!   bodies is the same: signatures, docblocks, imports, class headers,
//!   properties, constants, and top-level code (see [`units`]).
//! - **The declarations did not change.**  The parser reads a few method
//!   bodies into the class itself (an Eloquent relationship's related
//!   model, a `casts()` map), so a body edit can still change a
//!   declaration.  The file's classes, functions, and constants must match
//!   the baseline's.
//! - **Nothing else reads the edited bodies.**  A few kinds of inference
//!   read a body from outside it (see [`body_is_observable`]), and an edit
//!   to such a body can change what the rest of the file resolves to.
//!
//! A collector whose findings can depend on any body (whether a property
//! is ever written) is the exception to carrying diagnostics over: a
//! partial pass drops what it reported and runs it over the whole file.
//!
//! # What waits for the next save
//!
//! An edit to a body can also change a project-wide registry the file
//! feeds (a Laravel macro, gate, or runtime config key registered in a
//! service provider).  Other open files only see that on save, and so do
//! the rest of the edited file's functions: a save always runs a full pass.

pub(crate) mod epoch;
pub(crate) mod region;
mod shift;
mod units;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::Diagnostic;

use crate::Backend;
use crate::parser::with_parsed_program;
use crate::types::{ANONYMOUS_CLASS_PREFIX, ClassInfo};

pub(crate) use region::reports_site;
use shift::ChangedSpan;
use units::DiagnosticUnit;

/// What a slow pass leaves for the next one to start from.
pub(crate) struct SlowBaseline {
    /// The text the pass diagnosed.
    content: Arc<String>,
    /// The functions and methods in that text.
    units: Vec<DiagnosticUnit>,
    /// What the file declared.
    declarations: DeclarationSnapshot,
    /// Where the publication counters stood when the pass started.
    epoch: epoch::EpochSnapshot,
    /// What the pass reported.
    diagnostics: Vec<Diagnostic>,
}

/// The outcome of [`Backend::collect_slow_diagnostics_incremental`].
pub(crate) struct SlowPass {
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// What the next pass can start from, or `None` when this one cannot
    /// be trusted to describe a single state of the index.
    pub(crate) baseline: Option<Arc<SlowBaseline>>,
    pub(crate) kind: SlowPassKind,
    /// The other files whose function or method bodies this pass read.
    /// A pass that was not [`SlowPassKind::Full`] saw only the reads of
    /// what it diagnosed again.
    pub(crate) bodies_read_from: HashSet<String>,
}

/// How a slow pass arrived at its diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlowPassKind {
    /// Every collector ran over the whole file.
    Full(FullPassReason),
    /// Only the edited functions and methods were diagnosed again.
    Partial { units: usize },
    /// The text had not changed, so the previous result was kept as is.
    Reused,
}

/// Why a slow pass could not start from the previous one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FullPassReason {
    /// There is no previous pass to start from.
    NoBaseline,
    /// The file is a Blade template, diagnosed as the PHP it lowers to.
    Template,
    /// The index is still catching up with the text.
    IndexBehind,
    /// Something besides this file changed.
    ProjectChanged,
    /// The edit reached outside the function and method bodies.
    OutsideBodies,
    /// The file declares something different.
    DeclarationsChanged,
    /// Code outside an edited body can read it.
    BodyObservable,
    /// A previous diagnostic could not be moved to the edited text.
    Unmovable,
}

/// What a file declares, from the workspace index.
///
/// Anonymous classes are left out: their names carry their offset, so they
/// change name whenever an edit above them moves them, and nothing outside
/// the function holding one can name it anyway.
struct DeclarationSnapshot {
    classes: Vec<Arc<ClassInfo>>,
    functions: Vec<String>,
    constants: Vec<(String, Option<String>)>,
}

impl DeclarationSnapshot {
    fn capture(backend: &Backend, uri: &str) -> Self {
        let classes = backend
            .symbols
            .uri_classes_index
            .read()
            .get(uri)
            .map(|classes| {
                classes
                    .iter()
                    .filter(|class| !class.name.starts_with(ANONYMOUS_CLASS_PREFIX))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let (mut functions, constant_names) = backend
            .symbols
            .uri_globals_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        functions.sort_unstable();
        let mut constants: Vec<(String, Option<String>)> = {
            let defines = backend.symbols.global_defines.read();
            constant_names
                .into_iter()
                .map(|name| {
                    let value = defines
                        .get(&name)
                        .filter(|define| define.file_uri == uri)
                        .and_then(|define| define.value.clone());
                    (name, value)
                })
                .collect()
        };
        constants.sort_unstable();
        Self {
            classes,
            functions,
            constants,
        }
    }

    fn matches(&self, other: &Self) -> bool {
        self.classes.len() == other.classes.len()
            && self
                .classes
                .iter()
                .zip(&other.classes)
                .all(|(a, b)| a.signature_eq(b))
            && self.functions == other.functions
            && self.constants == other.constants
    }
}

/// Methods whose bodies are read by name, from anywhere a class is used.
///
/// - `rules`: a form request's validation rules are read out of its
///   `rules()` body to type `validated()` and check the keys requested
///   from it.
/// - `getFacadeAccessor`: a facade's target class is read out of the
///   accessor's body.
const BODIES_READ_BY_NAME: &[&str] = &["rules", "getFacadeAccessor"];

/// Whether code outside `unit` can read its body, so that an edit inside
/// it can change what the rest of the file resolves to.  `previous` is the
/// same unit in the baseline's text.
///
/// Each case mirrors a place that reads a body it is not walking:
///
/// - Inference read it during an earlier pass ([`DiagnosticUnit::observed`]):
///   a method's return type left open by its declaration, or what a
///   by-reference parameter is left holding.  The reads are recorded as
///   they happen ([`crate::type_engine::body_reads`]), whichever file's
///   code asked, so a call that reaches this body through another file is
///   counted too.
/// - A function declared inside the body is global once the body runs.
/// - The constructor decides what the class's readonly properties hold in
///   every other method, and which of their writes come too late.
/// - Other magic methods answer accesses that never spell their name.
/// - [`BODIES_READ_BY_NAME`], read straight from the source.
fn body_is_observable(content: &str, unit: &DiagnosticUnit, previous: &DiagnosticUnit) -> bool {
    if previous.observed || unit.declares_function || previous.declares_function {
        return true;
    }
    let name = unit.name(content);
    name.starts_with("__")
        || BODIES_READ_BY_NAME
            .iter()
            .any(|read| name.eq_ignore_ascii_case(read))
}

/// The previous result, moved to the new text, plus what is left to
/// compute.
enum Plan {
    /// Nothing changed; keep the previous diagnostics.
    Reuse(Vec<Diagnostic>),
    /// Diagnose the changed spans again (walking the bodies that open at
    /// `targets`) and keep `carried` for the rest.
    Partial {
        changed: Vec<ChangedSpan>,
        targets: Vec<u32>,
        carried: Vec<Diagnostic>,
    },
}

impl Backend {
    /// Collect the slow diagnostics for an open file, re-diagnosing only
    /// what changed since `baseline` when that is sound (see the module
    /// documentation).
    ///
    /// Returns the diagnostics together with the baseline the next pass
    /// should start from.
    pub(crate) fn collect_slow_diagnostics_incremental(
        &self,
        uri: &str,
        content: &Arc<String>,
        baseline: Option<&SlowBaseline>,
    ) -> SlowPass {
        let reads = crate::type_engine::body_reads::record_body_reads();

        // A template is diagnosed as the PHP it lowers to, through a source
        // map; neither the lowering nor the map is tracked here.
        if let Some(virtual_php) = self.blade_virtual_php_arc(uri) {
            let _parse_guard = crate::parser::with_parse_cache_arc(Arc::clone(&virtual_php));
            let mut diagnostics = Vec::new();
            self.collect_slow_diagnostics(uri, &virtual_php, &mut diagnostics);
            return SlowPass {
                diagnostics,
                baseline: None,
                kind: SlowPassKind::Full(FullPassReason::Template),
                bodies_read_from: reads.files_other_than(uri),
            };
        }

        // Taken before anything is read from the index, so a publication
        // landing from here on is seen by the end-of-pass check below.
        let epoch = self.diag.epoch.snapshot(uri);
        let symbol_map = self.symbol_map_for(uri);
        let index_current = epoch.is_some()
            && symbol_map
                .as_ref()
                .is_some_and(|map| map.built_from(content));

        // The collectors below parse the file through this same cache.
        let _parse_guard = crate::parser::with_parse_cache_arc(Arc::clone(content));
        let mut units = with_parsed_program(content, "diagnostic_units", |program, _| {
            units::collect(program)
        });
        if let Some(map) = &symbol_map {
            units::mark_function_declarations(&mut units, map);
        }
        let declarations = DeclarationSnapshot::capture(self, uri);

        let plan = match (baseline, epoch) {
            _ if !index_current => Err(FullPassReason::IndexBehind),
            (Some(baseline), Some(now)) => {
                self.plan_partial(baseline, content, &units, &declarations, now)
            }
            _ => Err(FullPassReason::NoBaseline),
        };

        let (diagnostics, kind) = match plan {
            Ok(Plan::Reuse(diagnostics)) => (diagnostics, SlowPassKind::Reused),
            Ok(Plan::Partial {
                changed,
                targets,
                carried,
            }) => {
                let spans: Vec<(u32, u32)> = changed.iter().map(|span| span.new).collect();
                let mut fresh = Vec::new();
                self.collect_slow_diagnostics_in_region(uri, content, &spans, &targets, &mut fresh);
                shift::retain_within(content, &spans, &mut fresh);
                let mut diagnostics = carried;
                diagnostics.retain(|diagnostic| !super::collect::reads_every_body(diagnostic));
                diagnostics.extend(fresh);
                self.collect_slow_diagnostics_reading_every_body(uri, content, &mut diagnostics);
                (
                    diagnostics,
                    SlowPassKind::Partial {
                        units: changed.len(),
                    },
                )
            }
            Err(reason) => {
                let mut diagnostics = Vec::new();
                self.collect_slow_diagnostics(uri, content, &mut diagnostics);
                (diagnostics, SlowPassKind::Full(reason))
            }
        };

        // A pass that only diagnosed the edited functions saw only their
        // reads; the rest of the file still reads what it read before.
        if let (SlowPassKind::Partial { .. } | SlowPassKind::Reused, Some(baseline)) =
            (kind, baseline)
        {
            for (unit, previous) in units.iter_mut().zip(&baseline.units) {
                unit.observed = previous.observed;
            }
        }
        units::mark_observed(&mut units, &reads.offsets_in(uri));
        let bodies_read_from = reads.files_other_than(uri);
        drop(reads);

        let baseline = epoch
            .filter(|&now| index_current && self.diag.epoch.unchanged_since(now))
            .map(|now| {
                Arc::new(SlowBaseline {
                    content: Arc::clone(content),
                    units,
                    declarations,
                    epoch: now,
                    diagnostics: diagnostics.clone(),
                })
            });

        SlowPass {
            diagnostics,
            baseline,
            kind,
            bodies_read_from,
        }
    }

    /// Work out what a partial pass has left to do, or why it cannot run.
    fn plan_partial(
        &self,
        baseline: &SlowBaseline,
        content: &str,
        units: &[DiagnosticUnit],
        declarations: &DeclarationSnapshot,
        now: epoch::EpochSnapshot,
    ) -> Result<Plan, FullPassReason> {
        if !now.only_own_changes_since(baseline.epoch) {
            return Err(FullPassReason::ProjectChanged);
        }
        let changed = units::changed_units(&baseline.content, &baseline.units, content, units)
            .ok_or(FullPassReason::OutsideBodies)?;
        if !baseline.declarations.matches(declarations) {
            return Err(FullPassReason::DeclarationsChanged);
        }
        if changed.is_empty() {
            return Ok(Plan::Reuse(baseline.diagnostics.clone()));
        }

        if changed
            .iter()
            .any(|&i| body_is_observable(content, &units[i], &baseline.units[i]))
        {
            return Err(FullPassReason::BodyObservable);
        }
        let spans: Vec<ChangedSpan> = changed
            .iter()
            .map(|&i| ChangedSpan {
                old: baseline.units[i].span,
                new: units[i].span,
            })
            .collect();

        let carried = shift::carry_over(&baseline.content, content, &spans, &baseline.diagnostics)
            .ok_or(FullPassReason::Unmovable)?;
        Ok(Plan::Partial {
            changed: spans,
            targets: changed.iter().map(|&i| units[i].body.0).collect(),
            carried,
        })
    }
}
