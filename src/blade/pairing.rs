//! The stack walk that pairs a template's openers with their closers.
//!
//! Blade has two syntaxes with one shape: a block directive (`@if` …
//! `@endif`) and a component tag (`<x-alert>` … `</x-alert>`) both open
//! something the template has to close, and Blade closes whichever is
//! innermost without checking that the names agree. A `@endif` after a
//! `@foreach` ends the loop, and a `</x-card>` after an `<x-alert>` ends
//! the alert. The directive check and the component-tag scan therefore
//! have to make the same decisions about a closer that does not match,
//! and they make them here, once.
//!
//! The walk is generic over what an opener and a closer are. Callers
//! produce the token stream in document order and say when a closer
//! closes an opener; the walk hands back the pairs, the openers that were
//! ended without a pair, the ones still open at the end, and the closers
//! that closed the wrong thing or nothing at all.

/// One opener or closer, in document order.
pub(crate) enum Token<O, C> {
    Open(O),
    Close(C),
}

/// An opener and the closer that ended it.
pub(crate) struct Pair<O, C> {
    pub(crate) opener: O,
    pub(crate) closer: C,
}

/// A closer that did not end the block it sits in.
pub(crate) enum Stray<O, C> {
    /// The closer belongs to a block further out, or to no open block at
    /// all while one is open. `skipped` is the block still open when the
    /// closer arrived: the one opened directly inside the block it closes,
    /// or the innermost open block when it closes none. It is ended without
    /// a pair and is also in [`Pairing::consumed`].
    Mismatched { closer: C, skipped: O },
    /// A closer with nothing open to close.
    Unexpected { closer: C },
}

/// What one walk of a token stream finds.
pub(crate) struct Pairing<O, C> {
    /// Every well-nested pair, in the order the closers were met.
    pub(crate) pairs: Vec<Pair<O, C>>,
    /// Openers a mismatched closer ended without a pair: the block it
    /// skipped past, everything open inside that block, and the block it
    /// did belong to. None of these can be closed by anything later.
    pub(crate) consumed: Vec<O>,
    /// Openers still open when the stream ended, outermost first.
    pub(crate) open: Vec<O>,
    /// Every closer that closed the wrong thing or nothing.
    pub(crate) strays: Vec<Stray<O, C>>,
}

/// Walk `tokens` once with a stack of open blocks.
///
/// `closes` says whether a closer is one that ends a given opener. A
/// closer that matches the innermost open block pairs with it. One that
/// matches a block further out ends that block *and* everything opened
/// inside it, none of which pairs: the closer is not the inner block's,
/// and the block it does close has an unclosed block inside it. One that
/// matches no open block still ends the innermost one, because that is
/// what Blade compiles it to. The block left open directly inside the
/// one the closer ended is what gets reported, as the thing the author
/// forgot to close before it.
pub(crate) fn pair<O: Clone, C>(
    tokens: impl IntoIterator<Item = Token<O, C>>,
    closes: impl Fn(&O, &C) -> bool,
) -> Pairing<O, C> {
    let mut pairing = Pairing {
        pairs: Vec::new(),
        consumed: Vec::new(),
        open: Vec::new(),
        strays: Vec::new(),
    };
    let mut stack: Vec<O> = Vec::new();

    for token in tokens {
        let closer = match token {
            Token::Open(opener) => {
                stack.push(opener);
                continue;
            }
            Token::Close(closer) => closer,
        };
        match stack.iter().rposition(|opener| closes(opener, &closer)) {
            Some(index) if index + 1 == stack.len() => {
                if let Some(opener) = stack.pop() {
                    pairing.pairs.push(Pair { opener, closer });
                }
            }
            Some(index) => {
                let skipped = stack[index + 1].clone();
                pairing.consumed.extend(stack.drain(index..));
                pairing.strays.push(Stray::Mismatched { closer, skipped });
            }
            None => match stack.pop() {
                Some(opener) => {
                    pairing.strays.push(Stray::Mismatched {
                        closer,
                        skipped: opener.clone(),
                    });
                    pairing.consumed.push(opener);
                }
                None => pairing.strays.push(Stray::Unexpected { closer }),
            },
        }
    }

    pairing.open = stack;
    pairing
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pair a stream written as `+name` (open) and `-name` (close) words,
    /// where a closer closes the opener of the same name, and describe the
    /// result as short strings.
    fn walk(stream: &str) -> Vec<String> {
        let tokens = stream.split_whitespace().map(|word| {
            let name = word[1..].to_string();
            if word.starts_with('+') {
                Token::Open(name)
            } else {
                Token::Close(name)
            }
        });
        let pairing = pair(tokens, |open: &String, close: &String| open == close);
        let mut out: Vec<String> = pairing
            .pairs
            .iter()
            .map(|pair| format!("pair {}", pair.opener))
            .collect();
        out.extend(pairing.consumed.iter().map(|o| format!("consumed {o}")));
        out.extend(pairing.open.iter().map(|o| format!("open {o}")));
        out.extend(pairing.strays.iter().map(|stray| match stray {
            Stray::Mismatched { closer, skipped } => format!("mismatched {closer}/{skipped}"),
            Stray::Unexpected { closer } => format!("unexpected {closer}"),
        }));
        out
    }

    #[test]
    fn nested_blocks_pair_innermost_first() {
        assert_eq!(walk("+a +b -b -a"), ["pair b", "pair a"]);
    }

    #[test]
    fn a_block_never_closed_stays_open() {
        assert_eq!(walk("+a +b -b"), ["pair b", "open a"]);
    }

    #[test]
    fn a_closer_with_nothing_open_is_unexpected() {
        assert_eq!(walk("-a"), ["unexpected a"]);
    }

    /// A closer matching no open block still ends the innermost one, and
    /// that block is what the report names.
    #[test]
    fn a_closer_for_nothing_open_ends_the_innermost_block() {
        assert_eq!(walk("+a -b"), ["consumed a", "mismatched b/a"]);
        // The block is gone for pairing too: its own closer now closes
        // nothing.
        assert_eq!(
            walk("+a -b -a"),
            ["consumed a", "mismatched b/a", "unexpected a"]
        );
    }

    /// A closer for a block further out ends that block and everything
    /// inside it, pairing none of them, and reports the block left open
    /// directly inside the one it closed.
    #[test]
    fn a_closer_for_an_outer_block_consumes_the_run() {
        assert_eq!(
            walk("+a +b +c -a"),
            ["consumed a", "consumed b", "consumed c", "mismatched a/b"]
        );
    }

    #[test]
    fn blocks_outside_a_mismatch_still_pair() {
        assert_eq!(
            walk("+x +a +b -a -x"),
            ["pair x", "consumed a", "consumed b", "mismatched a/b"]
        );
    }
}
