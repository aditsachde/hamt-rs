#![allow(warnings)]
mod hamt;
mod cidstore;
mod nodestate;

use bitvec::prelude::*;
use hamt::HashMap;
use libipld::cbor::encode::write_u64;
use libipld::codec::Encode;
use std::collections::HashMap as StdHashMap;
use cidstore::{CidStore, MemStore};
use multihash::{Code, MultihashDigest};
use std::{convert::TryFrom, io::Read, fmt};
use libipld::cbor::{DagCbor, DagCborCodec};
use libipld::DagCbor;
use libipld::{
    cid::Cid,
    codec::References,
    codec::{assert_roundtrip, Codec, Decode},
    ipld::Ipld,
    raw_value::{IgnoredAny, RawValue, SkipOne},
};
use serde_json::{Result, Value};
use serde::{Serialize, Deserialize};
use std::fs;

use crate::hamt::{HashMapNode, HashMapOptions, Node};
use crate::nodestate::Complete;

pub type AliceWords = StdHashMap<String, Vec<Element>>;

#[derive(Serialize, Deserialize, Debug)]
pub struct Element {
    line: u64,
    column: u64,
}

impl Encode<DagCborCodec> for Element {
    fn encode<W: std::io::Write>(&self, c: DagCborCodec, w: &mut W) -> anyhow::Result<()> {
        write_u64(w, 5, 2 as u64)?;
        "line".encode(c, w)?;
        self.line.encode(c, w)?;
        "column".encode(c, w)?;
        self.column.encode(c, w)?;
        Ok(())
    }
}

impl Decode<DagCborCodec> for Element {
    fn decode<R: Read + std::io::Seek>(c: DagCborCodec, r: &mut R) -> anyhow::Result<Self> {
        todo!()
    }
}


fn main() {
    let words: AliceWords = serde_json::from_str(&fs::read_to_string("./alicewords.json").unwrap()).unwrap();

    let store = MemStore::new();
    let mut map: HashMap<Complete, MemStore, Vec<Element>> = HashMap {
        options: HashMapOptions {
            hash_alg: Code::Sha2_256,
            bucket_size: 3,
            bit_width: 5,
        },
        root: Box::new(HashMapNode::Node(Node {
            map: bitvec![Lsb0, u8; 0; (2usize.pow(5))],
            data: vec![],
        })),
        state: Complete,
        store: store,
    };
    for (key, value) in words {
        let key = key.into_bytes();
        map.insert(key, value).unwrap();
    }
    println!("{}", map.cid().unwrap());
}

