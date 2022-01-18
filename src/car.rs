use anyhow::Result;
use cid::Cid;

use std::io::{self, Write};
use unsigned_varint::encode::{usize, usize_buffer};

const EMPTY_CAR_HEADER: &[u8] = include_bytes!("empty.car");

pub struct Car {
    header: &'static [u8],
    file: Box<dyn Write>,
}

impl Car {
    pub fn new(file: Box<dyn Write>) -> Self {
        Car {
            header: EMPTY_CAR_HEADER,
            file,
        }
    }

    pub fn encode_header(&mut self) -> io::Result<()> {
        self.file.write_all(self.header)
    }

    pub fn write_block_cid(&mut self, cid: &Cid, block: &[u8]) -> Result<()> {
        self.write_block(&cid.to_bytes(), block)
    }

    pub fn write_block(&mut self, cid: &[u8], block: &[u8]) -> Result<()> {
        self.file
            .write_all(usize(cid.len() + block.len(), &mut usize_buffer()))?;
        self.file.write_all(cid)?;
        self.file.write_all(block)?;
        Ok(())
    }
}
