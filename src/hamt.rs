#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]

use core::panic;
use std::cmp::Ordering;
//use std::backtrace::Backtrace;
use std::convert::{TryFrom, TryInto};
use std::fmt::Debug;
use std::ops::Deref;

use crate::cidstore::{CidStore, MemStore};
use crate::nodestate::{Complete, Edit, HashMapState};
use bitvec::macros::internal::funty::IsInteger;
use bitvec::prelude::*;
use bitvec::ptr::BitSpanError;
use cid::{Cid, CidGeneric};
use libipld::cbor::{encode, DagCborCodec};
use libipld::codec::{Codec, Decode, Encode};
use libipld::DagCbor;
use multihash::{Code, MultihashDigest};
use serde::Serialize;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum HashMapError {
    #[error("Index was out of bounds of vector")]
    OutOfBounds,

    #[error("Hashing conversion failed")]
    HashingError {
        #[from]
        source: BitSpanError<u8>,
        //backtrace: Backtrace
    },

    #[error("CID conversion failed")]
    CidConversionFailure,
}

#[derive(Debug)]
pub struct HashMap<S, T, V>
where
    S: HashMapState,
    T: CidStore,
    V: Serialize + Debug + Encode<DagCborCodec> + Decode<DagCborCodec>,
{
    pub options: HashMapOptions,
    pub root: Box<HashMapNode<V>>,
    pub state: S,
    pub store: T,
}

#[derive(Debug)]
pub struct HashMapOptions {
    //hash_alg_raw: usize,
    pub hash_alg: Code,
    pub bucket_size: u64,
    pub bit_width: u64,
}

#[derive(Debug)]
pub enum HashMapNode<V: Serialize + Debug + Encode<DagCborCodec> + Decode<DagCborCodec>> {
    Cid(Cid),
    Node(Node<V>),
}

#[derive(Debug)]
pub struct Node<V: Serialize + Debug + Encode<DagCborCodec> + Decode<DagCborCodec>> {
    pub map: BitVec<Lsb0, u8>,
    pub data: Vec<Element<V>>,
}

#[derive(Debug)]
pub enum Element<V: Serialize + Debug + Encode<DagCborCodec> + Decode<DagCborCodec>> {
    HashMapNode(HashMapNode<V>),
    Bucket(Vec<BucketEntry<V>>),
}

#[derive(Debug)]
struct BucketEntry<V: Serialize + Debug + Encode<DagCborCodec> + Decode<DagCborCodec>> {
    key: Vec<u8>,
    value: V,
}

impl<
        S: HashMapState,
        T: CidStore,
        V: Serialize + Debug + Encode<DagCborCodec> + Decode<DagCborCodec>,
    > Encode<DagCborCodec> for HashMap<S, T, V>
{
    fn encode<W: std::io::Write>(&self, c: DagCborCodec, w: &mut W) -> anyhow::Result<()> {
        encode::write_u64(w, 5, 3 as u64)?;
        "hamt".encode(c, w)?;
        self.root.encode(c, w)?;
        "hashAlg".encode(c, w)?;
        let code: u64 = self.options.hash_alg.into();
        code.encode(c, w)?;
        "bucketSize".encode(c, w)?;
        self.options.bucket_size.encode(c, w)?;
        Ok(())
    }
}

impl<V: Serialize + Debug + Encode<DagCborCodec> + Decode<DagCborCodec>> Encode<DagCborCodec>
    for HashMapNode<V>
{
    fn encode<W: std::io::Write>(&self, c: DagCborCodec, w: &mut W) -> anyhow::Result<()> {
        match self {
            HashMapNode::Cid(cid) => {
                cid.encode(c, w);
            }
            HashMapNode::Node(node) => {
                // The block is an array with 2 elements
                encode::write_u64(w, 4, 2)?;

                // The first element is the map, encoded as a set of bytes
                node.map.as_raw_slice()[0..(node.map.len() / 8)].encode(c, w)?;

                // The second element is another array, specifically the bucket
                // Write array prefix with len of the bucket
                encode::write_u64(w, 4, node.data.len().try_into().unwrap())?;
                for element in &node.data {
                    match element {
                        Element::HashMapNode(node) => match node {
                            HashMapNode::Cid(cid) => {
                                cid.encode(c, w)?;
                            }
                            HashMapNode::Node(_) => {
                                anyhow::bail!("Cannot serialize uncollapsed HashMapNode")
                            }
                        },
                        Element::Bucket(bucket) => {
                            encode::write_u64(w, 4, bucket.len().try_into().unwrap())?;
                            for entry in bucket {
                                encode::write_u64(w, 4, 2)?;
                                entry.key.as_slice().encode(c, w)?;
                                entry.value.encode(c, w)?;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

impl<V: Serialize + Debug + Encode<DagCborCodec> + Decode<DagCborCodec>> HashMapNode<V> {
    pub fn get(
        &self,
        depth: u64,
        digest: BitVec<Msb0, u8>,
        key: Vec<u8>,
        opts: &HashMapOptions,
    ) -> Result<Option<&V>, HashMapError> {
        let offset = depth * opts.bit_width;
        //println!("{} {} {}", offset, offset + opts.bit_width, key.len());
        let index = &digest[offset as usize..(offset + opts.bit_width) as usize];
        //println!("{:?}", index);
        let index = to_int(index);
        //println!("reached 2 {}", index);

        let node = match self {
            HashMapNode::Cid(_) => todo!(),
            HashMapNode::Node(node) => node,
        };

        let data_index = node.map[0..index].iter_ones().count();
        //println!("reached 4");

        //println!("{} {} {}", offset, index, data_index);
        //println!("{:?}", node.map);

        let value_at_index = *(node.map.get(index).ok_or(HashMapError::OutOfBounds)?);

        match value_at_index {
            false => Ok(None),
            true => {
                let data_item = node.data.get(data_index).ok_or(HashMapError::OutOfBounds)?;
                match data_item {
                    Element::HashMapNode(node) => node.get(depth + 1, digest, key, opts),
                    Element::Bucket(bucket) => {
                        for entry in bucket {
                            if entry.key == key {
                                return Ok(Some(&entry.value));
                            }
                        }
                        Ok(None)
                    }
                }
            }
        }
    }

    pub fn insert(
        &mut self,
        depth: u64,
        digest: BitVec<Msb0, u8>,
        key: Vec<u8>,
        value: V,
        opts: &HashMapOptions,
    ) -> Result<(), HashMapError> {
        if depth > 31 {
            panic!("Depth limit exceded");
        }
        let offset = depth * opts.bit_width;
        //println!("{} {} {}", offset, offset + opts.bit_width, key.len());
        let index = &digest[offset as usize..(offset + opts.bit_width) as usize];
        //println!("{:?}", index);
        let index = to_int(index);
        //println!("reached 2 {}", index);

        let node = match self {
            HashMapNode::Cid(_) => todo!(),
            HashMapNode::Node(node) => node,
        };

        //println!("reached 3 {}", node.map.len());

        let data_index = node.map[0..index].iter_ones().count();
        //println!("reached 4");

        //println!("{} {} {}", offset, index, data_index);
        //println!("{:?}", node.map);

        let value_at_index = *(node.map.get(index).ok_or(HashMapError::OutOfBounds)?);
        //println!("{}", value_at_index);

        match value_at_index {
            false => {
                node.data.insert(
                    data_index,
                    Element::Bucket(vec![BucketEntry { key, value: value }]),
                );
                node.map.set(index, true)
            }

            true => {
                let mut data_item = node
                    .data
                    .get_mut(data_index)
                    .ok_or(HashMapError::OutOfBounds)?;
                match data_item {
                    Element::HashMapNode(node) => {
                        node.insert(depth + 1, digest, key, value, opts)?;
                    }
                    Element::Bucket(bucket) => {
                        if bucket.len() < opts.bucket_size as usize {
                            match bucket.binary_search_by(|element| {
                                if element.key < key {
                                    Ordering::Less
                                } else if element.key == key {
                                    Ordering::Equal
                                } else {
                                    Ordering::Greater
                                }
                            }) {
                                Ok(pos) => {
                                    bucket[pos].value = value;
                                }
                                Err(pos) => bucket.insert(pos, BucketEntry { key, value }),
                            }
                            //Element::Bucket(bucket)
                        } else {
                            let mut new_node: HashMapNode<V> = HashMapNode::Node(Node {
                                data: vec![],
                                map: bitvec![Lsb0, u8; 0; (2.pow(5))],
                            });

                            let bucket_taken = std::mem::take(bucket);

                            for entry in bucket_taken {
                                let multihash = opts.hash_alg.digest(entry.key.as_slice());
                                let digest = bitvec::prelude::BitVec::<Msb0, _>::from_slice(
                                    multihash.digest(),
                                )?;
                                new_node.insert(depth + 1, digest, entry.key, entry.value, opts)?;
                            }

                            new_node.insert(depth + 1, digest, key, value, opts)?;

                            *data_item = Element::HashMapNode(new_node);
                            //Element::HashMapNode(new_node)
                        }
                    }
                };
            }
        }

        //todo!()
        Ok(())
        //todo!();
    }

    fn collapse(&mut self) -> Result<Cid, HashMapError> {
        if let HashMapNode::Cid(cid) = self {
            return Ok(*cid);
        };

        if let HashMapNode::Node(node) = self {
            for element in &mut node.data {
                if let Element::HashMapNode(nested_node) = element {
                    let temp = Element::<V>::HashMapNode(HashMapNode::Cid(nested_node.collapse()?));
                    std::mem::replace(element, temp);
                }
            }
        };

        let result = DagCborCodec
            .encode(self)
            .map_err(|_| HashMapError::CidConversionFailure {})?;
        let cid = Cid::new_v1(0x71, Code::Sha2_256.digest(&result));

        //println!("{:#?}", self);
        //println!("{:02x?}", result);
        //println!("{}", cid);
        //panic!();

        Ok(cid)
    }
}

impl<
        S: HashMapState,
        T: CidStore,
        V: Serialize + Debug + Encode<DagCborCodec> + Decode<DagCborCodec>,
    > HashMap<S, T, V>
{
    pub fn get(&self, key: Vec<u8>) -> Result<Option<&V>, HashMapError> {
        let multihash = self.options.hash_alg.digest(key.as_slice());
        let digest = bitvec::prelude::BitVec::<Msb0, _>::from_slice(multihash.digest())?;
        self.root.get(0, digest, key, &self.options)
    }

    pub fn insert(&mut self, key: Vec<u8>, value: V) -> Result<(), HashMapError> {
        let multihash = self.options.hash_alg.digest(key.as_slice());
        let digest = bitvec::prelude::BitVec::<Msb0, _>::from_slice(multihash.digest())?;
        self.root.insert(0, digest, key, value, &self.options)?;
        Ok(())
    }

    pub fn remove(&mut self, key: Vec<u8>) -> Option<V> {
        todo!()
    }

    pub fn collapse(&mut self) -> Result<(), HashMapError> {
        if let HashMapNode::Node(node) = &mut *self.root {
            for element in &mut node.data {
                if let Element::HashMapNode(nested_node) = element {
                    let temp = Element::<V>::HashMapNode(HashMapNode::Cid(nested_node.collapse()?));
                    std::mem::replace(element, temp);
                }
            }
        };

        //self.root = Box::new(HashMapNode::Cid(self.root.collapse()?));
        Ok(())
    }

    pub fn cid(&mut self) -> Result<Cid, HashMapError> {
        self.collapse()?;
        let result = DagCborCodec
            .encode(self)
            .map_err(|err| HashMapError::CidConversionFailure)?;
        let cid = Cid::new_v1(0x71, Code::Sha2_256.digest(&result));
        Ok(cid)
    }
}

// MISC STUFF
// MISC STUFF
// MISC STUFF

impl<T: CidStore, V: Serialize + Debug + Encode<DagCborCodec> + Decode<DagCborCodec>>
    From<HashMap<Complete, T, V>> for HashMap<Edit, T, V>
{
    fn from(map: HashMap<Complete, T, V>) -> Self {
        HashMap {
            options: map.options,
            root: map.root,
            state: Edit,
            store: map.store,
        }
    }
}

impl<T: CidStore> HashMap<Complete, T, String> {
    pub fn new(store: T) -> HashMap<Complete, T, String> {
        HashMap {
            options: HashMapOptions {
                hash_alg: Code::Sha2_256,
                bucket_size: 3,
                bit_width: 5,
            },
            root: Box::new(HashMapNode::Node(Node {
                map: bitvec![Lsb0, u8; 0; (2.pow(5))],
                data: vec![],
            })),
            state: Complete,
            store: store,
        }
    }
}

fn to_int(slice: &BitSlice<Msb0, u8>) -> usize {
    // https://www.reddit.com/r/rust/comments/36ixl0/converting_a_vector_of_bits_to_an_integer/crehkpw/
    slice
        .iter()
        .by_val()
        //.rev() - For little endian, must reverse
        .fold(0, |acc, b| (acc << 1) | (b as usize))
}

#[cfg(test)]
mod tests {
    use crate::{
        cidstore::{CidStore, MemStore},
        hamt::{Element, HashMap, HashMapNode, HashMapOptions, Node},
        nodestate::Complete,
    };
    use bitvec::prelude::*;
    use cid::Cid;
    use libipld::{cbor::{DagCborCodec, encode::write_u64}, codec::{Codec, Decode, Encode}};
    use multihash::Code;
    use std::{collections::HashMap as StdHashMap, io::{Read, Write}};
    use std::fs;
    use serde_json::{Result, Value};
    use serde::{Serialize, Deserialize};    

    #[test]
    fn to_int_test() {
        //let array = bitarr![Msb0, usize; 1; 4];
        let array = bitvec![Msb0, u8; 0, 0, 1, 1];
        assert_eq!(super::to_int(&array), 3);
        let array = bitvec![Msb0, u8; 0, 0, 0, 0, 1, 1, 0, 0, 0, 1];
        assert_eq!(super::to_int(&array), 49)
    }

    #[test]
    fn forwards_backwards_insert() {
        let store = MemStore::new();
        let mut map = HashMap::new(store);

        let store = MemStore::new();
        let mut map2 = HashMap::new(store);

        println!();
        for key in 0..1000 {
            let key = key.to_string().into_bytes();
            map.insert(key, true.to_string()).unwrap();
        }
        for key in (0..1000).rev() {
            let key = key.to_string().into_bytes();
            map2.insert(key, true.to_string()).unwrap();
        }

        assert_eq!(map.cid().unwrap(), map2.cid().unwrap());
    }

    #[test]
    fn alice_words() {
        let words: AliceWords =
            serde_json::from_str(&fs::read_to_string("./hamt.json").unwrap()).unwrap();

        let store = MemStore::new();
        let mut map: HashMap<Complete, MemStore, Vec<AliceWordsElem>> = HashMap {
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
            //let hash = Code::Sha2_256.digest(&key);
            //println!("{:b}", V(hash.digest()));
            //println!("{:?}", hash.digest());
            map.insert(key, value).unwrap();
            //println!();
        }
        
        assert_eq!(map.cid().unwrap().to_string(), "bafyreic672jz6huur4c2yekd3uycswe2xfqhjlmtmm5dorb6yoytgflova")
    }

    pub type AliceWords = StdHashMap<String, Vec<AliceWordsElem>>;

    #[derive(Serialize, Deserialize, Debug)]
    pub struct AliceWordsElem {
        line: u64,
        column: u64,
    }

    impl Encode<DagCborCodec> for AliceWordsElem {
        fn encode<W: Write>(&self, c: DagCborCodec, w: &mut W) -> anyhow::Result<()> {
            write_u64(w, 5, 2 as u64)?;
            "line".encode(c, w)?;
            self.line.encode(c, w)?;
            "column".encode(c, w)?;
            self.column.encode(c, w)?;
            Ok(())
        }
    }

    impl Decode<DagCborCodec> for AliceWordsElem {
        fn decode<R: Read + std::io::Seek>(c: DagCborCodec, r: &mut R) -> anyhow::Result<Self> {
            todo!()
        }
    }
}
