use crate::{to_int, Cid};
use async_recursion::async_recursion;
use bitvec::prelude::*;
use futures::{stream::Map, StreamExt, TryStreamExt};
use ipfs_api_backend_hyper::{IpfsApi, IpfsClient};
use minicbor::Decode;
use multihash::{Code, MultihashDigest};
use std::ops::Deref;

#[derive(Debug)]
pub struct RootMapBlock {
    root: MapBlock,
    hash_alg: Code,
    width: usize,
}

impl RootMapBlock {
    pub async fn get_key(&self, key: &[u8]) -> Option<Cid> {
        let multihash = self.hash_alg.digest(key);
        let digest = bitvec::prelude::BitVec::<Msb0, _>::from_slice(multihash.digest()).ok()?;

        self.root.get_key(key, &digest, 0, self.width).await
    }
}

impl<'b> Decode<'b> for RootMapBlock {
    fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
        let length = d.map()?.unwrap();

        let mut root: Option<MapBlock> = None;
        let mut hash_alg: Option<u64> = None;

        for _ in 0..length {
            let map_key = d.str()?;
            if map_key == "hamt" {
                root = d.decode()?;
            } else if map_key == "hashAlg" {
                hash_alg = d.decode()?;
            } else {
                d.skip()?;
            }
        }

        let root = root.ok_or(minicbor::decode::Error::EndOfInput)?;
        let hash_alg = hash_alg.ok_or(minicbor::decode::Error::EndOfInput)?;

        let width = log_2(root.elements.len()) as usize;

        Ok(RootMapBlock {
            root,
            width,
            hash_alg: Code::try_from(hash_alg)
                .map_err(|_| minicbor::decode::Error::Message("Invalid hash_alg"))?,
        })
    }
}

const fn num_bits<T>() -> usize {
    std::mem::size_of::<T>() * 8
}

fn log_2(x: usize) -> u32 {
    assert!(x > 0);
    num_bits::<usize>() as u32 - x.leading_zeros() - 1
}

#[derive(Debug)]
struct MapBlock {
    elements: Vec<Option<Element>>,
}

impl MapBlock {
    async fn get(hash: &Cid) -> Result<Self, minicbor::decode::Error> {
        let client = IpfsClient::default();

        let block = client
            .block_get(&hash.to_string())
            .map_ok(|chunk| chunk.to_vec())
            .try_concat()
            .await
            .map_err(|_| minicbor::decode::Error::EndOfInput)?;

        minicbor::decode(&block)
    }

    #[async_recursion(?Send)]
    async fn get_key(
        &self,
        key: &[u8],
        digest: &BitSlice<Msb0, u8>,
        depth: usize,
        width: usize,
    ) -> Option<Cid> {
        let offset = depth * width;
        let index = &digest[offset..(offset + width)];
        let index = to_int(index);

        match self.elements.get(index).map(Option::as_ref).flatten() {
            Some(e) => match e {
                Element::Node(n) => {
                    let n = MapBlock::get(n).await.ok()?;
                    let result = n.get_key(key, digest, depth + 1, width).await;
                    result
                }
                Element::Bucket(b) => match b.binary_search_by(|v| key.cmp(&v.0)) {
                    Ok(i) => Some(b[i].1.clone()),
                    Err(_) => None,
                },
            },
            None => None,
        }
    }
}

impl<'b> Decode<'b> for MapBlock {
    fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let map = d.bytes()?;
        let map: &BitSlice<Msb0, u8> = BitSlice::from_slice(map).unwrap();
        
        let mut elements: Vec<Option<Element>> = vec![];
        let mut cids = d.array_iter::<Element>().unwrap();

        for value in map {
            let value = value.deref();
            match value {
                true => elements.push(Some(cids.next().unwrap().unwrap())),
                false => elements.push(None),
            }
        }

        Ok(MapBlock { elements })
    }
}

type BucketEntry = (Vec<u8>, Cid);

#[derive(Debug)]
enum Element {
    Node(Cid),
    Bucket(Vec<BucketEntry>),
}

impl<'b> Decode<'b> for Element {
    fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
        match d.probe().tag() {
            Ok(_) => Ok(Element::Node(d.decode()?)),
            Err(_) => {
                let mut entries: Vec<BucketEntry> = vec![];

                let length = d.array()?.unwrap();
                for _ in 0..length {
                    d.array()?;
                    let entry: BucketEntry = (d.bytes()?.to_vec(), d.decode()?);
                    entries.push(entry);
                }

                Ok(Element::Bucket(entries))
            }
        }
    }
}
