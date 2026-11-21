//! What a game does with the strings its events set: WOLF RPG Editor has
//! one command for every string a game handles, so a set string or a string
//! handed to a common event may be a line the player reads, a file's name,
//! or a word the events compare. A string is display text only when the
//! variable it goes into is shown (written into a message, a choice or a
//! picture's text through `\cself[n]` or `\s[n]`) and is never used as
//! anything else, here or in any variable it is copied or handed on to.
//! When the game leaves that open, the string is left alone.
//!
//! The name of a database's row is such a string too when an event reads it
//! into a variable that is only shown: a game's own message window shows a
//! speaker's name that way.

use std::collections::{HashMap, HashSet};

use super::{
    commands::{
        CALL_EVENT, CALL_EVENT_BY_NAME, COMMENT, Command, DATABASE, DATABASE_FILE, DEBUG_MESSAGE,
        JUMP_TO_LABEL, LABEL, PICTURE, SET_STRING, SHOW_CHOICES, SHOW_MESSAGE, STRING_CONDITION,
    },
    common_events::CommonEvents,
    map::Map,
    text::Encoding,
};

const COMMON_EVENT_VARIABLES: u32 = 1_600_000;
const STRING_VARIABLES: u32 = 3_000_000;
const STRING_VARIABLES_END: u32 = 4_000_000;
/// A common event's own variables 5 to 9 hold strings, and its string
/// arguments arrive in them.
const FIRST_OWN_STRING: u32 = 5;
const LAST_OWN_STRING: u32 = 9;
const COMMON_EVENTS: u32 = 500_000;
const COMMON_EVENTS_END: u32 = 600_000;
/// A command's number below this is the number itself, not a variable's.
const FIRST_VARIABLE: u32 = 1_000_000;
/// A database command's fourth number: what it does, and with which of
/// the three databases.
const DATABASE_READS: u32 = 0x1000;
const DATABASE_OPERATION: u32 = 0xff;
const DATABASE_BASE: u32 = 0xf00;
const TYPE_BY_NAME: u32 = 0x1_0000;
const ROW_BY_NAME: u32 = 0x2_0000;
const FIELD_BY_NAME: u32 = 0x4_0000;
/// In a field's place: the row's name.
const ROW_NAME: u32 = 0xffff_fffd;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Variable {
    /// A string variable of one common event.
    Own { event: u32, number: u32 },
    /// One of the game's string variables.
    Shared(u32),
}

/// One of a game's three databases.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Base {
    Changeable,
    System,
    User,
}

/// The type a database command works on.
#[derive(Clone, Debug, Eq, PartialEq)]
enum TypeOf {
    Number(usize),
    Name(String),
    /// Picked by a variable: any of them.
    Unknown,
}

/// What a database command says of a database's rows by their names.
struct DatabaseCommand {
    base: Base,
    of: TypeOf,
    /// The row it finds by this name.
    looked_up: Option<String>,
    /// The string variable it puts a row's name into, and whether it puts
    /// it there as it is, replacing what the variable held.
    name_into: Option<(Variable, bool)>,
}

impl DatabaseCommand {
    fn of(scope: Scope, command: &Command, strings: &[String]) -> Option<Self> {
        let &[type_number, _, field, what, target, ..] = command.args.as_slice() else {
            return None;
        };
        let base = match (what & DATABASE_BASE) >> 8 {
            0 => Base::Changeable,
            1 => Base::System,
            2 => Base::User,
            _ => return None,
        };
        let named = |flag: u32, string: usize| {
            strings
                .get(string)
                .filter(|name| what & flag != 0 && !name.is_empty())
        };
        let of = match named(TYPE_BY_NAME, 1) {
            Some(name) => TypeOf::Name(name.clone()),
            None if what & TYPE_BY_NAME == 0 && type_number < FIRST_VARIABLE => {
                TypeOf::Number(type_number as usize)
            }
            None => TypeOf::Unknown,
        };
        let reads_name =
            field == ROW_NAME && what & (DATABASE_READS | FIELD_BY_NAME) == DATABASE_READS;
        Some(Self {
            base,
            looked_up: named(ROW_BY_NAME, 2).cloned(),
            name_into: variable(scope, target)
                .filter(|_| reads_name)
                .map(|target| (target, what & DATABASE_OPERATION == 0)),
            of,
        })
    }

    fn concerns(&self, base: Base, number: usize, name: &str) -> bool {
        self.base == base
            && match &self.of {
                TypeOf::Number(of) => *of == number,
                TypeOf::Name(of) => of == name,
                TypeOf::Unknown => true,
            }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scope {
    CommonEvent(u32),
    MapEvent,
}

/// A string argument of a command that goes into a variable as it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Literal {
    /// Which of the command's strings.
    pub string: usize,
    pub target: Variable,
    /// Added to what the variable holds, not replacing it.
    pub appended: bool,
}

#[derive(Default)]
pub struct Usage {
    events_by_name: HashMap<Vec<u8>, u32>,
    shown: HashSet<Variable>,
    other_use: HashSet<Variable>,
    flows: HashMap<Variable, Vec<Variable>>,
    compared: HashMap<Variable, Vec<String>>,
    takes_a_file: HashSet<Variable>,
    appended_to: HashSet<Variable>,
    /// Strings the events use as words of their own: compared, or looked
    /// up in a database. A database value equal to one stays as it is.
    pub kept: HashSet<String>,
    /// What the databases call their types, fields and hand-named rows.
    pub database_words: HashSet<String>,
    database_commands: Vec<DatabaseCommand>,
    /// The names of the game's files, without folder and extension, and
    /// the words in them.
    file_words: HashSet<String>,
    /// Whether the game puts file names together from strings
    /// (`Picture/\cself[5].png`, a folder with a name added to it). In such
    /// a game a database value that is a file's name, or a word of one,
    /// may be what a name is built from.
    pub builds_file_names: bool,
}

impl Usage {
    pub fn of(common_events: &CommonEvents, maps: &[(String, Map)]) -> Self {
        let mut usage = Self::default();
        // The maps are in the same encoding; a game that mixes two is
        // refused before it gets here.
        let encoding = common_events.encoding;
        for event in &common_events.events {
            usage
                .events_by_name
                .entry(event.name.clone())
                .or_insert(event.id);
        }
        let lists = || {
            common_events
                .events
                .iter()
                .map(|event| (Scope::CommonEvent(event.id), &event.commands))
                .chain(maps.iter().flat_map(|(_, map)| {
                    map.events.iter().flat_map(|event| {
                        event
                            .pages
                            .iter()
                            .map(|page| (Scope::MapEvent, &page.commands))
                    })
                }))
        };
        for (scope, commands) in lists() {
            for command in commands {
                usage.read(encoding, scope, command);
            }
        }
        // A variable that also takes a file's name is not text.
        usage.builds_file_names |= !usage.takes_a_file.is_disjoint(&usage.appended_to);
        let files = usage.takes_a_file.iter().copied().collect::<Vec<_>>();
        usage.other_use.extend(files);
        let mut kept = Vec::new();
        for (scope, commands) in lists() {
            for command in commands {
                for literal in usage.literals(scope, command) {
                    if let Some(text) = text_of(encoding, command, literal.string)
                        && !usage.is_shown(literal.target, &text)
                    {
                        kept.push(text);
                    }
                }
            }
        }
        usage.kept.extend(kept);
        usage
    }

    /// The command's strings that go into a variable as they are: a set
    /// string's, and the string arguments of a call to a common event.
    pub fn literals(&self, scope: Scope, command: &Command) -> Vec<Literal> {
        match command.id {
            SET_STRING => {
                let Some(set) = SetString::of(scope, command) else {
                    return Vec::new();
                };
                match (set.target, set.source, set.operation) {
                    (Some(target), Source::Literal, Operation::Assign | Operation::Append)
                        if command.strings.len() == 1 =>
                    {
                        vec![Literal {
                            string: 0,
                            target,
                            appended: set.operation == Operation::Append,
                        }]
                    }
                    _ => Vec::new(),
                }
            }
            CALL_EVENT | CALL_EVENT_BY_NAME => {
                let Some(call) = Call::of(command) else {
                    return Vec::new();
                };
                let Some(event) = self.called(command) else {
                    return Vec::new();
                };
                (0..call.strings)
                    .filter(|&at| call.is_literal(at) && at + 1 < command.strings.len())
                    .map(|at| Literal {
                        string: at + 1,
                        target: Variable::Own {
                            event,
                            number: FIRST_OWN_STRING + at as u32,
                        },
                        appended: false,
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// The common event a call command runs, when the command names it.
    pub fn called(&self, command: &Command) -> Option<u32> {
        match command.id {
            CALL_EVENT => command
                .args
                .first()
                .filter(|&&event| (COMMON_EVENTS..COMMON_EVENTS_END).contains(&event))
                .map(|event| event - COMMON_EVENTS),
            CALL_EVENT_BY_NAME => self.events_by_name.get(command.strings.first()?).copied(),
            _ => None,
        }
    }

    /// Whether a database value is a word the game uses as its own.
    pub fn keeps(&self, value: &str) -> bool {
        self.kept.contains(value)
            || self.database_words.contains(value)
            || (self.builds_file_names && self.file_words.contains(value))
    }

    /// Whether an event reads the names of this type's rows into a
    /// variable that is only ever shown.
    pub fn reads_row_names(&self, base: Base, number: usize, type_name: &str) -> bool {
        self.shows_row_name(base, number, type_name, "")
    }

    /// Whether the name of one of the type's rows is only ever shown: an
    /// event reads the type's names to show them, none reads them for
    /// anything else, and none finds a row by this name.
    pub fn shows_row_name(&self, base: Base, number: usize, type_name: &str, name: &str) -> bool {
        let mut shown = false;
        for command in &self.database_commands {
            if !command.concerns(base, number, type_name) {
                continue;
            }
            if command.looked_up.as_deref() == Some(name) {
                return false;
            }
            match (command.name_into, &command.of) {
                (None, _) => {}
                // A tool for any type (a debug window) speaks against a
                // name only when the events look for it in what it read.
                (Some((target, _)), TypeOf::Unknown) | (Some((target, false)), _) => {
                    if self.is_compared(target, name) {
                        return false;
                    }
                }
                (Some((target, true)), _) => {
                    if !self.is_shown(target, name) {
                        return false;
                    }
                    shown = true;
                }
            }
        }
        shown
    }

    /// Whether `text`, put into `variable`, is only ever shown.
    pub fn is_shown(&self, variable: Variable, text: &str) -> bool {
        let mut seen = HashSet::from([variable]);
        let mut pending = vec![variable];
        let mut shown = false;
        while let Some(variable) = pending.pop() {
            if self.other_use.contains(&variable) {
                return false;
            }
            // The events look for these words in what the variable holds.
            if self
                .compared
                .get(&variable)
                .is_some_and(|words| words.iter().any(|word| text.contains(word.as_str())))
            {
                return false;
            }
            shown |= self.shown.contains(&variable);
            for &next in self.flows.get(&variable).into_iter().flatten() {
                if seen.insert(next) {
                    pending.push(next);
                }
            }
        }
        shown
    }

    /// Whether the events look for a word of `text` in what `variable`
    /// holds, or in a variable it is copied or handed on to.
    fn is_compared(&self, variable: Variable, text: &str) -> bool {
        let mut seen = HashSet::from([variable]);
        let mut pending = vec![variable];
        while let Some(variable) = pending.pop() {
            if self.compared.get(&variable).is_some_and(|words| {
                words
                    .iter()
                    .any(|word| !text.is_empty() && text.contains(word.as_str()))
            }) {
                return true;
            }
            for &next in self.flows.get(&variable).into_iter().flatten() {
                if seen.insert(next) {
                    pending.push(next);
                }
            }
        }
        false
    }

    fn read(&mut self, encoding: Encoding, scope: Scope, command: &Command) {
        let strings = command
            .strings
            .iter()
            .map(|string| encoding.decode(string).unwrap_or_default())
            .collect::<Vec<_>>();
        if !matches!(command.id, COMMENT | DEBUG_MESSAGE) {
            for string in strings.iter().filter(|string| looks_like_file(string)) {
                self.read_file_name(string);
            }
        }
        match command.id {
            COMMENT | DEBUG_MESSAGE | LABEL | JUMP_TO_LABEL => {}
            SHOW_MESSAGE | SHOW_CHOICES => {
                for string in &strings {
                    self.shown.extend(written_into(scope, string));
                }
            }
            PICTURE if is_picture_text(command) => {
                for string in &strings {
                    self.shown.extend(written_into(scope, string));
                }
                self.used(scope, &command.args[1..]);
            }
            SET_STRING => self.read_set_string(scope, command, &strings),
            CALL_EVENT | CALL_EVENT_BY_NAME => self.read_call(scope, command, &strings),
            STRING_CONDITION => self.read_condition(scope, command, &strings),
            other => {
                if matches!(other, DATABASE | DATABASE_FILE) {
                    self.keep(strings.iter().skip(1));
                }
                let database = (other == DATABASE)
                    .then(|| DatabaseCommand::of(scope, command, &strings))
                    .flatten();
                // Where a type's row name goes when it replaces what the
                // variable held is followed; the other numbers are not.
                let followed = database.as_ref().is_some_and(|command| {
                    command.of != TypeOf::Unknown && matches!(command.name_into, Some((_, true)))
                });
                if followed {
                    self.used(scope, &command.args[..4]);
                    self.used(scope, &command.args[5..]);
                } else {
                    self.used(scope, &command.args);
                }
                self.database_commands.extend(database);
                for string in &strings {
                    self.other_use.extend(written_into(scope, string));
                }
            }
        }
    }

    fn read_set_string(&mut self, scope: Scope, command: &Command, strings: &[String]) {
        let Some(set) = SetString::of(scope, command) else {
            self.used(scope, &command.args);
            return;
        };
        let Some(target) = set.target else {
            self.used(scope, &command.args[1..]);
            return;
        };
        match (set.source, set.operation) {
            (Source::Literal, Operation::Assign | Operation::Append) if strings.len() == 1 => {
                if looks_like_file(&strings[0]) {
                    self.takes_a_file.insert(target);
                }
                if set.operation == Operation::Append {
                    self.appended_to.insert(target);
                }
                for variable in written_into(scope, &strings[0]) {
                    self.flows.entry(variable).or_default().push(target);
                }
            }
            (Source::Variable, operation) if operation.moves_text() => {
                if operation == Operation::Append {
                    self.appended_to.insert(target);
                }
                match command.args.get(2).and_then(|&from| variable(scope, from)) {
                    Some(from) => self.flows.entry(from).or_default().push(target),
                    None => self.used(scope, &command.args[2..]),
                }
            }
            (Source::Literal, Operation::LooksFor) => {
                self.keep(strings.iter());
                self.compared
                    .entry(target)
                    .or_default()
                    .extend(strings.iter().filter(|word| !word.is_empty()).cloned());
                for string in strings {
                    self.other_use.extend(written_into(scope, string));
                }
            }
            (Source::Other, Operation::Assign | Operation::Append) => {
                self.used(scope, &command.args[2..]);
            }
            _ => {
                self.other_use.insert(target);
                self.used(scope, &command.args[1..]);
                for string in strings {
                    self.other_use.extend(written_into(scope, string));
                }
            }
        }
    }

    fn read_call(&mut self, scope: Scope, command: &Command, strings: &[String]) {
        let (Some(call), Some(event)) = (Call::of(command), self.called(command)) else {
            self.used(scope, &command.args);
            for string in strings {
                self.other_use.extend(written_into(scope, string));
            }
            return;
        };
        if command.id == CALL_EVENT_BY_NAME {
            self.keep(strings.iter().take(1));
        }
        let values = &command.args[2..];
        self.used(scope, &values[..call.numbers.min(values.len())]);
        for at in 0..call.strings {
            let argument = Variable::Own {
                event,
                number: FIRST_OWN_STRING + at as u32,
            };
            if call.is_literal(at) {
                let Some(string) = strings.get(at + 1) else {
                    continue;
                };
                if looks_like_file(string) {
                    self.takes_a_file.insert(argument);
                }
                for variable in written_into(scope, string) {
                    self.flows.entry(variable).or_default().push(argument);
                }
            } else if let Some(from) = values
                .get(call.numbers + at)
                .and_then(|&from| variable(scope, from))
            {
                self.flows.entry(from).or_default().push(argument);
            }
        }
        // What the event returns goes into a variable; one that takes it
        // holds more than the strings set into it.
        if let Some(rest) = values.get(call.numbers + call.strings..) {
            self.used(scope, rest);
        }
    }

    fn read_condition(&mut self, scope: Scope, command: &Command, strings: &[String]) {
        let Some(&cases) = command.args.first() else {
            return;
        };
        let cases = (cases & 0xf) as usize;
        for (at, &compared) in command.args.iter().skip(1).enumerate() {
            let left = variable(scope, compared & 0x00ff_ffff);
            let with_variable = (compared >> 24) & 0xf != 0;
            match (left, strings.get(at)) {
                (Some(left), Some(word)) if at < cases && !with_variable => {
                    self.keep([word]);
                    if !word.is_empty() {
                        self.compared.entry(left).or_default().push(word.clone());
                    }
                }
                (Some(left), _) => {
                    self.other_use.insert(left);
                }
                (None, _) => {}
            }
        }
        for string in strings {
            self.other_use.extend(written_into(scope, string));
        }
    }

    /// String variables named among a command's numbers are used in a way
    /// the launcher does not follow.
    fn used(&mut self, scope: Scope, args: &[u32]) {
        self.other_use.extend(args.iter().filter_map(|&arg| {
            variable(scope, arg).or_else(|| variable(scope, arg & 0x00ff_ffff))
        }));
    }

    fn keep<'a>(&mut self, strings: impl IntoIterator<Item = &'a String>) {
        self.kept.extend(
            strings
                .into_iter()
                .filter(|string| !string.is_empty())
                .cloned(),
        );
    }

    /// Takes a database's string: a file's name there is one of the game's
    /// files, and a string variable written into it is part of that name.
    pub fn read_database_value(&mut self, value: &str, is_file: bool) {
        if !is_file && !looks_like_file(value) {
            return;
        }
        self.read_file_name(value);
        self.other_use.extend(written_into(Scope::MapEvent, value));
    }

    /// Takes a string that is a file's name: its name without folder and
    /// extension and the words in it are words of the game's files, and
    /// one with a string written into it, or only a folder, is a name
    /// being put together.
    fn read_file_name(&mut self, path: &str) {
        let lower = path.to_ascii_lowercase();
        let built = path.ends_with(['/', '\\'])
            || ["\\cself[", "\\s[", "\\cdb[", "\\udb[", "\\sdb["]
                .iter()
                .any(|code| lower.contains(code));
        self.builds_file_names |= built;
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        self.file_words.insert(stem.to_owned());
        self.file_words.extend(
            stem.split(|character: char| {
                character.is_ascii_digit() || "-_ ()（）　.".contains(character)
            })
            .filter(|word| word.chars().count() > 1)
            .map(str::to_owned),
        );
    }
}

fn text_of(encoding: Encoding, command: &Command, string: usize) -> Option<String> {
    command
        .strings
        .get(string)
        .and_then(|string| encoding.decode(string))
        .filter(|text| !text.is_empty())
}

fn variable(scope: Scope, reference: u32) -> Option<Variable> {
    if (STRING_VARIABLES..STRING_VARIABLES_END).contains(&reference) {
        return Some(Variable::Shared(reference - STRING_VARIABLES));
    }
    let number = reference.checked_sub(COMMON_EVENT_VARIABLES)?;
    match scope {
        Scope::CommonEvent(event) if (FIRST_OWN_STRING..=LAST_OWN_STRING).contains(&number) => {
            Some(Variable::Own { event, number })
        }
        _ => None,
    }
}

/// The string variables a string writes in through `\cself[n]` and
/// `\s[n]`.
fn written_into(scope: Scope, text: &str) -> Vec<Variable> {
    let mut found = Vec::new();
    let lower = text.to_ascii_lowercase();
    for (code, own) in [("\\cself[", true), ("\\s[", false)] {
        for (at, _) in lower.match_indices(code) {
            let digits = &lower[at + code.len()..];
            let Some(end) = digits.find(']') else {
                continue;
            };
            let Ok(number) = digits[..end].parse::<u32>() else {
                continue;
            };
            let variable = if own {
                variable(scope, COMMON_EVENT_VARIABLES + number)
            } else {
                Some(Variable::Shared(number))
            };
            found.extend(variable);
        }
    }
    found
}

/// A picture command that shows its string as text.
pub fn is_picture_text(command: &Command) -> bool {
    command
        .args
        .first()
        .is_some_and(|flags| flags & 0xf == 0 && (flags >> 4) & 0xf == 2)
}

/// A file's path or name: a folder the game reads from, or an extension,
/// and no space or sentence in it.
pub fn looks_like_file(text: &str) -> bool {
    if text.contains(['\n', '\r', ' ', '　', '。', '、']) {
        return false;
    }
    let extension = text.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty()
            && (2..=4).contains(&extension.len())
            && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
    });
    let folder = text.split_once('/').is_some_and(|(folder, _)| {
        !folder.is_empty()
            && folder
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    });
    extension || folder
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Source {
    Literal,
    Variable,
    Other,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Operation {
    Assign,
    Append,
    /// Takes the source's first line or character.
    TakePart,
    /// Removes, replaces or cuts at a word.
    LooksFor,
    Other,
}

impl Operation {
    fn moves_text(self) -> bool {
        matches!(self, Self::Assign | Self::Append | Self::TakePart)
    }
}

struct SetString {
    target: Option<Variable>,
    source: Source,
    operation: Operation,
}

impl SetString {
    fn of(scope: Scope, command: &Command) -> Option<Self> {
        let (&target, &flags) = (command.args.first()?, command.args.get(1)?);
        Some(Self {
            target: variable(scope, target),
            source: match flags & 0xff {
                0 => Source::Literal,
                1 => Source::Variable,
                _ => Source::Other,
            },
            operation: match (flags >> 8) & 0xff {
                0 => Operation::Assign,
                1 => Operation::Append,
                2..=4 => Operation::TakePart,
                8..=11 => Operation::LooksFor,
                _ => Operation::Other,
            },
        })
    }
}

/// A call's arguments: so many numbers, then so many strings, each either
/// written in the command or taken from a variable.
struct Call {
    numbers: usize,
    strings: usize,
    literal: u32,
}

impl Call {
    fn of(command: &Command) -> Option<Self> {
        let flags = *command.args.get(1)?;
        Some(Self {
            numbers: (flags & 0xf) as usize,
            strings: ((flags >> 4) & 0xf) as usize,
            literal: (flags >> 12) & 0xf,
        })
    }

    fn is_literal(&self, string: usize) -> bool {
        self.literal >> string & 1 == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wolf::{
        commands::testing::{command, message},
        common_events::testing::common_events,
        map::testing::map,
        text::decode,
    };

    const OWN_5: u32 = COMMON_EVENT_VARIABLES + 5;
    const OWN_6: u32 = COMMON_EVENT_VARIABLES + 6;
    const OWN_7: u32 = COMMON_EVENT_VARIABLES + 7;
    const OWN_8: u32 = COMMON_EVENT_VARIABLES + 8;
    const SHARED_7: u32 = STRING_VARIABLES + 7;
    const TEXT_PICTURE: u32 = 0x20;

    fn set(target: u32, flags: u32, text: &str) -> Command {
        command(SET_STRING, &[target, flags], &[text])
    }

    fn shown(usage: &Usage, scope: Scope, command: &Command) -> Vec<bool> {
        usage
            .literals(scope, command)
            .iter()
            .map(|literal| {
                usage.is_shown(
                    literal.target,
                    &decode(&command.strings[literal.string]).unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn a_rows_name_is_text_when_an_event_reads_it_only_to_show_it() {
        const FACES: &str = "顔グラフィック名";
        let name_of = |type_number: u32, what: u32, target: u32, type_name: &str| {
            command(
                DATABASE,
                &[type_number, 1_600_010, ROW_NAME, what, target],
                &["", type_name, "", ""],
            )
        };
        let events = common_events(vec![
            // A message window of the game's own shows the speaker's name.
            vec![
                name_of(24, 0x1100, OWN_5, ""),
                command(PICTURE, &[TEXT_PICTURE, 1], &["\\f[17]\\cself[5]"]),
            ],
            // A map's name is read, shown, and compared with one word; a
            // weapon's name is compared with another string.
            vec![
                name_of(0, 0x1100, OWN_6, ""),
                command(STRING_CONDITION, &[1, OWN_6], &["町"]),
                name_of(0, 0x1_1100, OWN_7, "武器"),
                command(STRING_CONDITION, &[0x11, 0x0100_0000 | OWN_5, OWN_7], &[]),
                message("\\cself[6]\\cself[7]"),
            ],
            // A type picked by its name, in the user database.
            vec![name_of(0, 0x1_1200, OWN_5, "道具"), message("\\cself[5]")],
            // A face is found by its row's name.
            vec![command(
                DATABASE,
                &[24, 0, 0, 0x2_1100, 2_000_000],
                &["", "", "受付", ""],
            )],
            // A window for any type looks for a word in the names it reads,
            // and adds a name of type 24 to a string nothing shows.
            vec![
                name_of(1_600_011, 0x1100, OWN_7, ""),
                command(STRING_CONDITION, &[1, OWN_7], &["ミュスカ"]),
                name_of(24, 0x1110, OWN_8, ""),
            ],
        ]);
        let usage = Usage::of(&events, &[]);
        assert!(usage.reads_row_names(Base::System, 24, FACES));
        assert!(usage.shows_row_name(Base::System, 24, FACES, "男Ａ"));
        assert!(!usage.shows_row_name(Base::System, 24, FACES, "受付"));
        assert!(!usage.shows_row_name(Base::System, 24, FACES, "ミュスカ"));
        assert!(!usage.shows_row_name(Base::System, 0, "マップ設定", "町"));
        assert!(usage.shows_row_name(Base::System, 0, "マップ設定", "洞窟"));
        assert!(!usage.reads_row_names(Base::System, 5, "武器"));
        assert!(!usage.reads_row_names(Base::System, 23, FACES));
        assert!(!usage.reads_row_names(Base::User, 24, FACES));
        assert!(usage.reads_row_names(Base::User, 3, "道具"));
        assert!(!usage.reads_row_names(Base::User, 3, "武器"));
        assert!(!usage.reads_row_names(Base::Changeable, 3, "道具"));
        // The variable a name is put into is still one whose other strings
        // are text.
        let label = set(OWN_5, 0, "名無し");
        assert_eq!(shown(&usage, Scope::CommonEvent(0), &label), [true]);
        assert_eq!(
            shown(&usage, Scope::CommonEvent(4), &set(OWN_8, 0, "名")),
            [false]
        );
    }

    #[test]
    fn a_set_string_is_text_only_when_its_variable_is_shown_and_nothing_else() {
        let label = set(OWN_5, 0, "が足りない！");
        let word = set(OWN_6, 0, "ミュスカ");
        let unused = set(SHARED_7, 0, "どこにも出ない");
        let events = common_events(vec![vec![
            label.clone(),
            message("@1\n\\cself[5]"),
            word.clone(),
            message("\\cself[6]だ"),
            command(STRING_CONDITION, &[1, OWN_6], &["ミュスカ"]),
            unused.clone(),
        ]]);
        let usage = Usage::of(&events, &[]);
        let scope = Scope::CommonEvent(0);
        assert_eq!(shown(&usage, scope, &label), [true]);
        // Shown, but compared with the very word.
        assert_eq!(shown(&usage, scope, &word), [false]);
        assert_eq!(shown(&usage, scope, &unused), [false]);
        assert!(usage.kept.contains("ミュスカ"));
        assert!(usage.kept.contains("どこにも出ない"));
        assert!(!usage.kept.contains("が足りない！"));
    }

    #[test]
    fn a_comparison_with_nothing_or_another_word_leaves_the_text_alone() {
        let line = set(OWN_5, 0x100, "は倒れた。");
        let events = common_events(vec![vec![
            line.clone(),
            command(STRING_CONDITION, &[2, OWN_5, OWN_5], &["", "なし"]),
            command(PICTURE, &[TEXT_PICTURE, 1, 0], &["\\f[12]\\cself[5]"]),
        ]]);
        let usage = Usage::of(&events, &[]);
        let scope = Scope::CommonEvent(0);
        assert_eq!(shown(&usage, scope, &line), [true]);
        assert!(usage.literals(scope, &line)[0].appended);
        let other = set(OWN_5, 0, "なしでは済まない");
        assert_eq!(shown(&usage, scope, &other), [false]);
    }

    #[test]
    fn a_variable_used_for_a_file_a_number_or_another_variable_is_not_text() {
        let in_file = set(OWN_5, 0, "立ち絵");
        let with_file = set(OWN_6, 0, "説明だよ");
        let events = common_events(vec![
            vec![
                in_file.clone(),
                command(PICTURE, &[0, 1, 0], &["Picture/\\cself[5].png"]),
                message("\\cself[5]"),
            ],
            vec![
                with_file.clone(),
                set(OWN_6, 0, "Picture/顔-笑い.png"),
                message("\\cself[6]"),
            ],
            vec![
                set(OWN_5, 0, "数字"),
                message("\\cself[5]"),
                command(121, &[COMMON_EVENT_VARIABLES + 20, OWN_5, 0, 0], &[]),
            ],
            vec![
                set(OWN_5, 0, "比べる"),
                message("\\cself[5]"),
                command(STRING_CONDITION, &[1, (1 << 24) | OWN_5, OWN_6], &[""]),
            ],
        ]);
        let usage = Usage::of(&events, &[]);
        assert_eq!(shown(&usage, Scope::CommonEvent(0), &in_file), [false]);
        assert_eq!(shown(&usage, Scope::CommonEvent(1), &with_file), [false]);
        for event in [2, 3] {
            let first = &events.events[event].commands[0];
            assert_eq!(
                shown(&usage, Scope::CommonEvent(event as u32), first),
                [false]
            );
        }
        // The first event writes a string into a file's name, so the words
        // of the game's file names are its own.
        assert!(usage.builds_file_names);
        for word in ["顔-笑い", "笑い", "立ち絵"] {
            assert!(usage.keeps(word), "{word}");
        }
        assert!(!usage.keeps("顔"));
    }

    #[test]
    fn follows_a_string_into_the_event_it_is_handed_to_and_on() {
        // Event 1 shows its first string argument and compares its second;
        // event 2 hands its own argument on to event 1.
        let direct = command(
            CALL_EVENT,
            &[COMMON_EVENTS + 1, 0x3021, 9, 0, 0],
            &["", "おいで…♪", "ミュスカ5"],
        );
        let by_name = command(
            CALL_EVENT_BY_NAME,
            &[0, 0x1010, 0],
            &["コモン2", "二階のトイレが壊れた"],
        );
        let from_variable = command(CALL_EVENT, &[COMMON_EVENTS + 1, 0x0020, OWN_5, OWN_5], &[]);
        let events = common_events(vec![
            vec![direct.clone(), by_name.clone()],
            vec![
                command(PICTURE, &[0x1020, 5, 0], &["\\cself[5]"]),
                command(STRING_CONDITION, &[1, OWN_6], &["ミュスカ5"]),
            ],
            vec![from_variable],
        ]);
        let maps = [("Map.mps".to_owned(), map(vec![vec![direct.clone()]]))];
        let usage = Usage::of(&events, &maps);
        assert_eq!(usage.called(&by_name), Some(2));
        assert_eq!(shown(&usage, Scope::CommonEvent(0), &direct), [true, false]);
        assert_eq!(shown(&usage, Scope::MapEvent, &direct), [true, false]);
        assert_eq!(shown(&usage, Scope::CommonEvent(0), &by_name), [true]);
        assert!(usage.kept.contains("コモン2"));

        // A call the command does not name, and a missing event.
        let unknown = command(
            CALL_EVENT,
            &[COMMON_EVENT_VARIABLES, 0x1010, 0],
            &["", "誰だ"],
        );
        assert!(usage.literals(Scope::MapEvent, &unknown).is_empty());
        let missing = command(CALL_EVENT_BY_NAME, &[0, 0x1010, 0], &["ない", "誰だ"]);
        assert!(usage.literals(Scope::MapEvent, &missing).is_empty());
    }

    #[test]
    fn keeps_the_words_of_database_lookups_and_cuts() {
        let events = common_events(vec![vec![
            command(
                DATABASE,
                &[0, 0, 0, 0x51000, OWN_5],
                &["", "主人公", "", "名前"],
            ),
            set(OWN_6, 0x800, "色="),
            set(OWN_6, 0, "色=赤"),
            message("\\cself[6]"),
            command(COMMENT, &[], &["\\cself[5]はメモ"]),
        ]]);
        let usage = Usage::of(&events, &[]);
        for word in ["主人公", "名前", "色="] {
            assert!(usage.kept.contains(word), "{word}");
        }
        let option = &events.events[0].commands[2];
        assert_eq!(shown(&usage, Scope::CommonEvent(0), option), [false]);
    }

    #[test]
    fn a_games_file_names_are_its_own_words_only_when_it_builds_them() {
        let plain = common_events(vec![vec![
            command(PICTURE, &[0, 1, 0], &["Picture/薬草.png"]),
            set(OWN_5, 0, "SE/決定.wav"),
        ]]);
        let usage = Usage::of(&plain, &[]);
        assert!(!usage.builds_file_names);
        assert!(!usage.keeps("薬草"));

        let joined = common_events(vec![vec![
            set(OWN_5, 0, "Picture/薬草.png"),
            set(OWN_6, 0, "Picture/"),
        ]]);
        assert!(Usage::of(&joined, &[]).keeps("薬草"));
        let added = common_events(vec![vec![
            set(OWN_5, 0, "Picture/顔.png"),
            set(OWN_5, 0x100, "薬草"),
            command(PICTURE, &[0, 1, 0], &["Picture/薬草.png"]),
        ]]);
        assert!(Usage::of(&added, &[]).keeps("薬草"));
    }

    #[test]
    fn tells_file_names_from_text() {
        for file in [
            "Picture/黒.png",
            "SE/刺す.wav",
            "Data/key.csv",
            "顔.png",
            "BGM/",
        ] {
            assert!(looks_like_file(file), "{file}");
        }
        for text in [
            "はい",
            "HP/MP を回復",
            "ver.1 です。",
            "残り…3.",
            "一行目\nPicture/a.png",
        ] {
            assert!(!looks_like_file(text), "{text}");
        }
        assert!(is_picture_text(&command(PICTURE, &[0x4020], &["文"])));
        assert!(!is_picture_text(&command(PICTURE, &[0x1000], &["a.png"])));
        assert!(!is_picture_text(&command(PICTURE, &[0x21], &[])));
    }
}
