//! Fire Emblem Awakening's message archives (`m/E/*.bin`, LZ11-packed in the RomFS), as far as
//! Naga's Tear needs them: read, add messages, write back.
//!
//! Layout (from the game's files): a 0x20-byte header { file size, data size, pointer count,
//! label count, 0 x4 }; the data (the archive's name, then UTF-16LE texts, offsets from the start
//! of the data); the pointer table (pointer count u32s); the label table (label count pairs of
//! { text offset in the data, name offset in the names }); the names (Shift-JIS, NUL-terminated).
//! The game puts every label in one global name table, so where a label sits does not matter.

use std::fmt;

const HEADER: usize = 0x20;

#[derive(Debug, Clone, PartialEq)]
pub struct Archive {
    pub data: Vec<u8>,
    pub pointers: Vec<u32>,
    /// (text offset in data, name offset in names)
    pub labels: Vec<(u32, u32)>,
    pub names: Vec<u8>,
    /// the four header words after the counts, kept as they were
    pub reserved: [u32; 4],
}

#[derive(Debug)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn word(data: &[u8], at: usize) -> Result<u32, Error> {
    data.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(|| Error(format!("offset {at:#x} past the end")))
}

impl Archive {
    pub fn parse(bytes: &[u8]) -> Result<Archive, Error> {
        let size = word(bytes, 0)? as usize;
        let data_size = word(bytes, 4)? as usize;
        let pointer_count = word(bytes, 8)? as usize;
        let label_count = word(bytes, 12)? as usize;
        if size != bytes.len() {
            return Err(Error(format!("header says {size} bytes, file has {}", bytes.len())));
        }
        let pointers_at = HEADER + data_size;
        let labels_at = pointers_at + 4 * pointer_count;
        let names_at = labels_at + 8 * label_count;
        if names_at > bytes.len() {
            return Err(Error("tables past the end".into()));
        }
        let pointers = (0..pointer_count).map(|i| word(bytes, pointers_at + 4 * i)).collect::<Result<_, _>>()?;
        let labels = (0..label_count).map(|i| Ok((word(bytes, labels_at + 8 * i)?, word(bytes, labels_at + 8 * i + 4)?))).collect::<Result<_, Error>>()?;
        let reserved = [word(bytes, 16)?, word(bytes, 20)?, word(bytes, 24)?, word(bytes, 28)?];
        Ok(Archive { data: bytes[HEADER..pointers_at].to_vec(), pointers, labels, names: bytes[names_at..].to_vec(), reserved })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let size = HEADER + self.data.len() + 4 * self.pointers.len() + 8 * self.labels.len() + self.names.len();
        for w in [size as u32, self.data.len() as u32, self.pointers.len() as u32, self.labels.len() as u32] {
            out.extend(w.to_le_bytes());
        }
        for w in self.reserved {
            out.extend(w.to_le_bytes());
        }
        out.extend(&self.data);
        for p in &self.pointers {
            out.extend(p.to_le_bytes());
        }
        for (text, name) in &self.labels {
            out.extend(text.to_le_bytes());
            out.extend(name.to_le_bytes());
        }
        out.extend(&self.names);
        out
    }

    pub fn label_name(&self, index: usize) -> Option<&[u8]> {
        let at = self.labels.get(index)?.1 as usize;
        let rest = self.names.get(at..)?;
        Some(&rest[..rest.iter().position(|&b| b == 0)?])
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        (0..self.labels.len()).find(|&i| self.label_name(i) == Some(name))
    }

    pub fn text(&self, index: usize) -> Option<String> {
        let at = self.labels.get(index)?.0 as usize;
        let units: Vec<u16> = self.data.get(at..)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
        String::from_utf16(&units).ok()
    }

    /// Adds a message, or replaces the text of one with that name. The text goes at the end of the
    /// data, 4-byte aligned like the game's own; existing texts keep their offsets.
    pub fn set(&mut self, name: &[u8], text: &str) {
        while self.data.len() % 4 != 0 {
            self.data.push(0);
        }
        let at = self.data.len() as u32;
        for unit in text.encode_utf16() {
            self.data.extend(unit.to_le_bytes());
        }
        self.data.extend([0, 0]);
        while self.data.len() % 4 != 0 {
            self.data.push(0);
        }
        if let Some(i) = self.find(name) {
            self.labels[i].0 = at;
            return;
        }
        let name_at = self.names.len() as u32;
        self.names.extend(name);
        self.names.push(0);
        self.labels.push((at, name_at));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut a = Archive { data: b"MESS_ARCHIVE_T\0\0".to_vec(), pointers: vec![], labels: vec![], names: vec![], reserved: [0; 4] };
        a.set(b"MID_A", "Begin this chapter?");
        a.write()
    }

    #[test]
    fn an_archive_reads_and_writes_back_byte_for_byte() {
        let data = sample();
        assert_eq!(Archive::parse(&data).unwrap().write(), data);
    }

    #[test]
    fn an_added_message_is_found_and_leaves_the_others_alone() {
        let mut a = Archive::parse(&sample()).unwrap();
        a.set(b"MID_B", "Naga's Tear\nobtained!");
        let again = Archive::parse(&a.write()).unwrap();
        assert_eq!(again.text(again.find(b"MID_A").unwrap()).unwrap(), "Begin this chapter?");
        assert_eq!(again.text(again.find(b"MID_B").unwrap()).unwrap(), "Naga's Tear\nobtained!");
        assert_eq!(again.labels.len(), 2);
    }
}
