//! Human-readable formatting for Mago parse errors.
//!
//! The upstream `mago-syntax` crate derives `strum::Display` for
//! `TokenKind`, which outputs the Rust variant name (e.g. `Minus`,
//! `LeftBrace`, `Variable`).  That is fine for compiler internals but
//! confusing for end-users who see messages like:
//!
//! > Expected one of `Variable`, found `Minus`
//!
//! This module translates those internal names to the actual PHP tokens
//! so the diagnostic reads:
//!
//! > Syntax error: unexpected `-`, expected a variable
//!
//! The public entry-point is [`format_parse_error`].

use mago_syntax::error::{ParseError, SyntaxError};
use mago_syntax::token::TokenKind;

/// Format a [`ParseError`] as a human-readable diagnostic message.
pub(crate) fn format_parse_error(error: &ParseError) -> String {
    match error {
        ParseError::SyntaxError(e) => format_syntax_error(e),
        ParseError::UnexpectedEndOfFile(expected, _, _) => {
            let expected = friendly_token_list(expected);
            if expected.is_empty() {
                "Syntax error: unexpected end of file".to_string()
            } else {
                format!("Syntax error: unexpected end of file, expected {expected}")
            }
        }
        ParseError::UnexpectedToken(expected, found, _) => {
            let found = friendly_token_name(found);
            let expected = friendly_token_list(expected);
            if expected.is_empty() {
                format!("Syntax error: unexpected token {found}")
            } else {
                format!("Syntax error: unexpected token {found}, expected {expected}")
            }
        }
        ParseError::UnclosedLiteralString(kind, _) => {
            use mago_syntax::ast::LiteralStringKind;
            match kind {
                LiteralStringKind::SingleQuoted => {
                    "Syntax error: unclosed single-quoted string".to_string()
                }
                LiteralStringKind::DoubleQuoted => {
                    "Syntax error: unclosed double-quoted string".to_string()
                }
            }
        }
        ParseError::RecursionLimitExceeded(_) => {
            "Syntax error: maximum nesting depth exceeded".to_string()
        }
    }
}

fn format_syntax_error(error: &SyntaxError) -> String {
    match error {
        SyntaxError::UnexpectedToken(_, byte, _) => {
            let ch = *byte as char;
            if ch.is_ascii_graphic() {
                format!("Syntax error: unexpected character `{ch}`")
            } else {
                format!("Syntax error: unexpected byte 0x{byte:02X}")
            }
        }
        SyntaxError::UnrecognizedToken(_, byte, _) => {
            let ch = *byte as char;
            if ch.is_ascii_graphic() {
                format!("Syntax error: unrecognized character `{ch}`")
            } else {
                format!("Syntax error: unrecognized byte 0x{byte:02X}")
            }
        }
        SyntaxError::UnexpectedEndOfFile(_, _) => {
            "Syntax error: unexpected end of file".to_string()
        }
    }
}

/// Build a human-friendly comma-separated list of expected tokens.
///
/// For a single item: `\`function\``
/// For two items: `\`function\` or \`class\``
/// For three+: `\`function\`, \`class\`, or \`interface\``
fn friendly_token_list(kinds: &[TokenKind]) -> String {
    let names: Vec<String> = kinds.iter().map(friendly_token_name).collect();
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        2 => format!("{} or {}", names[0], names[1]),
        _ => {
            let (last, rest) = names.split_last().unwrap();
            format!("{}, or {last}", rest.join(", "))
        }
    }
}

/// Map a single [`TokenKind`] to a human-readable representation.
///
/// Keywords and identifiers keep their PHP spelling; punctuation tokens
/// are shown as quoted symbols (e.g. `` `-` ``).
fn friendly_token_name(kind: &TokenKind) -> String {
    let s = match kind {
        // ── Identifiers & variables ──────────────────────────────
        TokenKind::Identifier => "an identifier",
        TokenKind::QualifiedIdentifier => "a qualified name",
        TokenKind::FullyQualifiedIdentifier => "a fully-qualified name",
        TokenKind::Variable => "a variable",
        TokenKind::Dollar => "`$`",

        // ── Literals ─────────────────────────────────────────────
        TokenKind::LiteralInteger => "an integer",
        TokenKind::LiteralFloat => "a number",
        TokenKind::LiteralString => "a string",
        TokenKind::PartialLiteralString => "a string",
        TokenKind::StringPart => "a string part",

        // ── Keywords ─────────────────────────────────────────────
        TokenKind::Abstract => "`abstract`",
        TokenKind::And => "`and`",
        TokenKind::Array => "`array`",
        TokenKind::As => "`as`",
        TokenKind::Break => "`break`",
        TokenKind::Callable => "`callable`",
        TokenKind::Case => "`case`",
        TokenKind::Catch => "`catch`",
        TokenKind::Class => "`class`",
        TokenKind::Clone => "`clone`",
        TokenKind::Const => "`const`",
        TokenKind::Continue => "`continue`",
        TokenKind::Declare => "`declare`",
        TokenKind::Default => "`default`",
        TokenKind::Do => "`do`",
        TokenKind::Echo => "`echo`",
        TokenKind::Else => "`else`",
        TokenKind::ElseIf => "`elseif`",
        TokenKind::Empty => "`empty`",
        TokenKind::EndDeclare => "`enddeclare`",
        TokenKind::EndFor => "`endfor`",
        TokenKind::EndForeach => "`endforeach`",
        TokenKind::EndIf => "`endif`",
        TokenKind::EndSwitch => "`endswitch`",
        TokenKind::EndWhile => "`endwhile`",
        TokenKind::Enum => "`enum`",
        TokenKind::Eval => "`eval`",
        TokenKind::Exit => "`exit`",
        TokenKind::Die => "`die`",
        TokenKind::Extends => "`extends`",
        TokenKind::False => "`false`",
        TokenKind::Final => "`final`",
        TokenKind::Finally => "`finally`",
        TokenKind::Fn => "`fn`",
        TokenKind::For => "`for`",
        TokenKind::Foreach => "`foreach`",
        TokenKind::From => "`from`",
        TokenKind::Function => "`function`",
        TokenKind::Global => "`global`",
        TokenKind::Goto => "`goto`",
        TokenKind::HaltCompiler => "`__halt_compiler`",
        TokenKind::If => "`if`",
        TokenKind::Implements => "`implements`",
        TokenKind::Include => "`include`",
        TokenKind::IncludeOnce => "`include_once`",
        TokenKind::Instanceof => "`instanceof`",
        TokenKind::Insteadof => "`insteadof`",
        TokenKind::Interface => "`interface`",
        TokenKind::Isset => "`isset`",
        TokenKind::List => "`list`",
        TokenKind::Match => "`match`",
        TokenKind::Namespace => "`namespace`",
        TokenKind::New => "`new`",
        TokenKind::Null => "`null`",
        TokenKind::Or => "`or`",
        TokenKind::Print => "`print`",
        TokenKind::Private => "`private`",
        TokenKind::PrivateSet => "`private(set)`",
        TokenKind::Protected => "`protected`",
        TokenKind::ProtectedSet => "`protected(set)`",
        TokenKind::Public => "`public`",
        TokenKind::PublicSet => "`public(set)`",
        TokenKind::Readonly => "`readonly`",
        TokenKind::Require => "`require`",
        TokenKind::RequireOnce => "`require_once`",
        TokenKind::Return => "`return`",
        TokenKind::Self_ => "`self`",
        TokenKind::Parent => "`parent`",
        TokenKind::Static => "`static`",
        TokenKind::Switch => "`switch`",
        TokenKind::Throw => "`throw`",
        TokenKind::Trait => "`trait`",
        TokenKind::True => "`true`",
        TokenKind::Try => "`try`",
        TokenKind::Unset => "`unset`",
        TokenKind::Use => "`use`",
        TokenKind::Var => "`var`",
        TokenKind::While => "`while`",
        TokenKind::Xor => "`xor`",
        TokenKind::Yield => "`yield`",

        // ── Magic constants ──────────────────────────────────────
        TokenKind::ClassConstant => "`__CLASS__`",
        TokenKind::TraitConstant => "`__TRAIT__`",
        TokenKind::FunctionConstant => "`__FUNCTION__`",
        TokenKind::MethodConstant => "`__METHOD__`",
        TokenKind::LineConstant => "`__LINE__`",
        TokenKind::FileConstant => "`__FILE__`",
        TokenKind::DirConstant => "`__DIR__`",
        TokenKind::NamespaceConstant => "`__NAMESPACE__`",
        TokenKind::PropertyConstant => "`__PROPERTY__`",

        // ── Casts ────────────────────────────────────────────────
        TokenKind::ArrayCast => "`(array)`",
        TokenKind::BoolCast => "`(bool)`",
        TokenKind::BooleanCast => "`(boolean)`",
        TokenKind::DoubleCast => "`(double)`",
        TokenKind::RealCast => "`(real)`",
        TokenKind::FloatCast => "`(float)`",
        TokenKind::IntCast => "`(int)`",
        TokenKind::IntegerCast => "`(integer)`",
        TokenKind::ObjectCast => "`(object)`",
        TokenKind::StringCast => "`(string)`",
        TokenKind::BinaryCast => "`(binary)`",
        TokenKind::UnsetCast => "`(unset)`",
        TokenKind::VoidCast => "`(void)`",

        // ── Single-character punctuation ─────────────────────────
        TokenKind::Ampersand => "`&`",
        TokenKind::At => "`@`",
        TokenKind::Asterisk => "`*`",
        TokenKind::Backtick => "`` ` ``",
        TokenKind::Bang => "`!`",
        TokenKind::Caret => "`^`",
        TokenKind::Colon => "`:`",
        TokenKind::Comma => "`,`",
        TokenKind::Dot => "`.`",
        TokenKind::DoubleQuote => "`\"`",
        TokenKind::Equal => "`=`",
        TokenKind::GreaterThan => "`>`",
        TokenKind::LeftBrace => "`{`",
        TokenKind::LeftBracket => "`[`",
        TokenKind::LeftParenthesis => "`(`",
        TokenKind::LessThan => "`<`",
        TokenKind::Minus => "`-`",
        TokenKind::Percent => "`%`",
        TokenKind::Pipe => "`|`",
        TokenKind::Plus => "`+`",
        TokenKind::Question => "`?`",
        TokenKind::RightBrace => "`}`",
        TokenKind::RightBracket => "`]`",
        TokenKind::RightParenthesis => "`)`",
        TokenKind::Semicolon => "`;`",
        TokenKind::Slash => "`/`",
        TokenKind::Tilde => "`~`",
        TokenKind::NamespaceSeparator => "`\\`",

        // ── Multi-character operators ────────────────────────────
        TokenKind::AmpersandEqual => "`&=`",
        TokenKind::AmpersandAmpersand => "`&&`",
        TokenKind::AmpersandAmpersandEqual => "`&&=`",
        TokenKind::AsteriskEqual => "`*=`",
        TokenKind::AsteriskAsterisk => "`**`",
        TokenKind::AsteriskAsteriskEqual => "`**=`",
        TokenKind::BangEqual => "`!=`",
        TokenKind::BangEqualEqual => "`!==`",
        TokenKind::CaretEqual => "`^=`",
        TokenKind::ColonColon => "`::`",
        TokenKind::DotEqual => "`.=`",
        TokenKind::DotDotDot => "`...`",
        TokenKind::EqualEqual => "`==`",
        TokenKind::EqualEqualEqual => "`===`",
        TokenKind::EqualGreaterThan => "`=>`",
        TokenKind::GreaterThanEqual => "`>=`",
        TokenKind::HashLeftBracket => "`#[`",
        TokenKind::LeftShift => "`<<`",
        TokenKind::LeftShiftEqual => "`<<=`",
        TokenKind::LessThanEqual => "`<=`",
        TokenKind::LessThanGreaterThan => "`<>`",
        TokenKind::LessThanEqualGreaterThan => "`<=>`",
        TokenKind::MinusEqual => "`-=`",
        TokenKind::MinusMinus => "`--`",
        TokenKind::MinusGreaterThan => "`->`",
        TokenKind::PercentEqual => "`%=`",
        TokenKind::PipeEqual => "`|=`",
        TokenKind::PipePipe => "`||`",
        TokenKind::PipeGreaterThan => "`|>`",
        TokenKind::PlusEqual => "`+=`",
        TokenKind::PlusPlus => "`++`",
        TokenKind::QuestionQuestion => "`??`",
        TokenKind::QuestionQuestionEqual => "`??=`",
        TokenKind::QuestionMinusGreaterThan => "`?->`",
        TokenKind::RightShift => "`>>`",
        TokenKind::RightShiftEqual => "`>>=`",
        TokenKind::SlashEqual => "`/=`",

        // ── PHP tags ─────────────────────────────────────────────
        TokenKind::OpenTag => "`<?php`",
        TokenKind::EchoTag => "`<?=`",
        TokenKind::ShortOpenTag => "`<?`",
        TokenKind::CloseTag => "`?>`",

        // ── Special / structural ─────────────────────────────────
        TokenKind::DollarLeftBrace => "`${`",
        TokenKind::DocumentStart(_) => "a heredoc/nowdoc start",
        TokenKind::DocumentEnd => "a heredoc/nowdoc end",
        TokenKind::InlineText => "inline HTML",
        TokenKind::InlineShebang => "a shebang line",
        TokenKind::Whitespace => "whitespace",
        TokenKind::SingleLineComment => "a comment",
        TokenKind::HashComment => "a comment",
        TokenKind::MultiLineComment => "a comment",
        TokenKind::DocBlockComment => "a doc comment",
    };
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mago_database::file::FileId;
    use mago_span::{Position, Span};

    fn pos(offset: u32) -> Position {
        Position::new(offset)
    }

    fn span(start: u32, end: u32) -> Span {
        Span::new(FileId::new(b"test.php"), pos(start), pos(end))
    }

    #[test]
    fn unexpected_minus_instead_of_variable() {
        let err = ParseError::UnexpectedToken(
            Box::new([TokenKind::Variable]),
            TokenKind::Minus,
            span(10, 11),
        );
        let msg = format_parse_error(&err);
        assert_eq!(
            msg,
            "Syntax error: unexpected token `-`, expected a variable"
        );
    }

    #[test]
    fn unexpected_token_with_multiple_expected() {
        let err = ParseError::UnexpectedToken(
            Box::new([TokenKind::Function, TokenKind::Class, TokenKind::Interface]),
            TokenKind::Minus,
            span(10, 11),
        );
        let msg = format_parse_error(&err);
        assert_eq!(
            msg,
            "Syntax error: unexpected token `-`, expected `function`, `class`, or `interface`"
        );
    }

    #[test]
    fn unexpected_token_with_two_expected() {
        let err = ParseError::UnexpectedToken(
            Box::new([TokenKind::Semicolon, TokenKind::RightBrace]),
            TokenKind::LeftParenthesis,
            span(5, 6),
        );
        let msg = format_parse_error(&err);
        assert_eq!(
            msg,
            "Syntax error: unexpected token `(`, expected `;` or `}`"
        );
    }

    #[test]
    fn unexpected_token_no_expected() {
        let err = ParseError::UnexpectedToken(Box::new([]), TokenKind::At, span(0, 1));
        let msg = format_parse_error(&err);
        assert_eq!(msg, "Syntax error: unexpected token `@`");
    }

    #[test]
    fn unexpected_eof_with_expected() {
        let file_id = FileId::new(b"test.php");
        let err =
            ParseError::UnexpectedEndOfFile(Box::new([TokenKind::Semicolon]), file_id, pos(100));
        let msg = format_parse_error(&err);
        assert_eq!(msg, "Syntax error: unexpected end of file, expected `;`");
    }

    #[test]
    fn unexpected_eof_no_expected() {
        let file_id = FileId::new(b"test.php");
        let err = ParseError::UnexpectedEndOfFile(Box::new([]), file_id, pos(50));
        let msg = format_parse_error(&err);
        assert_eq!(msg, "Syntax error: unexpected end of file");
    }

    #[test]
    fn unclosed_single_quoted_string() {
        use mago_syntax::ast::LiteralStringKind;
        let err = ParseError::UnclosedLiteralString(LiteralStringKind::SingleQuoted, span(5, 20));
        let msg = format_parse_error(&err);
        assert_eq!(msg, "Syntax error: unclosed single-quoted string");
    }

    #[test]
    fn unclosed_double_quoted_string() {
        use mago_syntax::ast::LiteralStringKind;
        let err = ParseError::UnclosedLiteralString(LiteralStringKind::DoubleQuoted, span(5, 20));
        let msg = format_parse_error(&err);
        assert_eq!(msg, "Syntax error: unclosed double-quoted string");
    }

    #[test]
    fn recursion_limit() {
        let err = ParseError::RecursionLimitExceeded(span(0, 100));
        let msg = format_parse_error(&err);
        assert_eq!(msg, "Syntax error: maximum nesting depth exceeded");
    }

    #[test]
    fn syntax_error_unexpected_char() {
        let file_id = FileId::new(b"test.php");
        let err = ParseError::SyntaxError(SyntaxError::UnexpectedToken(file_id, b'#', pos(5)));
        let msg = format_parse_error(&err);
        assert_eq!(msg, "Syntax error: unexpected character `#`");
    }

    #[test]
    fn syntax_error_unrecognized_char() {
        let file_id = FileId::new(b"test.php");
        let err = ParseError::SyntaxError(SyntaxError::UnrecognizedToken(file_id, b'@', pos(10)));
        let msg = format_parse_error(&err);
        assert_eq!(msg, "Syntax error: unrecognized character `@`");
    }

    #[test]
    fn syntax_error_eof() {
        let file_id = FileId::new(b"test.php");
        let err = ParseError::SyntaxError(SyntaxError::UnexpectedEndOfFile(file_id, pos(50)));
        let msg = format_parse_error(&err);
        assert_eq!(msg, "Syntax error: unexpected end of file");
    }

    #[test]
    fn non_graphic_byte() {
        let file_id = FileId::new(b"test.php");
        let err = ParseError::SyntaxError(SyntaxError::UnexpectedToken(file_id, 0x01, pos(0)));
        let msg = format_parse_error(&err);
        assert_eq!(msg, "Syntax error: unexpected byte 0x01");
    }
}
