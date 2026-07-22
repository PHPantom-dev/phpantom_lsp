//! `ScopeMap` and the variable-access data types the forward-pass
//! collector produces, plus the query API used by the code-action and
//! diagnostic consumers.
//!
//! These types have no dependency on the AST walker; they are the
//! *output* of a collection pass (see [`super::collector`] and
//! [`super::build`]).

// ─── By-reference parameter resolution ──────────────────────────────────────

/// Describes a call expression so the by-ref resolver can look up
/// the callee's parameter list.
pub(crate) enum ByRefCallKind<'a> {
    /// A standalone function call (e.g. `myFunc($var)`).
    Function(&'a str),
    /// A static method call (e.g. `Cls::method($var)`).
    StaticMethod(&'a str, &'a str),
    /// A constructor call (e.g. `new Cls($var)`).
    Constructor(&'a str),
    /// An instance method call whose receiver class is known
    /// (e.g. `$this->method($var)`, `new A()->method($var)`).
    /// The first string is the class name, the second the method name.
    InstanceMethod(&'a str, &'a str),
}

/// Callback that resolves by-reference parameter positions for a call.
///
/// Given a [`ByRefCallKind`] describing the call, returns a list of
/// 0-based argument positions that are by-reference.  Returns `None`
/// if the function/method cannot be resolved.
pub(crate) type ByRefResolver<'a> = &'a dyn Fn(&ByRefCallKind<'_>) -> Option<Vec<usize>>;

// ─── Core types ─────────────────────────────────────────────────────────────

/// Whether a variable access is a read or a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessKind {
    /// The variable is being read (e.g. `foo($x)`, `return $x`).
    Read,
    /// The variable is being written (e.g. `$x = …`, parameter decl,
    /// foreach binding, catch binding).
    Write,
    /// The variable is being both read and written (e.g. `$x .= …`,
    /// `$x++`, `$x--`, `$x += …`).
    ReadWrite,
}

/// A single variable access (read or write) at a specific byte offset.
#[derive(Debug, Clone)]
pub(crate) struct VarAccess {
    /// Variable name **with** `$` prefix (e.g. `$x`, `$this`).
    pub name: String,
    /// Byte offset of the `$` character in the source.
    pub offset: u32,
    /// Whether this is a read, write, or read-write access.
    pub kind: AccessKind,
}

/// A scope frame representing a function, closure, or arrow function body.
///
/// Each frame records its own variable accesses.  Frames form a tree
/// (closures nested inside functions, etc.), but for the initial
/// implementation we store them in a flat vec and use byte-range
/// containment to determine nesting.
#[derive(Debug, Clone)]
pub(crate) struct Frame {
    /// Byte offset of the frame's opening boundary.
    ///
    /// For functions/methods: the opening `{` of the body.
    /// For closures: the opening `{` of the body.
    /// For arrow functions: the `=>` token offset.
    /// For catch blocks: the opening `{` of the catch body.
    /// For top-level code: `0`.
    pub start: u32,
    /// Byte offset of the frame's closing boundary.
    ///
    /// For functions/methods/closures/catch: the closing `}`.
    /// For arrow functions: the end of the body expression.
    /// For top-level code: `u32::MAX`.
    pub end: u32,
    /// What kind of scope boundary this frame represents.
    pub kind: FrameKind,
    /// Variables explicitly captured via `use($x, &$y)` in closures.
    /// Each entry is `(name_with_dollar, is_by_reference)`.
    ///
    /// Populated during collection; read by the unused-variable
    /// diagnostic to skip by-reference captures, and by Extract Function
    /// to detect closure captures that cross extraction boundaries.
    pub captures: Vec<(String, bool)>,
    /// Parameter names (with `$` prefix) declared on this frame.
    ///
    /// Populated for functions, methods, closures, and arrow functions.
    /// Used by the undefined-variable diagnostic to identify which
    /// variables are defined as parameters (their write offsets are
    /// before the frame's `start` and cannot be distinguished from
    /// outer-scope writes by offset alone).
    pub parameters: Vec<String>,
}

/// The kind of scope boundary a [`Frame`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameKind {
    /// Top-level code (outside any function/class).
    TopLevel,
    /// Named function: `function foo() { … }`
    Function,
    /// Class method (regular, static, abstract with body, etc.).
    Method,
    /// Closure: `function($x) use($y) { … }`
    Closure,
    /// Arrow function: `fn($x) => expr`
    ArrowFunction,
    /// Catch block: `catch (E $e) { … }`
    Catch,
}

/// The result of a scope collection pass.
///
/// Contains all variable accesses organised by frame, plus a query API
/// for extracting parameter / return-value / local sets for a given
/// byte range.
#[derive(Debug, Clone, Default)]
pub(crate) struct ScopeMap {
    /// All variable accesses across all frames, in source order.
    pub accesses: Vec<VarAccess>,
    /// All scope frames, sorted by `start` offset.
    pub frames: Vec<Frame>,
    /// Whether `$this`, `self::`, `static::`, or `parent::` appears
    /// anywhere in the collected region.  Set during collection.
    pub has_this_or_self: bool,
    /// Whether any by-reference parameter (`&$var`) was encountered.
    ///
    /// Used by Extract Function to detect when by-reference semantics
    /// would make extraction unsafe.
    pub has_reference_params: bool,
}

impl ScopeMap {
    /// Whether the enclosing scope uses by-reference parameters.
    ///
    /// When `true`, variable extraction must be careful about
    /// reference semantics — a variable modified via `&$var` in the
    /// extracted range may need to be passed by reference to the new
    /// function.
    pub(crate) fn uses_reference_params(&self) -> bool {
        self.has_reference_params
    }
}

/// Variables classified by their role relative to a byte range.
///
/// Returned by [`ScopeMap::classify_range`].
#[derive(Debug, Clone, Default)]
pub(crate) struct RangeClassification {
    /// Variables **read** inside `[start, end)` whose most recent
    /// write is **before** `start`.  These would become parameters
    /// of an extracted function.
    pub parameters: Vec<String>,
    /// Variables **written** inside `[start, end)` that are **read
    /// after** `end` in the enclosing scope.  These would become
    /// return values of an extracted function.
    pub return_values: Vec<String>,
    /// Variables whose entire lifetime (first write to last read) is
    /// contained within `[start, end)`.  These stay inside the
    /// extracted function.
    pub locals: Vec<String>,
    /// Whether `$this`, `self::`, `static::`, or `parent::` appears
    /// in the range.
    pub uses_this: bool,
    /// Variables that are written by reference (`&$var`) inside the
    /// range.
    pub reference_writes: Vec<String>,
}

// ─── ScopeMap query API ─────────────────────────────────────────────────────

impl ScopeMap {
    /// Find the innermost frame that fully contains the given offset.
    pub(crate) fn enclosing_frame(&self, offset: u32) -> Option<&Frame> {
        // Iterate in reverse so we find the innermost (most recently
        // opened) frame first.  Frames are sorted by start offset.
        self.frames
            .iter()
            .rev()
            .find(|f| offset >= f.start && offset <= f.end)
    }

    /// Find the innermost frame that fully contains the given range.
    pub(crate) fn enclosing_frame_for_range(&self, start: u32, end: u32) -> Option<&Frame> {
        self.frames
            .iter()
            .rev()
            .find(|f| start >= f.start && end <= f.end)
    }

    /// Return all accesses of variable `name` within the given frame,
    /// excluding accesses that fall inside a nested frame (closure or
    /// arrow function).
    pub(crate) fn accesses_in_frame<'a>(&'a self, name: &str, frame: &Frame) -> Vec<&'a VarAccess> {
        self.accesses
            .iter()
            .filter(|a| a.name == name && a.offset >= frame.start && a.offset <= frame.end)
            .filter(|a| {
                !self.frames.iter().any(|f| {
                    f.start > frame.start
                        && f.end < frame.end
                        && a.offset >= f.start
                        && a.offset <= f.end
                        && f.kind != FrameKind::Catch
                })
            })
            .collect()
    }

    /// Classify variables relative to a byte range `[start, end)`.
    ///
    /// This is the primary query for Extract Function: it determines
    /// which variables become parameters, return values, or locals.
    pub(crate) fn classify_range(&self, start: u32, end: u32) -> RangeClassification {
        let frame = match self.enclosing_frame_for_range(start, end) {
            Some(f) => f,
            None => return RangeClassification::default(),
        };

        // Collect all unique variable names accessed within the range
        // (excluding nested frames and pseudo-variables).
        let mut var_names: Vec<String> = Vec::new();
        for access in &self.accesses {
            if access.offset >= start
                && access.offset < end
                && !var_names.contains(&access.name)
                && access.name != "$this"
                && access.name != "self"
                && access.name != "static"
                && access.name != "parent"
            {
                // Skip if inside a nested frame.
                let in_nested = self.frames.iter().any(|f| {
                    f.start > frame.start
                        && f.end < frame.end
                        && access.offset >= f.start
                        && access.offset <= f.end
                        && f.kind != FrameKind::Catch
                });
                if !in_nested {
                    var_names.push(access.name.clone());
                }
            }
        }

        // Check for $this / self / static / parent usage in range.
        let mut result = RangeClassification {
            uses_this: self.accesses.iter().any(|a| {
                a.offset >= start
                    && a.offset < end
                    && (a.name == "$this"
                        || a.name == "self"
                        || a.name == "static"
                        || a.name == "parent")
            }),
            ..Default::default()
        };

        for var_name in &var_names {
            let frame_accesses = self.accesses_in_frame(var_name, frame);

            let has_write_before = frame_accesses.iter().any(|a| {
                a.offset < start && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
            });

            let has_read_inside = frame_accesses.iter().any(|a| {
                a.offset >= start
                    && a.offset < end
                    && matches!(a.kind, AccessKind::Read | AccessKind::ReadWrite)
            });

            let has_write_inside = frame_accesses.iter().any(|a| {
                a.offset >= start
                    && a.offset < end
                    && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
            });

            let has_read_after = frame_accesses.iter().any(|a| {
                a.offset >= end && matches!(a.kind, AccessKind::Read | AccessKind::ReadWrite)
            });

            let first_write = frame_accesses
                .iter()
                .filter(|a| matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite))
                .min_by_key(|a| a.offset);

            let last_read = frame_accesses
                .iter()
                .filter(|a| matches!(a.kind, AccessKind::Read | AccessKind::ReadWrite))
                .max_by_key(|a| a.offset);

            // Variable whose entire lifetime is within [start, end).
            let entirely_inside = first_write.is_some_and(|w| w.offset >= start && w.offset < end)
                && last_read.is_none_or(|r| r.offset < end)
                && !has_write_before
                && !has_read_after;

            if entirely_inside {
                result.locals.push(var_name.clone());
            } else if has_read_inside && has_write_before && !has_write_inside {
                // Read-only inside the range, written before → parameter.
                result.parameters.push(var_name.clone());
            } else if has_read_inside && has_write_before && has_write_inside {
                // Both read and written inside, also written before →
                // parameter (the initial value matters).
                result.parameters.push(var_name.clone());
                if has_read_after {
                    result.return_values.push(var_name.clone());
                }
            } else if has_write_inside && has_read_after {
                // Written inside, read after → return value.  It is ALSO a
                // parameter whenever the extracted code reads the variable's
                // *incoming* value.  That happens when it was written before
                // the range, or when a read inside the range occurs before
                // the first pure write inside it (a read that consumes the
                // value passed in).  A `+=`-style read-write also reads the
                // incoming value, so a read-write that precedes any pure
                // write counts too.
                let first_pure_write_inside = frame_accesses
                    .iter()
                    .filter(|a| {
                        a.offset >= start && a.offset < end && matches!(a.kind, AccessKind::Write)
                    })
                    .map(|a| a.offset)
                    .min();
                let reads_incoming_value = frame_accesses.iter().any(|a| {
                    a.offset >= start
                        && a.offset < end
                        && matches!(a.kind, AccessKind::Read | AccessKind::ReadWrite)
                        && first_pure_write_inside.is_none_or(|w| a.offset < w)
                });
                let needs_param = has_write_before || reads_incoming_value;
                if needs_param && !result.parameters.contains(var_name) {
                    result.parameters.push(var_name.clone());
                }
                if !result.return_values.contains(var_name) {
                    result.return_values.push(var_name.clone());
                }
            } else if has_read_inside && !has_write_before && !has_write_inside {
                // Read inside but never written — could be a global or
                // an undefined variable.  Treat as parameter.
                result.parameters.push(var_name.clone());
            } else if has_write_inside && !has_read_inside && !has_read_after {
                // Written inside but never read anywhere — local (dead write).
                result.locals.push(var_name.clone());
            }
        }

        // Sort for deterministic output.
        result.parameters.sort();
        result.return_values.sort();
        result.locals.sort();
        result.reference_writes.sort();

        result
    }

    /// Return all unique variable names accessed within the enclosing
    /// frame of the given offset.
    pub(crate) fn variables_in_scope(&self, offset: u32) -> Vec<String> {
        let frame = match self.enclosing_frame(offset) {
            Some(f) => f,
            None => return Vec::new(),
        };

        let mut names: Vec<String> = Vec::new();
        for access in &self.accesses {
            if access.offset >= frame.start
                && access.offset <= frame.end
                && !names.contains(&access.name)
                && access.name != "$this"
            {
                names.push(access.name.clone());
            }
        }
        names.sort();
        names
    }

    /// Return all offsets where variable `name` is accessed within the
    /// enclosing frame of the given offset.  Useful for document
    /// highlights / find-references within a scope.
    pub(crate) fn all_occurrences(&self, name: &str, offset: u32) -> Vec<(u32, AccessKind)> {
        let frame = match self.enclosing_frame(offset) {
            Some(f) => f,
            None => return Vec::new(),
        };

        self.accesses_in_frame(name, frame)
            .into_iter()
            .map(|a| (a.offset, a.kind))
            .collect()
    }
}
