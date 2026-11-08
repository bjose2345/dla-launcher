//! Ruby's Marshal format (version 4.8) as far as RPG Maker VX Ace's data
//! files use it. The reader keeps every value but follows links only to
//! strings, which is all text extraction needs; the writer writes the plain
//! shapes the launcher hands the game: arrays, hashes, integers and strings.

use std::fmt;

const VERSION: [u8; 2] = [4, 8];

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Integer(i64),
    /// A string's bytes; RPG Maker VX Ace's text is UTF-8.
    String(Vec<u8>),
    Symbol(String),
    Array(Vec<Value>),
    Hash(Vec<(Value, Value)>),
    /// An instance of a Ruby class (`RPG::Map`), its fields named with `@`.
    Object {
        class: String,
        fields: Vec<(String, Value)>,
    },
    /// A float, a big number, a class's own dump (`Table`), or a link to a
    /// value that is not a string.
    Other,
}

impl Value {
    pub fn field(&self, name: &str) -> &Value {
        match self {
            Value::Object { fields, .. } => fields
                .iter()
                .find(|(field, _)| field == name)
                .map(|(_, value)| value)
                .unwrap_or(&Value::Nil),
            _ => &Value::Nil,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(bytes) => std::str::from_utf8(bytes).ok(),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::String(bytes) => Some(bytes),
            _ => None,
        }
    }

    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Value::Integer(value) => Some(*value),
            _ => None,
        }
    }

    pub fn items(&self) -> &[Value] {
        match self {
            Value::Array(items) => items,
            _ => &[],
        }
    }

    /// A hash's values, or an array's items.
    pub fn values(&self) -> Vec<&Value> {
        match self {
            Value::Array(items) => items.iter().collect(),
            Value::Hash(pairs) => pairs.iter().map(|(_, value)| value).collect(),
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct MarshalError(String);

impl fmt::Display for MarshalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "not a Ruby data file the launcher can read: {}",
            self.0
        )
    }
}

impl std::error::Error for MarshalError {}

pub fn read(bytes: &[u8]) -> Result<Value, MarshalError> {
    if bytes.get(..2) != Some(&VERSION[..]) {
        return Err(MarshalError("unknown version".to_owned()));
    }
    let mut reader = Reader {
        bytes,
        at: 2,
        symbols: Vec::new(),
        objects: Vec::new(),
    };
    reader.value()
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    symbols: Vec<String>,
    // Ruby numbers every object as it appears; only strings are kept.
    objects: Vec<Option<Vec<u8>>>,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, MarshalError> {
        let byte = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| MarshalError("it ends early".to_owned()))?;
        self.at += 1;
        Ok(byte)
    }

    fn take(&mut self, count: usize) -> Result<&[u8], MarshalError> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| MarshalError("it ends early".to_owned()))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn long(&mut self) -> Result<i64, MarshalError> {
        let first = self.byte()? as i8;
        Ok(match first {
            0 => 0,
            5..=127 => i64::from(first) - 5,
            -128..=-5 => i64::from(first) + 5,
            1..=4 => {
                let bytes = self.take(first as usize)?;
                bytes
                    .iter()
                    .rev()
                    .fold(0_i64, |value, byte| (value << 8) | i64::from(*byte))
            }
            _ => {
                let count = usize::from(first.unsigned_abs());
                let bytes = self.take(count)?;
                let value = bytes
                    .iter()
                    .rev()
                    .fold(0_i64, |value, byte| (value << 8) | i64::from(*byte));
                value - (1_i64 << (8 * count))
            }
        })
    }

    fn length(&mut self) -> Result<usize, MarshalError> {
        usize::try_from(self.long()?).map_err(|_| MarshalError("a negative length".to_owned()))
    }

    fn symbol(&mut self) -> Result<String, MarshalError> {
        match self.byte()? {
            b':' => {
                let length = self.length()?;
                let name = String::from_utf8_lossy(self.take(length)?).into_owned();
                self.symbols.push(name.clone());
                Ok(name)
            }
            b';' => {
                let index = self.length()?;
                self.symbols
                    .get(index)
                    .cloned()
                    .ok_or_else(|| MarshalError("a symbol link out of range".to_owned()))
            }
            // A symbol with an encoding.
            b'I' => {
                let name = self.symbol()?;
                self.skip_fields()?;
                Ok(name)
            }
            other => Err(MarshalError(format!(
                "a symbol was expected, not {other:#04x}"
            ))),
        }
    }

    fn skip_fields(&mut self) -> Result<(), MarshalError> {
        for _ in 0..self.length()? {
            self.symbol()?;
            self.value()?;
        }
        Ok(())
    }

    fn register(&mut self) -> usize {
        self.objects.push(None);
        self.objects.len() - 1
    }

    fn value(&mut self) -> Result<Value, MarshalError> {
        Ok(match self.byte()? {
            b'0' => Value::Nil,
            b'T' => Value::Bool(true),
            b'F' => Value::Bool(false),
            b'i' => Value::Integer(self.long()?),
            b':' | b';' => {
                self.at -= 1;
                Value::Symbol(self.symbol()?)
            }
            b'"' => {
                let length = self.length()?;
                let bytes = self.take(length)?.to_vec();
                self.objects.push(Some(bytes.clone()));
                Value::String(bytes)
            }
            // A value with instance variables, as strings carry their encoding.
            b'I' => {
                let value = self.value()?;
                self.skip_fields()?;
                value
            }
            b'@' => {
                let index = self.length()?;
                match self.objects.get(index) {
                    Some(Some(bytes)) => Value::String(bytes.clone()),
                    Some(None) => Value::Other,
                    None => return Err(MarshalError("an object link out of range".to_owned())),
                }
            }
            b'[' => {
                self.register();
                let count = self.length()?;
                let mut items = Vec::with_capacity(count.min(1 << 16));
                for _ in 0..count {
                    items.push(self.value()?);
                }
                Value::Array(items)
            }
            kind @ (b'{' | b'}') => {
                self.register();
                let count = self.length()?;
                let mut pairs = Vec::with_capacity(count.min(1 << 16));
                for _ in 0..count {
                    let key = self.value()?;
                    pairs.push((key, self.value()?));
                }
                if kind == b'}' {
                    self.value()?;
                }
                Value::Hash(pairs)
            }
            kind @ (b'o' | b'S') => {
                self.register();
                let class = self.symbol()?;
                let count = self.length()?;
                let mut fields = Vec::with_capacity(count.min(1 << 10));
                for _ in 0..count {
                    let name = self.symbol()?;
                    fields.push((name, self.value()?));
                }
                if kind == b'S' {
                    return Ok(Value::Other);
                }
                Value::Object { class, fields }
            }
            b'u' => {
                self.symbol()?;
                let length = self.length()?;
                self.take(length)?;
                self.register();
                Value::Other
            }
            b'U' => {
                self.register();
                self.symbol()?;
                self.value()?;
                Value::Other
            }
            b'f' => {
                let length = self.length()?;
                self.take(length)?;
                self.register();
                Value::Other
            }
            b'l' => {
                self.byte()?;
                let length = self.length()?;
                self.take(length.saturating_mul(2))?;
                self.register();
                Value::Other
            }
            b'/' => {
                let length = self.length()?;
                self.take(length)?;
                self.byte()?;
                self.register();
                Value::Other
            }
            b'c' | b'm' | b'M' => {
                let length = self.length()?;
                self.take(length)?;
                self.register();
                Value::Other
            }
            b'e' | b'C' => {
                self.symbol()?;
                self.value()?
            }
            b'd' => {
                self.register();
                self.symbol()?;
                self.value()?;
                Value::Other
            }
            other => return Err(MarshalError(format!("unknown type {other:#04x}"))),
        })
    }
}

/// Writes Marshal data Ruby 1.9 reads back: UTF-8 strings carry their
/// encoding, other strings are binary.
pub struct Writer {
    bytes: Vec<u8>,
    symbols: Vec<String>,
}

impl Writer {
    pub fn new() -> Self {
        Self {
            bytes: VERSION.to_vec(),
            symbols: Vec::new(),
        }
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }

    fn long(&mut self, value: i64) {
        match value {
            0 => self.bytes.push(0),
            1..=122 => self.bytes.push((value + 5) as u8),
            -123..=-1 => self.bytes.push((value - 5) as i8 as u8),
            // Ruby's own: low bytes until only 0 or -1 is left, which the
            // count's sign stands for.
            _ => {
                let mut bytes = Vec::with_capacity(8);
                let mut rest = value;
                let count = loop {
                    bytes.push((rest & 0xff) as u8);
                    rest >>= 8;
                    if rest == 0 {
                        break bytes.len() as u8;
                    }
                    if rest == -1 {
                        break (-(bytes.len() as i8)) as u8;
                    }
                };
                self.bytes.push(count);
                self.bytes.extend_from_slice(&bytes);
            }
        }
    }

    pub fn integer(&mut self, value: i64) {
        self.bytes.push(b'i');
        self.long(value);
    }

    pub fn text(&mut self, text: &str) {
        self.bytes.push(b'I');
        self.binary(text.as_bytes());
        self.long(1);
        self.symbol("E");
        self.bytes.push(b'T');
    }

    pub fn binary(&mut self, bytes: &[u8]) {
        self.bytes.push(b'"');
        self.long(bytes.len() as i64);
        self.bytes.extend_from_slice(bytes);
    }

    /// Followed by that many values.
    pub fn array(&mut self, count: usize) {
        self.bytes.push(b'[');
        self.long(count as i64);
    }

    /// Followed by that many keys, each with its value.
    pub fn hash(&mut self, count: usize) {
        self.bytes.push(b'{');
        self.long(count as i64);
    }

    /// Followed by that many fields, each a [`Writer::symbol`] and a value.
    #[cfg(test)]
    pub fn object(&mut self, class: &str, fields: usize) {
        self.bytes.push(b'o');
        self.symbol(class);
        self.long(fields as i64);
    }

    /// A symbol the first time, a link to it after.
    pub(crate) fn symbol(&mut self, name: &str) {
        match self.symbols.iter().position(|known| known == name) {
            Some(index) => {
                self.bytes.push(b';');
                self.long(index as i64);
            }
            None => {
                self.bytes.push(b':');
                self.long(name.len() as i64);
                self.bytes.extend_from_slice(name.as_bytes());
                self.symbols.push(name.to_owned());
            }
        }
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_back_what_it_writes() {
        let mut writer = Writer::new();
        writer.array(4);
        writer.integer(77_000_001);
        writer.text("ルーク");
        writer.binary(&[0, 159, 255]);
        writer.hash(2);
        writer.text("剣");
        writer.text("Sword");
        writer.text("盾");
        writer.integer(-300);
        let value = read(&writer.finish()).unwrap();
        assert_eq!(
            value,
            Value::Array(vec![
                Value::Integer(77_000_001),
                Value::String("ルーク".as_bytes().to_vec()),
                Value::String(vec![0, 159, 255]),
                Value::Hash(vec![
                    (
                        Value::String("剣".as_bytes().to_vec()),
                        Value::String(b"Sword".to_vec())
                    ),
                    (
                        Value::String("盾".as_bytes().to_vec()),
                        Value::Integer(-300)
                    ),
                ]),
            ])
        );
    }

    #[test]
    fn writes_integers_as_ruby_does() {
        let written = |value| {
            let mut writer = Writer::new();
            writer.integer(value);
            writer.finish()[3..].to_vec()
        };
        assert_eq!(written(0), [0]);
        assert_eq!(written(122), [127]);
        assert_eq!(written(123), [1, 123]);
        assert_eq!(written(255), [1, 255]);
        assert_eq!(written(256), [2, 0, 1]);
        assert_eq!(written(-123), [128]);
        assert_eq!(written(-124), [255, 132]);
        assert_eq!(written(-256), [255, 0]);
        assert_eq!(written(-257), [254, 255, 254]);
        assert_eq!(written(77_000_001), [4, 0x41, 0xed, 0x96, 0x04]);
        for value in [
            0,
            5,
            122,
            123,
            255,
            256,
            65_535,
            1 << 30,
            -1,
            -123,
            -124,
            -256,
            -257,
            -70_000,
        ] {
            let mut writer = Writer::new();
            writer.integer(value);
            assert_eq!(
                read(&writer.finish()).unwrap(),
                Value::Integer(value),
                "{value}"
            );
        }
    }

    #[test]
    fn reads_rpg_maker_objects_with_links_to_strings() {
        // Two RPG::EventCommand objects; the second names its class and
        // fields by symbol links and its text by a link to the first's.
        let mut bytes = vec![4, 8, b'[', 7];
        bytes.extend_from_slice(b"o:\x16RPG::EventCommand\x07");
        bytes.extend_from_slice(b":\x0a@codei\x02\x91\x01");
        bytes.extend_from_slice(b":\x10@parameters[\x06I\"\x08\xe3\x81\x82\x06:\x06ET");
        // Objects so far: the list 0, the command 1, its parameters 2, the text 3.
        bytes.extend_from_slice(b"o;\x00\x07;\x06i\x02\x92\x01;\x07[\x06@\x08");
        let value = read(&bytes).unwrap();
        let commands = value.items();
        assert_eq!(commands[0].field("@code").as_integer(), Some(401));
        assert_eq!(
            commands[0].field("@parameters").items()[0].as_str(),
            Some("あ")
        );
        assert_eq!(commands[1].field("@code").as_integer(), Some(402));
        assert_eq!(
            commands[1].field("@parameters").items()[0].as_str(),
            Some("あ")
        );
    }
}
