//! One of WOLF RPG Editor's three databases (`DataBase`, `CDataBase`,
//! `SysDatabase` in `BasicData`): the `.dat` file holds every row's numbers
//! and strings by type, and the `.project` file says what the types, their
//! fields and their rows are called and what kind of value a field takes.
//! Version 3 writes the `.dat` file in two more forms, one of them packed
//! behind the file's first bytes; a game may hold both.

use std::io;

use super::{
    stream::{Reader, Writer, invalid, pack, unpack, unsupported},
    text::Encoding,
};

const START: [u8; 10] = *b"\0W\0\0OL\0FM\0";
const ENCODING_MARK: usize = 6;
/// The file ends with its version again.
const VERSION_2: u8 = 0xc1;
const VERSION_3: u8 = 0xc2;
const VERSION_3_PACKED: u8 = 0xc4;
const TYPE: [u8; 4] = [0xfe, 0xff, 0xff, 0xff];
/// Rows named in a way that puts one more string into the type, which no
/// game seen so far does.
const NAMED_WITH_A_STRING: u32 = 120_000;
const NUMBER_FIELDS: u32 = 1000;
const STRING_FIELDS: u32 = 2000;
/// A field the editor fills from a file dialog: a file's name.
pub const FILE_NAME_FIELD: u8 = 1;
/// A type whose rows are named by hand, in the `.project` file.
pub const NAMED_BY_HAND: u32 = 0;
/// A type whose rows are named after their first string field.
pub const NAMED_BY_FIRST_STRING: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Database {
    /// The `.project` file has no bytes that say so and is in the same.
    pub encoding: Encoding,
    version: u8,
    pub types: Vec<TypeRows>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeRows {
    /// How the editor names the type's rows.
    pub row_naming: u32,
    /// Each field's place among a row's numbers (from 1000) or strings
    /// (from 2000).
    fields: Vec<u32>,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    numbers: Vec<u32>,
    pub strings: Vec<Vec<u8>>,
}

impl Database {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        let other =
            || unsupported("the database is of a version the launcher does not read, or protected");
        let (start, rest) = bytes.split_at_checked(START.len() + 1).ok_or_else(other)?;
        let encoding =
            Encoding::marked(&start[..START.len()], &START, ENCODING_MARK).ok_or_else(other)?;
        let version = start[START.len()];
        let unpacked;
        let body = match version {
            VERSION_2 | VERSION_3 => rest,
            VERSION_3_PACKED => {
                unpacked = unpack(rest)?;
                &unpacked
            }
            _ => return Err(other()),
        };
        let mut reader = Reader::new(body);
        let types = (0..reader.count(TYPE.len())?)
            .map(|_| TypeRows::read(&mut reader))
            .collect::<io::Result<_>>()?;
        reader.expect(&[version], "the end of the database")?;
        if !reader.is_at_end() {
            return Err(invalid("the database goes on past its end"));
        }
        Ok(Self {
            encoding,
            version,
            types,
        })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        writer.count(self.types.len());
        for rows in &self.types {
            writer.bytes(&TYPE);
            writer.u32(rows.row_naming);
            writer.u32s(&rows.fields);
            writer.count(rows.rows.len());
            for row in &rows.rows {
                for number in &row.numbers {
                    writer.u32(*number);
                }
                for string in &row.strings {
                    writer.string(string);
                }
            }
        }
        writer.u8(self.version);
        let body = writer.finish();
        let mut bytes = START.to_vec();
        bytes[ENCODING_MARK] = self.encoding.mark();
        bytes.push(self.version);
        if self.version == VERSION_3_PACKED {
            bytes.extend_from_slice(&pack(&body));
        } else {
            bytes.extend_from_slice(&body);
        }
        bytes
    }
}

impl TypeRows {
    fn read(reader: &mut Reader<'_>) -> io::Result<Self> {
        reader.expect(&TYPE, "the start of a database type")?;
        let row_naming = reader.u32()?;
        if row_naming == NAMED_WITH_A_STRING {
            return Err(unsupported(
                "a database type names its rows in a way the launcher does not read",
            ));
        }
        let fields = reader.u32s()?;
        let strings = fields
            .iter()
            .filter(|&&field| field >= STRING_FIELDS)
            .count();
        let numbers = fields.len() - strings;
        let in_order = |start: u32, count: usize| {
            let mut places = fields
                .iter()
                .filter(|&&field| (field >= STRING_FIELDS) == (start == STRING_FIELDS))
                .map(|field| field.wrapping_sub(start) as usize)
                .collect::<Vec<_>>();
            places.sort_unstable();
            places.into_iter().eq(0..count)
        };
        if fields.iter().any(|&field| field < NUMBER_FIELDS)
            || !in_order(NUMBER_FIELDS, numbers)
            || !in_order(STRING_FIELDS, strings)
        {
            return Err(invalid(
                "a database type's fields are laid out in a way the launcher does not know",
            ));
        }
        let rows = (0..reader.count(numbers * 4 + strings * 5)?)
            .map(|_| {
                Ok(Row {
                    numbers: (0..numbers)
                        .map(|_| reader.u32())
                        .collect::<io::Result<_>>()?,
                    strings: (0..strings)
                        .map(|_| reader.string())
                        .collect::<io::Result<_>>()?,
                })
            })
            .collect::<io::Result<_>>()?;
        Ok(Self {
            row_naming,
            fields,
            rows,
        })
    }

    /// The fields that hold strings, as (the field's number, its place
    /// among a row's strings).
    pub fn string_fields(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.fields
            .iter()
            .enumerate()
            .filter(|&(_, &place)| place >= STRING_FIELDS)
            .map(|(field, &place)| (field, (place - STRING_FIELDS) as usize))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
    pub types: Vec<TypeNames>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeNames {
    pub name: Vec<u8>,
    pub fields: Vec<Vec<u8>>,
    pub rows: Vec<Vec<u8>>,
    description: Vec<u8>,
    /// What each field takes, by the field's number; the editor writes a
    /// fixed number of them.
    pub field_kinds: Vec<u8>,
    field_notes: Vec<Vec<u8>>,
    field_choices: Vec<Vec<Vec<u8>>>,
    field_choice_values: Vec<Vec<u32>>,
    field_defaults: Vec<u32>,
}

impl Project {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        let mut reader = Reader::new(bytes);
        let types = (0..reader.count(5)?)
            .map(|_| TypeNames::read(&mut reader))
            .collect::<io::Result<_>>()?;
        if !reader.is_at_end() {
            return Err(invalid("the database's project file goes on past its end"));
        }
        Ok(Self { types })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        writer.count(self.types.len());
        for names in &self.types {
            writer.string(&names.name);
            writer.strings(&names.fields);
            writer.strings(&names.rows);
            writer.string(&names.description);
            writer.count(names.field_kinds.len());
            writer.bytes(&names.field_kinds);
            writer.strings(&names.field_notes);
            writer.count(names.field_choices.len());
            for choices in &names.field_choices {
                writer.strings(choices);
            }
            writer.count(names.field_choice_values.len());
            for values in &names.field_choice_values {
                writer.u32s(values);
            }
            writer.u32s(&names.field_defaults);
        }
        writer.finish()
    }
}

impl TypeNames {
    fn read(reader: &mut Reader<'_>) -> io::Result<Self> {
        let name = reader.string()?;
        let fields = reader.strings()?;
        let rows = reader.strings()?;
        let description = reader.string()?;
        let kinds = reader.count(1)?;
        Ok(Self {
            name,
            fields,
            rows,
            description,
            field_kinds: reader.bytes(kinds)?.to_vec(),
            field_notes: reader.strings()?,
            field_choices: (0..reader.count(4)?)
                .map(|_| reader.strings())
                .collect::<io::Result<_>>()?,
            field_choice_values: (0..reader.count(4)?)
                .map(|_| reader.u32s())
                .collect::<io::Result<_>>()?,
            field_defaults: reader.u32s()?,
        })
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::wolf::text::encode;

    pub struct Field {
        pub name: &'static str,
        pub kind: u8,
        /// A string field's value in each row; `None` for a number field.
        pub strings: Option<Vec<&'static str>>,
    }

    pub fn text(name: &'static str, values: &[&'static str]) -> Field {
        Field {
            name,
            kind: 0,
            strings: Some(values.to_vec()),
        }
    }

    pub fn number(name: &'static str) -> Field {
        Field {
            name,
            kind: 0,
            strings: None,
        }
    }

    /// A type with `rows` rows named `row_names`, and its place in the
    /// `.dat` and the `.project` file.
    pub fn database_type(
        name: &str,
        row_naming: u32,
        row_names: &[&str],
        fields: Vec<Field>,
    ) -> (TypeRows, TypeNames) {
        let (mut numbers, mut strings) = (0, 0);
        let places = fields
            .iter()
            .map(|field| {
                if field.strings.is_some() {
                    strings += 1;
                    STRING_FIELDS + strings - 1
                } else {
                    numbers += 1;
                    NUMBER_FIELDS + numbers - 1
                }
            })
            .collect();
        let rows = (0..row_names.len())
            .map(|row| Row {
                numbers: (0..numbers)
                    .map(|number| number * 10 + row as u32)
                    .collect(),
                strings: fields
                    .iter()
                    .filter_map(|field| field.strings.as_ref())
                    .map(|values| encode(values[row]))
                    .collect(),
            })
            .collect();
        let mut field_kinds = vec![0; 100];
        for (at, field) in fields.iter().enumerate() {
            field_kinds[at] = field.kind;
        }
        (
            TypeRows {
                row_naming,
                fields: places,
                rows,
            },
            TypeNames {
                name: encode(name),
                fields: fields.iter().map(|field| encode(field.name)).collect(),
                rows: row_names.iter().map(|name| encode(name)).collect(),
                description: encode("この型の説明"),
                field_kinds,
                field_notes: fields.iter().map(|_| Vec::new()).collect(),
                field_choices: fields.iter().map(|_| vec![encode("選択肢")]).collect(),
                field_choice_values: fields.iter().map(|_| vec![4]).collect(),
                field_defaults: fields.iter().map(|_| 0).collect(),
            },
        )
    }

    pub fn database(types: Vec<(TypeRows, TypeNames)>) -> (Database, Project) {
        let (rows, names) = types.into_iter().unzip();
        (
            Database {
                encoding: Encoding::ShiftJis,
                version: VERSION_2,
                types: rows,
            },
            Project { types: names },
        )
    }

    /// The same rows as version 3 keeps them, packed or not; their
    /// strings are the caller's to encode.
    pub fn newer(mut database: Database, packed: bool) -> Database {
        database.encoding = Encoding::Utf8;
        database.version = if packed { VERSION_3_PACKED } else { VERSION_3 };
        database
    }
}

#[cfg(test)]
mod tests {
    use super::{testing::*, *};

    fn items() -> (Database, Project) {
        database(vec![
            database_type(
                "アイテム",
                NAMED_BY_FIRST_STRING,
                &["薬草", "鍵"],
                vec![
                    text("名前", &["薬草", "鍵"]),
                    number("値段"),
                    text("説明", &["HPを回復する。\r\n苦い。", ""]),
                    Field {
                        name: "画像",
                        kind: FILE_NAME_FIELD,
                        strings: Some(vec!["Picture/薬草.png", ""]),
                    },
                ],
            ),
            database_type("空", 0, &[], Vec::new()),
        ])
    }

    #[test]
    fn writes_both_files_back_byte_for_byte() {
        let (database, project) = items();
        let bytes = database.write();
        let parsed = Database::parse(&bytes).unwrap();
        assert_eq!(parsed, database);
        assert_eq!(parsed.write(), bytes);
        assert_eq!(
            parsed.types[0].string_fields().collect::<Vec<_>>(),
            [(0, 0), (2, 1), (3, 2)]
        );
        let bytes = project.write();
        let parsed = Project::parse(&bytes).unwrap();
        assert_eq!(parsed, project);
        assert_eq!(parsed.write(), bytes);
        assert_eq!(parsed.types[0].field_kinds[3], FILE_NAME_FIELD);
    }

    #[test]
    fn reads_and_writes_both_forms_of_version_3() {
        let plain = newer(items().0, false);
        let bytes = plain.write();
        assert_eq!(bytes[..11], *b"\0W\0\0OLUFM\0\xc2");
        assert_eq!(bytes.last(), Some(&0xc2));
        let end = bytes.len() - 1;
        assert_eq!(bytes[11..end], items().0.write()[11..end]);
        let parsed = Database::parse(&bytes).unwrap();
        assert_eq!(parsed, plain);
        assert_eq!(parsed.write(), bytes);

        let packed = newer(items().0, true);
        let bytes = packed.write();
        assert_eq!(bytes[..11], *b"\0W\0\0OLUFM\0\xc4");
        let unpacked = unpack(&bytes[11..]).unwrap();
        assert_eq!(unpacked.last(), Some(&0xc4));
        assert_eq!(unpacked[..4], [2, 0, 0, 0]);
        let parsed = Database::parse(&bytes).unwrap();
        assert_eq!(parsed, packed);
        assert_eq!(parsed.write(), bytes);

        // A file that ends with another version's byte.
        let mut mixed = plain.write();
        *mixed.last_mut().unwrap() = 0xc4;
        assert_eq!(
            Database::parse(&mixed).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let mut named = plain.write();
        named[11 + 4 + TYPE.len()..][..4].copy_from_slice(&NAMED_WITH_A_STRING.to_le_bytes());
        assert_eq!(
            Database::parse(&named).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }

    #[test]
    fn refuses_protected_files_and_stops_at_damage() {
        let bytes = items().0.write();
        let mut protected = bytes.clone();
        protected[0] = 0x31;
        assert_eq!(
            Database::parse(&protected).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        let mut unknown = bytes.clone();
        unknown[10] = 0xc3;
        assert_eq!(
            Database::parse(&unknown).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        // A field placed among the strings twice.
        let mut twice = bytes.clone();
        let fields_at = START.len() + 1 + 4 + TYPE.len() + 4 + 4;
        twice[fields_at + 8..fields_at + 12].copy_from_slice(&STRING_FIELDS.to_le_bytes());
        assert_eq!(
            Database::parse(&twice).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(Database::parse(&bytes[..bytes.len() - 1]).is_err());
        assert!(Project::parse(&items().1.write()[..40]).is_err());
    }
}
