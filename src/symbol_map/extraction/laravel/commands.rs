//! Artisan command names (`Artisan::call('app:sync')`) and the arguments and
//! options a command reads from its own signature (`$this->option('queue')`).

use super::*;
use crate::virtual_members::laravel::helpers::string_literal_at_range;

/// If the first argument of `argument_list` is a plain, non-empty string
/// literal, push a [`SymbolKind::CommandOwnParam`] span covering the string
/// content.  Used for `$this->argument('user')` / `$this->option('queue')`
/// inside a console command.
pub(in crate::symbol_map::extraction) fn try_emit_command_own_param_span(
    is_option: bool,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some((name, start, end)) = argument_list
        .arguments
        .first()
        .and_then(|first_arg| string_literal_at_range(first_arg.value(), content))
    else {
        return;
    };
    spans.push(SymbolSpan {
        start,
        end,
        kind: SymbolKind::CommandOwnParam {
            name: name.to_string(),
            is_option,
        },
    });
}

/// Returns `true` for a static call whose first argument is an Artisan
/// command name: `Artisan::call(...)`, `Artisan::queue(...)`, or
/// `Schedule::command(...)`.  Recognises both the short facade name and its
/// `Illuminate\Support\Facades\` FQN.
pub(in crate::symbol_map::extraction) fn is_artisan_command_static_call(
    clean_subject: &str,
    member_name: &str,
) -> bool {
    (matches_laravel_facade(clean_subject, "Artisan")
        && (member_name.eq_ignore_ascii_case("call") || member_name.eq_ignore_ascii_case("queue")))
        || (matches_laravel_facade(clean_subject, "Schedule")
            && member_name.eq_ignore_ascii_case("command"))
}
