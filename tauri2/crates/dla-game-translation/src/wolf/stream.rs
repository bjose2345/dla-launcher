//! The primitives WOLF RPG Editor's data files are made of: little-endian
//! integers and strings stored as a length (counting the closing NUL) and
//! their bytes in the game's code page, one after another with no table of
//! offsets, so a file is read to its end or not at all. Version 3 packs
//! most files with LZ4 behind their first bytes.

use std::io;

pub fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// A file from a version of the editor the launcher does not read: stopped
/// at, never guessed through.
pub fn unsupported(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message.into())
}

/// What version 3 keeps after a file's first bytes: the size of the data,
/// the size it is packed to, and the data as one LZ4 block to the file's
/// end.
pub fn unpack(packed: &[u8]) -> io::Result<Vec<u8>> {
    let mut reader = Reader::new(packed);
    let (size, packed_size) = (reader.u32()? as usize, reader.u32()? as usize);
    let block = reader.bytes(packed_size)?;
    // A block cannot hold more than 255 times its own size.
    if !reader.is_at_end() || size > packed_size.saturating_mul(255) {
        return Err(invalid("the packed data is not as long as it says"));
    }
    match lz4_flex::block::decompress(block, size) {
        Ok(data) if data.len() == size => Ok(data),
        _ => Err(invalid("the packed data cannot be unpacked")),
    }
}

pub fn pack(data: &[u8]) -> Vec<u8> {
    let block = lz4_flex::block::compress(data);
    let mut writer = Writer::default();
    writer.count(data.len());
    writer.count(block.len());
    writer.bytes(&block);
    writer.finish()
}

pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub fn at(&self) -> usize {
        self.at
    }

    pub fn is_at_end(&self) -> bool {
        self.at == self.bytes.len()
    }

    #[cfg(test)]
    pub fn rest(&mut self) -> &'a [u8] {
        let rest = &self.bytes[self.at..];
        self.at = self.bytes.len();
        rest
    }

    pub fn bytes(&mut self, length: usize) -> io::Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(length)
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| invalid(format!("the file ends inside a value at {}", self.at)))?;
        let bytes = &self.bytes[self.at..end];
        self.at = end;
        Ok(bytes)
    }

    pub fn array<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let mut array = [0; N];
        array.copy_from_slice(self.bytes(N)?);
        Ok(array)
    }

    pub fn u8(&mut self) -> io::Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// A count that the rest of the file could hold at `least` bytes an
    /// item, so a damaged count never reserves memory for it.
    pub fn count(&mut self, least: usize) -> io::Result<usize> {
        let at = self.at;
        let count = self.u32()? as usize;
        if count.saturating_mul(least.max(1)) > self.bytes.len() - self.at {
            return Err(invalid(format!("a count at {at} runs past the file's end")));
        }
        Ok(count)
    }

    pub fn expect(&mut self, expected: &[u8], what: &str) -> io::Result<()> {
        let at = self.at;
        if self.bytes(expected.len())? == expected {
            Ok(())
        } else {
            Err(invalid(format!("{what} was expected at {at}")))
        }
    }

    /// A string without its closing NUL.
    pub fn string(&mut self) -> io::Result<Vec<u8>> {
        let at = self.at;
        let length = self.u32()? as usize;
        match self.bytes(length)?.split_last() {
            Some((0, text)) => Ok(text.to_vec()),
            _ => Err(invalid(format!("a string at {at} does not end as one"))),
        }
    }

    pub fn strings(&mut self) -> io::Result<Vec<Vec<u8>>> {
        (0..self.count(5)?).map(|_| self.string()).collect()
    }

    pub fn u32s(&mut self) -> io::Result<Vec<u32>> {
        (0..self.count(4)?).map(|_| self.u32()).collect()
    }
}

#[derive(Default)]
pub struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }

    pub fn bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    pub fn count(&mut self, count: usize) {
        self.u32(count as u32);
    }

    pub fn string(&mut self, text: &[u8]) {
        self.count(text.len() + 1);
        self.bytes(text);
        self.u8(0);
    }

    pub fn strings(&mut self, strings: &[Vec<u8>]) {
        self.count(strings.len());
        for string in strings {
            self.string(string);
        }
    }

    pub fn u32s(&mut self, values: &[u32]) {
        self.count(values.len());
        for value in values {
            self.u32(*value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_data_behind_its_two_sizes_and_unpacks_it() {
        let data = b"WOLF WOLF WOLF WOLF WOLF WOLF WOLF WOLF".repeat(40);
        let packed = pack(&data);
        assert_eq!(packed[..4], (data.len() as u32).to_le_bytes());
        assert_eq!(packed[4..8], (packed.len() as u32 - 8).to_le_bytes());
        assert!(packed.len() < data.len() / 4);
        assert_eq!(unpack(&packed).unwrap(), data);
        assert_eq!(unpack(&pack(b"")).unwrap(), b"");
    }

    #[test]
    fn unpacks_a_block_another_packer_wrote() {
        // "WOLF RPG Editor " six times over, as the reference LZ4 packs it.
        let block = b"\xff\x01WOLF RPG Editor \x10\x008Pitor ";
        let mut packed = vec![96, 0, 0, 0, block.len() as u8, 0, 0, 0];
        packed.extend_from_slice(block);
        assert_eq!(unpack(&packed).unwrap(), b"WOLF RPG Editor ".repeat(6));
    }

    #[test]
    fn refuses_packed_data_that_is_not_what_it_says() {
        let data = b"WOLF WOLF WOLF WOLF WOLF WOLF WOLF WOLF".repeat(40);
        let packed = pack(&data);
        let mut longer = packed.clone();
        longer.push(0);
        assert!(unpack(&longer).is_err());
        assert!(unpack(&packed[..packed.len() - 1]).is_err());
        let mut other_size = packed.clone();
        other_size[0] ^= 1;
        assert!(unpack(&other_size).is_err());
        let mut huge = packed.clone();
        huge[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(unpack(&huge).is_err());
        // A first run of literals longer than the block.
        let mut damaged = packed;
        damaged[8..12].fill(0xff);
        assert!(unpack(&damaged).is_err());
    }
}
