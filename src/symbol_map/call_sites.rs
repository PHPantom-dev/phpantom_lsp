//! Call sites recorded during the AST walk, stored in shared arenas so a
//! file's sites cost no allocation of their own.

/// A call expression site discovered during the AST walk.
///
/// A view into [`CallSites`], which stores every site of a file in
/// `SymbolMap::call_sites`, sorted by `args_start`.
/// Used by signature help to find the innermost call whose argument
/// list contains the cursor and to compute the active parameter index
/// from precomputed comma offsets.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CallSite<'a> {
    /// Byte offset immediately after the opening `(`.
    /// The cursor must be > `args_start` to be "inside" the call.
    pub args_start: u32,
    /// Byte offset of the closing `)`.
    /// When the parser recovered from an unclosed paren, this is the
    /// span end the parser chose.
    pub args_end: u32,
    /// The call expression in the format `resolve_callable` expects:
    ///   - `"functionName"` for standalone function calls
    ///   - `"$subject->method"` for instance/null-safe method calls
    ///   - `"ClassName::method"` for static method calls
    ///   - `"new ClassName"` for constructor calls
    pub call_expression: &'a str,
    /// Byte offsets of each top-level comma separator inside the
    /// argument list.  Used to compute the active parameter index:
    /// count how many comma offsets are < cursor offset.
    pub comma_offsets: &'a [u32],
    /// Byte offset of each argument expression's start token.
    ///
    /// One entry per argument in source order.  Used by inlay hints
    /// to place parameter-name annotations immediately before each
    /// argument.
    pub arg_offsets: &'a [u32],
    /// Number of arguments passed at the call site.
    ///
    /// Computed from the AST argument list length during extraction.
    /// Unlike `comma_offsets.len() + 1`, this correctly handles empty
    /// argument lists (0) and trailing commas.
    pub arg_count: u32,
    /// Whether any argument uses the `...` spread/unpacking operator.
    ///
    /// When `true`, argument count diagnostics are suppressed because
    /// the actual number of arguments is unknown at static analysis time.
    pub has_unpacking: bool,
    /// Indices (into `arg_offsets`) of arguments that use named syntax
    /// (e.g. `name: $value`).  Inlay hints are suppressed for these
    /// because the parameter name is already visible in source.
    pub named_arg_indices: &'a [u32],
    /// Parameter names (without `$` prefix) for each named argument,
    /// in the same order as `named_arg_indices`.  Used by inlay hints
    /// to determine which parameters are already consumed by named
    /// arguments so that positional arguments map to the correct
    /// remaining parameters.
    pub named_arg_names: NamedArgNames<'a>,
    /// Indices (into `arg_offsets`) of arguments that use the `...`
    /// spread/unpacking operator.  Inlay hints are suppressed for these
    /// because a single spread argument may expand into multiple parameters.
    pub spread_arg_indices: &'a [u32],
}

/// The names of a call site's named arguments, stored back to back.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NamedArgNames<'a> {
    text: &'a str,
    /// End of each name within `text`.
    ends: &'a [u32],
}

impl<'a> NamedArgNames<'a> {
    pub(crate) fn get(&self, index: usize) -> Option<&'a str> {
        let end = *self.ends.get(index)? as usize;
        let start = index.checked_sub(1).map_or(0, |i| self.ends[i] as usize);
        Some(&self.text[start..end])
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &'a str> + Clone + 'a {
        let text = self.text;
        let mut start = 0;
        self.ends.iter().map(move |&end| {
            let name = &text[start..end as usize];
            start = end as usize;
            name
        })
    }
}

/// One argument of a call site, as handed to [`CallSites::push`].
#[derive(Clone, Copy)]
pub(crate) struct CallArg<'s> {
    /// Byte offset of the argument's first token (the `...` of a spread,
    /// the name of a named argument).
    pub offset: u32,
    pub kind: CallArgKind<'s>,
}

#[derive(Clone, Copy)]
pub(crate) enum CallArgKind<'s> {
    Positional,
    Spread,
    Named(&'s str),
}

/// Where one call site's data lives in its [`CallSites`] arenas.
///
/// `nums[nums_start..]` holds, in order, the comma offsets, the argument
/// offsets, the named-argument indices, the spread-argument indices and
/// the end of each named-argument name relative to the names' start.
/// `text[text_start..]` holds the call expression followed by the
/// named-argument names.
#[derive(Debug, Clone, Copy)]
struct CallSiteRecord {
    args_start: u32,
    args_end: u32,
    text_start: u32,
    expression_len: u32,
    nums_start: u32,
    comma_count: u32,
    arg_count: u32,
    named_count: u32,
    spread_count: u32,
}

/// A file's call sites, sorted by `args_start`.
///
/// Every workspace file keeps its symbol map for the whole session, so the
/// per-site strings and offset lists live in two shared arenas rather than
/// in allocations of their own.  Interning the call expression instead
/// would leak: the interner never frees, and the text changes with every
/// edit.
#[derive(Debug, Clone, Default)]
pub(crate) struct CallSites {
    sites: Vec<CallSiteRecord>,
    text: String,
    nums: Vec<u32>,
}

impl CallSites {
    /// Record a call site.  `args` is walked once per list it feeds.
    pub(crate) fn push<'s>(
        &mut self,
        args_start: u32,
        args_end: u32,
        call_expression: &str,
        comma_offsets: impl Iterator<Item = u32>,
        args: impl Iterator<Item = CallArg<'s>> + Clone,
    ) {
        let text_start = self.text.len() as u32;
        self.text.push_str(call_expression);
        let nums_start = self.nums.len();
        self.nums.extend(comma_offsets);
        let comma_count = (self.nums.len() - nums_start) as u32;
        self.nums.extend(args.clone().map(|arg| arg.offset));
        let arg_count = (self.nums.len() - nums_start) as u32 - comma_count;
        let indices_of = |wanted: fn(&CallArgKind<'_>) -> bool| {
            args.clone()
                .enumerate()
                .filter(move |(_, arg)| wanted(&arg.kind))
                .map(|(i, _)| i as u32)
        };
        let before = self.nums.len();
        self.nums
            .extend(indices_of(|kind| matches!(kind, CallArgKind::Named(_))));
        let named_count = (self.nums.len() - before) as u32;
        let before = self.nums.len();
        self.nums
            .extend(indices_of(|kind| matches!(kind, CallArgKind::Spread)));
        let spread_count = (self.nums.len() - before) as u32;
        let names_start = self.text.len();
        for arg in args {
            if let CallArgKind::Named(name) = arg.kind {
                self.text.push_str(name);
                self.nums.push((self.text.len() - names_start) as u32);
            }
        }
        self.sites.push(CallSiteRecord {
            args_start,
            args_end,
            text_start,
            expression_len: call_expression.len() as u32,
            nums_start: nums_start as u32,
            comma_count,
            arg_count,
            named_count,
            spread_count,
        });
    }

    /// Sort by `args_start` and release the arenas' spare capacity, once
    /// extraction has pushed every site.
    pub(crate) fn finish(&mut self) {
        self.sites.sort_by_key(|site| site.args_start);
        self.sites.shrink_to_fit();
        self.text.shrink_to_fit();
        self.nums.shrink_to_fit();
    }

    pub(crate) fn iter(&self) -> CallSiteIter<'_> {
        CallSiteIter {
            owner: self,
            sites: self.sites.iter(),
        }
    }

    fn view(&self, site: &CallSiteRecord) -> CallSite<'_> {
        let text_start = site.text_start as usize;
        let names_start = text_start + site.expression_len as usize;
        let mut rest = &self.nums[site.nums_start as usize..];
        let mut take = |count: u32| {
            let (head, tail) = rest.split_at(count as usize);
            rest = tail;
            head
        };
        let comma_offsets = take(site.comma_count);
        let arg_offsets = take(site.arg_count);
        let named_arg_indices = take(site.named_count);
        let spread_arg_indices = take(site.spread_count);
        let name_ends = take(site.named_count);
        let names_end = names_start + name_ends.last().map_or(0, |&end| end as usize);
        CallSite {
            args_start: site.args_start,
            args_end: site.args_end,
            call_expression: &self.text[text_start..names_start],
            comma_offsets,
            arg_offsets,
            arg_count: site.arg_count,
            has_unpacking: site.spread_count > 0,
            named_arg_indices,
            named_arg_names: NamedArgNames {
                text: &self.text[names_start..names_end],
                ends: name_ends,
            },
            spread_arg_indices,
        }
    }

    /// Heap bytes held, for the memory audit.
    #[cfg(feature = "mem-audit")]
    pub(crate) fn heap_bytes(&self) -> usize {
        self.sites.capacity() * size_of::<CallSiteRecord>()
            + self.text.capacity()
            + self.nums.capacity() * size_of::<u32>()
    }
}

pub(crate) struct CallSiteIter<'a> {
    owner: &'a CallSites,
    sites: std::slice::Iter<'a, CallSiteRecord>,
}

impl<'a> Iterator for CallSiteIter<'a> {
    type Item = CallSite<'a>;

    fn next(&mut self) -> Option<CallSite<'a>> {
        self.sites.next().map(|site| self.owner.view(site))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.sites.size_hint()
    }
}

impl DoubleEndedIterator for CallSiteIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.sites.next_back().map(|site| self.owner.view(site))
    }
}

impl<'a> IntoIterator for &'a CallSites {
    type Item = CallSite<'a>;
    type IntoIter = CallSiteIter<'a>;

    fn into_iter(self) -> CallSiteIter<'a> {
        self.iter()
    }
}
