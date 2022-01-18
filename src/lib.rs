pub mod car;
mod cid;
pub mod query;
mod value;

pub use crate::cid::Cid;
use ::cid::Cid as ExtCid;
use anyhow::{anyhow, Result};

use bitvec::prelude::*;
use minicbor::{
    data::{Tag, Type},
    decode, encode, {Decode, Encode},
};
use multihash::{Code, MultihashDigest};
use sled::Tree;
use std::{fmt::Display, marker::PhantomData, os::unix::prelude::OsStrExt};
pub use value::Value;

#[derive(Debug)]
pub struct IpldHashMap {
    root: Node,
    options: Options,
}

#[derive(Debug)]
struct Options {
    hash_alg: Code,
    width: usize,
    bucket_size: usize,
}

#[derive(Debug)]
struct Node {
    elements: Vec<Option<Element>>,
}

type BucketEntry = Vec<(Box<[u8]>, Cid, BitVec<Msb0, u8>)>;

#[derive(Debug)]
enum Element {
    Node(Node),
    Bucket(BucketEntry),
}

#[derive(Debug)]
enum CollapsedElement<'a> {
    Node(Cid),
    Bucket(&'a BucketEntry),
}

impl Node {
    fn get(
        &self,
        key: &[u8],
        digest: &BitSlice<Msb0, u8>,
        depth: usize,
        opts: &Options,
    ) -> Option<&Cid> {
        let offset = depth * opts.width;
        let index = &digest[offset..(offset + opts.width)];
        let index = to_int(index);

        match self.elements.get(index).map(Option::as_ref).flatten() {
            Some(e) => match e {
                Element::Node(n) => n.get(key, digest, depth + 1, opts),
                Element::Bucket(b) => match b.binary_search_by(|v| key.cmp(&v.0)) {
                    Ok(i) => Some(&b[i].1),
                    Err(_) => None,
                },
            },
            None => None,
        }
    }

    fn set(
        &mut self,
        key: Box<[u8]>,
        value: Cid,
        digest: BitVec<Msb0, u8>,
        depth: usize,
        opts: &Options,
    ) -> Result<()> {
        let offset = depth * opts.width;
        let index = &digest[offset..(offset + opts.width)];
        let index = to_int(index);

        let test = self.elements.get_mut(index);

        match self.elements.get_mut(index).map(|x| x.as_mut()).flatten() {
            Some(e) => match e {
                Element::Node(n) => n.set(key, value, digest, depth + 1, opts),
                Element::Bucket(b) => match b.binary_search_by(|v| key.cmp(&v.0)) {
                    Ok(i) => {
                        let element = &mut b[i];
                        element.1 = value;
                        Ok(())
                    }
                    Err(i) => {
                        if b.len() < opts.bucket_size {
                            b.insert(i, (key, value, digest));
                            Ok(())
                        } else {
                            let b = std::mem::replace(b, Vec::with_capacity(0));
                            let mut new_node = Self::new(opts.width);
                            for entry in b.into_iter() {
                                new_node.set(entry.0, entry.1, entry.2, depth + 1, opts)?;
                            }
                            new_node.set(key, value, digest, depth + 1, opts)?;
                            *e = Element::Node(new_node);
                            Ok(())
                        }
                    }
                },
            },
            None => {
                self.elements
                    .insert(index, Some(Element::Bucket(vec![(key, value, digest)])));
                Ok(())
            }
        }
    }

    fn new(width: usize) -> Self {
        let node_capcity = 2usize.pow(width.try_into().unwrap());
        Node {
            elements: (0..node_capcity).map(|x| None).collect(),
        }
    }

    fn collapse(&self, tree: &Tree) -> Cid {
        let cids: Vec<Option<CollapsedElement>> = self
            .elements
            .iter()
            .map(|x| {
                x.as_ref().map(|y| match y {
                    Element::Node(n) => CollapsedElement::Node(n.collapse(tree)),
                    Element::Bucket(b) => CollapsedElement::Bucket(b),
                })
            })
            .collect();

        let (cid, block) = Node::serialize(cids);
        tree.insert(cid.0.to_bytes(), block).unwrap();
        cid
    }

    fn serialize(n: Vec<Option<CollapsedElement>>) -> (Cid, Vec<u8>) {
        let map: BitVec<Msb0, u8> = n.iter().map(|x| x.is_some()).collect();
        let data: Vec<CollapsedElement> = n.into_iter().flatten().collect();

        let serialize_node = SerializeNode {
            map: &map.into_vec(),
            data: &data,
        };

        let block = minicbor::to_vec(serialize_node).unwrap();
        let cid = Cid(ExtCid::new_v1(0x71, Code::Sha2_256.digest(&block)));

        (cid, block)
    }
}

struct SerializeNode<'a> {
    map: &'a [u8],
    data: &'a [CollapsedElement<'a>],
}

impl Encode for SerializeNode<'_> {
    fn encode<W: encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), encode::Error<W::Error>> {
        e.array(2)?;
        e.bytes(self.map)?;
        e.array(self.data.len().try_into().unwrap())?;
        for element in self.data {
            match element {
                CollapsedElement::Node(cid) => {
                    e.encode(cid)?;
                }
                CollapsedElement::Bucket(b) => {
                    e.array(b.len().try_into().unwrap())?;
                    for element in b.iter() {
                        e.array(2)?;
                        // Store key
                        e.bytes(&element.0)?;
                        // Store value
                        e.encode(&element.1)?;
                    }
                }
            }
        }

        Ok(())
    }
}

impl Encode for Options {
    fn encode<W: encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), encode::Error<W::Error>> {
        e.map(3)?;
        e.str("hashAlg")?;
        let base: u64 = self.hash_alg.into();
        e.encode(base)?;
        e.str("bucketSize")?;
        e.encode(2usize.pow(self.width.try_into().unwrap()))?;
        e.encode("hamt")?;
        Ok(())
    }
}

impl IpldHashMap {
    pub fn get(&self, key: &[u8]) -> Option<&Cid> {
        let multihash = self.options.hash_alg.digest(key);
        let digest = bitvec::prelude::BitVec::<Msb0, _>::from_slice(multihash.digest()).ok()?;

        self.root.get(key, &digest, 0, &self.options)
    }

    pub fn set(&mut self, key: Box<[u8]>, value: Cid) -> Result<()> {
        let multihash = self.options.hash_alg.digest(&key);
        let digest = bitvec::prelude::BitVec::<Msb0, _>::from_slice(multihash.digest())?;

        self.root.set(key, value, digest, 0, &self.options)
    }

    pub fn collapse(&self, tree: &Tree) -> Cid {
        let cid = self.root.collapse(tree);

        let root = tree.get(cid.0.to_bytes()).unwrap().unwrap();
        let mut root_block = minicbor::to_vec(&self.options).unwrap();
        root_block.extend(root.iter());

        tree.remove(cid.0.to_bytes()).unwrap();

        let cid = Cid(ExtCid::new_v1(0x71, Code::Sha2_256.digest(&root_block)));

        tree.insert(cid.0.to_bytes(), root_block).unwrap();

        cid
    }

    pub fn collapse_partial(&self, tree: &Tree) -> Result<Cid> {
        let subtrees: Vec<&Element> = self
            .root
            .elements
            .iter()
            .filter_map(|x| x.as_ref())
            .collect();

        if subtrees.len() != 1 {
            return Err(anyhow!(
                "Tree has more than one subtree! {}",
                subtrees.len()
            ));
        }

        match subtrees[0] {
            Element::Node(n) => Ok(n.collapse(tree)),
            Element::Bucket(_) => Err(anyhow!(
                "Tree does not have enough elements to partially collapse"
            )),
        }
    }

    pub fn serialize_root_of_subtrees(&self, tree: &Tree, subtrees: Vec<Cid>) -> Result<Cid> {
        let node_capcity = 2usize.pow(self.options.width.try_into().unwrap());
        if subtrees.len() != node_capcity {
            return Err(anyhow!("Subtree count does not match width of tree"));
        }

        let subtrees = subtrees
            .into_iter()
            .map(|x| Some(CollapsedElement::Node(x)))
            .collect();

        let (_, root) = Node::serialize(subtrees);

        let mut root_block = minicbor::to_vec(&self.options).unwrap();
        root_block.extend(root.iter());

        let cid = Cid(ExtCid::new_v1(0x71, Code::Sha2_256.digest(&root_block)));
        tree.insert(cid.0.to_bytes(), root_block).unwrap();
        Ok(cid)
    }

    pub fn new(width: usize, bucket_size: usize) -> IpldHashMap {
        IpldHashMap {
            root: Node::new(width),
            options: Options {
                hash_alg: Code::Sha2_256,
                width,
                bucket_size,
            },
        }
    }
}

pub fn to_int(slice: &BitSlice<Msb0, u8>) -> usize {
    // https://www.reddit.com/r/rust/comments/36ixl0/converting_a_vector_of_bits_to_an_integer/crehkpw/
    slice
        .iter()
        .by_val()
        //.rev() - For little endian, must reverse
        .fold(0, |acc, b| (acc << 1) | (b as usize))
}
