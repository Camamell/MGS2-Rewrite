//! `.gcx` compiled GCL scripts (file type 6, letter `g`).
//!
//! Loader `FUN_00152dc8`; interpreter `FUN_00153010` (blocks), `FUN_00152b88`
//! (commands), `FUN_00152ca0` (proc calls), `FUN_001522e0` (values),
//! `FUN_00154080` / `FUN_00153f40` (expressions), `FUN_00153738` (variables).
//!
//! ```text
//! +0  u32 timestamp
//! +4  u32 size of the proc section (from +8 to the main script)
//! +8  { u32 id; u32 offset } procs..., { 0, 0 }   bodies follow the table
//!     u32 main size; main block
//!     (16-byte aligned) resources:
//!       +4 -> string offset table, +8 -> string data, +C -> ?
//! ```
//!
//! Everything is little-endian. A block is `0x80, u32 len` (len counted from
//! the byte after 0x80) followed by statements up to a 0x00:
//! - `0x60, u16 size, u24 command, u16 options_offset, args..., options...`
//!   (`options_offset` is relative to its own position)
//! - `0x70, u16 size, u24 proc id, args..., 0x00`
//!
//! Names (commands, procs, actors) are `str_code` hashes.

use crate::{Result, format_err};

#[derive(Clone, Debug)]
pub struct Gcx {
    pub timestamp: u32,
    pub procs: Vec<Proc>,
    pub main: Vec<Stmt>,
    pub strings: Vec<String>,
    /// The same resources as raw bytes, from each entry's start to the next
    /// entry's (data blocks such as the enemy manager's `-v` table are
    /// little-endian words, read through `FUN_001529f0`).
    pub resources: Vec<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct Proc {
    pub id: u32,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Command { name: u32, args: Vec<Value>, options: Vec<(u8, Vec<Value>)> },
    Call { proc: u32, args: Vec<Value> },
}

#[derive(Clone, Debug)]
pub enum Value {
    /// Integer with its GCL type code (1 short, 2 byte, 3 char, 4 bool,
    /// 9/10/13 32-bit).
    Int(u8, i32),
    /// `str_code` name (type 6).
    Name(u32),
    /// Proc id (type 8).
    ProcRef(u32),
    Str(String),
    /// Index into the resource string table (type 0x0E).
    StrRef(u16),
    Var(Var),
    /// Reverse-Polish expression.
    Expr(Vec<ExprItem>),
    /// `$n` argument of the enclosing proc (type 0x40); 0 = last result.
    Arg(u8),
    Block(Vec<Stmt>),
}

#[derive(Clone, Debug)]
pub struct Var {
    /// Type: 1 short, 2/3 byte, 4 bit, 6/9 u32, 8 u24.
    pub ty: u8,
    /// True for the bank at 0x191910 (bit 23 set), else 0x191918.
    pub bank_b: bool,
    pub offset: u16,
    /// Bit number for bit variables.
    pub bit: u8,
    /// Array index for indexed variables (`0x2x` codes).
    pub index: Option<Box<Value>>,
}

#[derive(Clone, Debug)]
pub enum ExprItem {
    Push(Value),
    Op(Op),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Neg, Not, BitNot, Add, Sub, Mul, Div, Mod, Shl, Shr,
    Eq, Ne, Lt, Le, Gt, Ge, BitOr, BitAnd, BitXor, Or, And, Assign, Second,
    Unknown(u8),
}

impl Op {
    fn from(b: u8) -> Self {
        use Op::*;
        match b {
            1 => Neg, 2 => Not, 3 => BitNot, 4 => Add, 5 => Sub, 6 => Mul, 7 => Div,
            8 => Mod, 9 => Shl, 10 => Shr, 0xB => Eq, 0xC => Ne, 0xD => Lt, 0xE => Le,
            0xF => Gt, 0x10 => Ge, 0x11 => BitOr, 0x12 => BitAnd, 0x13 => BitXor,
            0x14 => Or, 0x15 => And, 0x16 => Assign, 0x17 => Second, x => Unknown(x),
        }
    }

    pub fn symbol(self) -> &'static str {
        use Op::*;
        match self {
            Neg => "neg", Not => "!", BitNot => "~", Add => "+", Sub => "-", Mul => "*",
            Div => "/", Mod => "%", Shl => "<<", Shr => ">>", Eq => "==", Ne => "!=",
            Lt => "<", Le => "<=", Gt => ">", Ge => ">=", BitOr => "|", BitAnd => "&",
            BitXor => "^", Or => "||", And => "&&", Assign => "=", Second => ",",
            Unknown(_) => "?",
        }
    }

    /// Neg, Not and BitNot use only their right operand; like every operator
    /// they still pop two (the compiler emits `0 x !`).
    pub fn is_unary(self) -> bool {
        matches!(self, Op::Neg | Op::Not | Op::BitNot)
    }
}

struct Cur<'a> {
    d: &'a [u8],
}

impl Cur<'_> {
    fn b(&self, o: usize) -> Result<u8> {
        self.d.get(o).copied().ok_or_else(|| crate::Error::Format(format!("gcx: read past end at {o:#x}")))
    }
    fn u16(&self, o: usize) -> Result<u16> {
        Ok(u16::from(self.b(o)?) | u16::from(self.b(o + 1)?) << 8)
    }
    fn u24(&self, o: usize) -> Result<u32> {
        Ok(u32::from(self.b(o)?) | u32::from(self.b(o + 1)?) << 8 | u32::from(self.b(o + 2)?) << 16)
    }
    fn u32(&self, o: usize) -> Result<u32> {
        Ok(self.u24(o)? | u32::from(self.b(o + 3)?) << 24)
    }

    /// Parses a block at `o` (which must hold 0x80).
    fn block(&self, o: usize) -> Result<Vec<Stmt>> {
        if self.b(o)? != 0x80 {
            return format_err(format!("gcx: expected block at {o:#x}"));
        }
        let end = o + 1 + self.u32(o + 1)? as usize;
        self.stmts(o + 5, end)
    }

    fn stmts(&self, mut o: usize, end: usize) -> Result<Vec<Stmt>> {
        let mut out = Vec::new();
        while o < end {
            let op = self.b(o)?;
            if op == 0 {
                break;
            }
            let size = self.u16(o + 1)? as usize;
            let next = o + 1 + size;
            match op {
                0x60 => {
                    let name = self.u24(o + 3)?;
                    let opt_at = o + 6 + self.u16(o + 6)? as usize;
                    let args = self.values(o + 8, opt_at.min(next))?;
                    let mut options = Vec::new();
                    let mut p = opt_at;
                    while p < next && self.b(p)? == 0x50 {
                        let letter = self.b(p + 1)?;
                        let len = self.u16(p + 2)? as usize;
                        options.push((letter, self.values(p + 4, p + 2 + len)?));
                        p += 2 + len;
                    }
                    out.push(Stmt::Command { name, args, options });
                }
                0x70 => {
                    out.push(Stmt::Call { proc: self.u24(o + 3)?, args: self.values(o + 6, next)? });
                }
                _ => return format_err(format!("gcx: unknown statement {op:#x} at {o:#x}")),
            }
            o = next;
        }
        Ok(out)
    }

    fn values(&self, mut o: usize, end: usize) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        while o < end {
            if self.b(o)? == 0 {
                break;
            }
            let (v, next) = self.value(o)?;
            out.push(v);
            o = next;
        }
        Ok(out)
    }

    fn value(&self, o: usize) -> Result<(Value, usize)> {
        let t = self.b(o)?;
        if matches!(t & 0xF0, 0x10 | 0x20) {
            return self.var(o);
        }
        Ok(match t {
            1 => (Value::Int(t, i32::from(self.u16(o + 1)? as i16)), o + 3),
            2..=4 => (Value::Int(t, i32::from(self.b(o + 1)?)), o + 2),
            6 => (Value::Name(self.u24(o + 1)?), o + 4),
            8 => (Value::ProcRef(self.u24(o + 1)?), o + 4),
            7 => {
                let len = self.b(o + 1)? as usize;
                let s = self.d.get(o + 2..o + 2 + len).unwrap_or_default();
                let s = s.split(|&c| c == 0).next().unwrap_or_default();
                (Value::Str(String::from_utf8_lossy(s).into_owned()), o + 2 + len)
            }
            9 | 10 | 13 => (Value::Int(t, self.u32(o + 1)? as i32), o + 5),
            0xE => (Value::StrRef(self.u16(o + 1)?), o + 3),
            0x30 => {
                let len = self.b(o + 1)? as usize;
                (Value::Expr(self.expr(o + 2)?), o + 1 + len)
            }
            0x40 => (Value::Arg(self.b(o + 1)?), o + 2),
            0x80 => {
                let len = self.u32(o + 1)? as usize;
                (Value::Block(self.stmts(o + 5, o + 1 + len)?), o + 1 + len)
            }
            _ => return format_err(format!("gcx: unknown value type {t:#x} at {o:#x}")),
        })
    }

    fn var(&self, o: usize) -> Result<(Value, usize)> {
        let w = self.u32(o)?.swap_bytes(); // stored big-endian
        let indexed = self.b(o)? & 0xF0 == 0x20;
        let (index, next) = if indexed {
            let (v, n) = self.value(o + 6)?;
            (Some(Box::new(v)), n)
        } else {
            (None, o + 4)
        };
        Ok((
            Value::Var(Var {
                ty: ((w >> 24) & 0xF) as u8,
                bank_b: (w >> 16) & 0xF0 == 0x80,
                offset: (w & 0xFFFF) as u16,
                bit: ((w >> 16) & 0xF) as u8,
                index,
            }),
            next,
        ))
    }

    fn expr(&self, mut o: usize) -> Result<Vec<ExprItem>> {
        let mut out = Vec::new();
        loop {
            if self.b(o)? == 0x31 {
                let op = self.b(o + 1)?;
                if op == 0 {
                    return Ok(out);
                }
                out.push(ExprItem::Op(Op::from(op)));
                o += 2;
            } else {
                let (v, next) = self.value(o)?;
                out.push(ExprItem::Push(v));
                o = next;
            }
        }
    }
}

pub fn parse(data: &[u8]) -> Result<Gcx> {
    let c = Cur { d: data };
    let timestamp = c.u32(0)?;
    let proc_size = c.u32(4)? as usize;
    let mut table = Vec::new();
    let mut o = 8;
    loop {
        let (id, off) = (c.u32(o)?, c.u32(o + 4)? as usize);
        o += 8;
        if id == 0 {
            break;
        }
        table.push((id, off));
    }
    let code = o;
    let procs = table
        .into_iter()
        .map(|(id, off)| Ok(Proc { id, body: c.block(code + off)? }))
        .collect::<Result<Vec<_>>>()?;

    let main_at = 8 + proc_size;
    let main_size = c.u32(main_at)? as usize;
    let main = c.block(main_at + 4)?;

    let res = (main_at + 4 + main_size + 15) & !15;
    let mut strings = Vec::new();
    let mut resources = Vec::new();
    if res + 12 <= data.len() {
        let (idx, base) = (res + c.u32(res + 4)? as usize, res + c.u32(res + 8)? as usize);
        let mut starts = Vec::new();
        let mut i = idx;
        while i + 4 <= base && i + 4 <= data.len() {
            starts.push(base + (c.u32(i)? & 0x7FFF_FFFF) as usize);
            i += 4;
        }
        let mut sorted = starts.clone();
        sorted.sort_unstable();
        for &s in &starts {
            let end = sorted.iter().copied().find(|&e| e > s).unwrap_or(data.len()).min(data.len());
            resources.push(data.get(s..end).unwrap_or_default().to_vec());
        }
        let mut i = idx;
        while i + 4 <= base && i + 4 <= data.len() {
            let s = base + (c.u32(i)? & 0x7FFF_FFFF) as usize;
            let bytes = data.get(s..).unwrap_or_default();
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            strings.push(String::from_utf8_lossy(&bytes[..end]).into_owned());
            i += 4;
        }
    }
    Ok(Gcx { timestamp, procs, main, strings, resources })
}
