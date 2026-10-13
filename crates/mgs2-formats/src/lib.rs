//! Readers for Metal Gear Solid 2: Sons of Liberty (PS2, NTSC-U, SLUS-20144) data.
//!
//! Every format here was reverse engineered from the retail executable; the
//! `re/notes` directory records which game function each piece came from.

pub mod cipher;
pub mod demo;
pub mod evm;
pub mod face;
pub mod gcx;
pub mod gs;
pub mod hzx;
pub mod iso;
pub mod kms;
pub mod lt2;
pub mod mar;
pub mod names;
pub mod sar;
pub mod sd;
pub mod stage_dat;
pub mod tri;
pub mod vox;

pub use cipher::{Cipher, str_code};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Format(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn format_err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Format(msg.into()))
}
