//! Fire Emblem Awakening's compiled event scripts (`scripts/*.cmb`), as far as Naga's Tear needs
//! them: read a script into its functions and strings, change or add functions, and write it back.
//!
//! Layout (USA rev0, version 0x20110819), worked out from the game's own files:
//! - header, 0x28 bytes: "cmb\0", version, 0, 0x28 (where the file name is), 0 x3,
//!   +0x1C the function table, +0x20 the string table, 0; then the file name.
//! - the function table: one u32 file offset per function, then 0.
//! - each function: a 24-byte header { +0 its own offset, +4 where its code starts,
//!   +8 type | argument count << 8 | frame size << 16, +0xC its index, +0x10 where its name is
//!   (0 for an unnamed event), +0x14 0 for a named function or, for some events, an offset },
//!   then the name (Shift-JIS, NUL), sometimes event parameters, then the bytecode. The code runs to
//!   the next function, or to the string table for the last one.
//! - the string table: NUL-terminated Shift-JIS strings. Code names them by offset from its start,
//!   so strings added at its end leave every existing reference alone.
//!
//! Bytecode used here (operands big-endian): 0x1D u16 push string, 0x47 u16 u8 call the function
//! named by the string with that many arguments (pushes its result), 0x21 drop the top value,
//! 0x4C i16 jump when the popped value is 0 (from the jump opcode + 1), 0x54 return.

use std::fmt;

pub const MAGIC: &[u8; 4] = b"cmb\0";
const HEADER_SIZE: usize = 0x28;
const FUNCTION_HEADER: usize = 24;

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    /// type | argc << 8 | frame << 16
    pub kind: u32,
    pub index: u32,
    pub name: Option<Vec<u8>>,
    /// +0x14 as stored: 0, or for unnamed events an offset (rebased on write)
    pub extra_offset: u32,
    /// bytes between the header (and name) and the code: event parameters, kept as they are
    pub params: Vec<u8>,
    /// the bytecode, with the padding before the next function
    pub code: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Script {
    /// the first 0x28 bytes plus the file name and its padding, up to the function table
    pub head: Vec<u8>,
    pub functions: Vec<Function>,
    /// bytes between the end of the function table and the first function (normally none)
    pub gap: Vec<u8>,
    pub strings: Vec<u8>,
}

#[derive(Debug)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}
fn err<T>(message: impl Into<String>) -> Result<T, Error> {
    Err(Error(message.into()))
}

fn u32_at(data: &[u8], at: usize) -> Result<u32, Error> {
    data.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(|| Error(format!("offset {at:#x} past the end")))
}

impl Script {
    pub fn parse(data: &[u8]) -> Result<Script, Error> {
        if data.len() < HEADER_SIZE || &data[..4] != MAGIC {
            return err("not a cmb script");
        }
        let table = u32_at(data, 0x1C)? as usize;
        let strings = u32_at(data, 0x20)? as usize;
        if table < HEADER_SIZE || table > data.len() || strings > data.len() {
            return err("function or string table out of range");
        }
        let mut offsets = Vec::new();
        let mut at = table;
        loop {
            let offset = u32_at(data, at)? as usize;
            at += 4;
            if offset == 0 {
                break;
            }
            offsets.push(offset);
        }
        let first = offsets.first().copied().unwrap_or(strings);
        if first < at || offsets.windows(2).any(|w| w[0] >= w[1]) || offsets.last().is_some_and(|&o| o >= strings) {
            return err("functions out of order");
        }
        let mut functions = Vec::new();
        for (i, &offset) in offsets.iter().enumerate() {
            let end = offsets.get(i + 1).copied().unwrap_or(strings);
            let h: Vec<u32> = (0..6).map(|k| u32_at(data, offset + 4 * k)).collect::<Result<_, _>>()?;
            if h[0] as usize != offset {
                return err(format!("function at {offset:#x} names {:#x} as its own offset", h[0]));
            }
            let code = h[1] as usize;
            let mut cursor = offset + FUNCTION_HEADER;
            let name = if h[4] != 0 {
                if h[4] as usize != cursor {
                    return err(format!("function at {offset:#x}: name not after the header"));
                }
                let nul = data[cursor..end].iter().position(|&b| b == 0).ok_or_else(|| Error("unterminated name".into()))?;
                let name = data[cursor..cursor + nul].to_vec();
                cursor += nul + 1;
                Some(name)
            } else {
                None
            };
            if code < cursor || code > end {
                return err(format!("function at {offset:#x}: code at {code:#x} outside it"));
            }
            if h[5] != 0 && (h[5] as usize) < offset || h[5] as usize > end {
                return err(format!("function at {offset:#x}: +0x14 {:#x} outside it", h[5]));
            }
            functions.push(Function { kind: h[2], index: h[3], name, extra_offset: h[5], params: data[cursor..code].to_vec(), code: data[code..end].to_vec() });
        }
        Ok(Script { head: data[..table].to_vec(), functions, gap: data[at..first].to_vec(), strings: data[strings..].to_vec() })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut out = self.head.clone();
        let table = out.len();
        out.extend(std::iter::repeat(0).take(4 * (self.functions.len() + 1)));
        out.extend(&self.gap);
        let mut offsets = Vec::new();
        for f in &self.functions {
            let offset = out.len();
            offsets.push(offset as u32);
            let mut cursor = offset + FUNCTION_HEADER;
            let name_at = f.name.as_ref().map(|n| {
                let at = cursor;
                cursor += n.len() + 1;
                at as u32
            });
            let params_at = cursor;
            let code_at = params_at + f.params.len();
            let extra = if f.extra_offset == 0 { 0 } else { f.extra_offset }; // rebased below
            let header = [offset as u32, code_at as u32, f.kind, f.index, name_at.unwrap_or(0), extra];
            for word in header {
                out.extend(word.to_le_bytes());
            }
            if let Some(name) = &f.name {
                out.extend(name);
                out.push(0);
            }
            out.extend(&f.params);
            out.extend(&f.code);
        }
        let strings = out.len() as u32;
        out.extend(&self.strings);
        for (i, offset) in offsets.iter().enumerate() {
            out[table + 4 * i..table + 4 * i + 4].copy_from_slice(&offset.to_le_bytes());
        }
        out[0x1C..0x20].copy_from_slice(&(table as u32).to_le_bytes());
        out[0x20..0x24].copy_from_slice(&strings.to_le_bytes());
        out
    }

    /// Rebases each function's +0x14 offset from the layout it was read from to the one `write`
    /// gives it, so that it keeps pointing at the same byte of its function.
    pub fn write_rebased(&self, original: &[u8]) -> Result<Vec<u8>, Error> {
        let old = Script::offsets(original)?;
        let mut out = self.write();
        let new = Script::offsets(&out)?;
        for (i, f) in self.functions.iter().enumerate() {
            if f.extra_offset == 0 {
                continue;
            }
            let Some(&old_at) = old.get(i) else { return err("a new function has a +0x14 offset") };
            let moved = new[i] as i64 - old_at as i64;
            let at = new[i] as usize + 0x14;
            let value = (f.extra_offset as i64 + moved) as u32;
            out[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        Ok(out)
    }

    fn offsets(data: &[u8]) -> Result<Vec<u32>, Error> {
        let mut at = u32_at(data, 0x1C)? as usize;
        let mut offsets = Vec::new();
        loop {
            let offset = u32_at(data, at)?;
            at += 4;
            if offset == 0 {
                return Ok(offsets);
            }
            offsets.push(offset);
        }
    }

    /// The offset of the string in the string table, added at its end when it is not there yet.
    pub fn string(&mut self, text: &[u8]) -> u16 {
        let mut at = 0;
        while at < self.strings.len() {
            let end = at + self.strings[at..].iter().position(|&b| b == 0).unwrap_or(self.strings.len() - at);
            if &self.strings[at..end] == text {
                return at as u16;
            }
            at = end + 1;
        }
        let at = self.strings.len();
        self.strings.extend(text);
        self.strings.push(0);
        assert!(at <= u16::MAX as usize, "string table too large for a u16 reference");
        at as u16
    }

    #[allow(dead_code)]
    pub fn string_at(&self, offset: usize) -> Option<&[u8]> {
        let rest = self.strings.get(offset..)?;
        Some(&rest[..rest.iter().position(|&b| b == 0)?])
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        self.functions.iter().position(|f| f.name.as_deref() == Some(name))
    }
}

/// Builds bytecode against a script's string table.
pub struct Code<'a> {
    pub script: &'a mut Script,
    pub bytes: Vec<u8>,
}

impl<'a> Code<'a> {
    pub fn new(script: &'a mut Script) -> Self {
        Code { script, bytes: Vec::new() }
    }
    pub fn push_string(&mut self, text: &[u8]) -> &mut Self {
        let at = self.script.string(text);
        self.bytes.push(0x1D);
        self.bytes.extend(at.to_be_bytes());
        self
    }
    /// calls the function by name; its result is left on the stack
    pub fn call(&mut self, name: &[u8], args: u8) -> &mut Self {
        let at = self.script.string(name);
        self.bytes.push(0x47);
        self.bytes.extend(at.to_be_bytes());
        self.bytes.push(args);
        self
    }
    pub fn drop(&mut self) -> &mut Self {
        self.bytes.push(0x21);
        self
    }
    /// calls and drops the result
    pub fn run(&mut self, name: &[u8], args: u8) -> &mut Self {
        self.call(name, args).drop()
    }
    /// returns when the popped value is not 0
    pub fn return_if_set(&mut self) -> &mut Self {
        self.bytes.extend([0x4C, 0x00, 0x03, 0x54]);
        self
    }
    pub fn ret(&mut self) -> &mut Self {
        self.bytes.push(0x54);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        // a two-function script like bev_shared.cmb: "f" (code 47 00 00 00 21 54) and an unnamed event
        let mut s = Script {
            head: {
                let mut h = vec![0u8; HEADER_SIZE];
                h[..4].copy_from_slice(MAGIC);
                h[4..8].copy_from_slice(&0x2011_0819u32.to_le_bytes());
                h[0xC..0x10].copy_from_slice(&0x28u32.to_le_bytes());
                h.extend(b"t.cmb\0\0\0");
                h
            },
            functions: vec![
                Function { kind: 0, index: 0, name: Some(b"f".to_vec()), extra_offset: 0, params: vec![], code: vec![0x47, 0, 0, 0, 0x21, 0x54, 0, 0] },
                Function { kind: 5, index: 1, name: None, extra_offset: 0, params: vec![], code: vec![0x54] },
            ],
            gap: vec![],
            strings: b"ev::X\0".to_vec(),
        };
        let first = s.write();
        // make the event's +0x14 point at its code, as Command.cmb's does
        let event = Script::offsets(&first).unwrap()[1] as usize;
        s.functions[1].extra_offset = (event + FUNCTION_HEADER) as u32;
        s.write()
    }

    #[test]
    fn a_script_reads_and_writes_back_byte_for_byte() {
        let data = sample();
        let script = Script::parse(&data).unwrap();
        assert_eq!(script.write_rebased(&data).unwrap(), data);
    }

    #[test]
    fn an_added_function_moves_the_others_and_keeps_their_offsets_pointing_inside_them() {
        let data = sample();
        let mut script = Script::parse(&data).unwrap();
        let mut code = Code::new(&mut script);
        code.push_string(b"MID").run(b"ev::X", 1).ret();
        let bytes = code.bytes;
        script.functions.push(Function { kind: 0, index: 2, name: Some(b"g".to_vec()), extra_offset: 0, params: vec![], code: bytes });
        let out = script.write_rebased(&data).unwrap();
        let again = Script::parse(&out).unwrap();
        assert_eq!(again.functions.len(), 3);
        assert_eq!(again.functions[2].code, vec![0x1D, 0, 6, 0x47, 0, 0, 1, 0x21, 0x54]);
        assert_eq!(again.string_at(6), Some(&b"MID"[..]));
        let offsets = Script::offsets(&out).unwrap();
        assert_eq!(again.functions[1].extra_offset, offsets[1] + FUNCTION_HEADER as u32);
        assert_eq!(again.functions[0].code, Script::parse(&data).unwrap().functions[0].code);
    }

    #[test]
    fn a_known_string_is_reused() {
        let mut script = Script::parse(&sample()).unwrap();
        assert_eq!(script.string(b"ev::X"), 0);
        assert_eq!(script.string(b"new"), 6);
        assert_eq!(script.string(b"new"), 6);
    }
}
