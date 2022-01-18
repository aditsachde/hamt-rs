use ::cid::Cid as ExtCid;
use anyhow::Result;

use minicbor::{
    data::{Tag, Type},
    decode, encode, {Decode, Encode},
};

use std::fmt::Display;

#[derive(Debug, Clone)]
pub struct Cid(pub ExtCid);
impl Ord for Cid {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }

    fn max(self, other: Self) -> Self
    where
        Self: Sized,
    {
        Cid(self.0.max(other.0))
    }

    fn min(self, other: Self) -> Self
    where
        Self: Sized,
    {
        Cid(self.0.min(other.0))
    }

    fn clamp(self, min: Self, max: Self) -> Self
    where
        Self: Sized,
    {
        Cid(self.0.clamp(min.0, max.0))
    }
}

impl PartialOrd for Cid {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }

    fn lt(&self, other: &Self) -> bool {
        self.0.lt(&other.0)
    }

    fn le(&self, other: &Self) -> bool {
        self.0.le(&other.0)
    }

    fn gt(&self, other: &Self) -> bool {
        self.0.gt(&other.0)
    }

    fn ge(&self, other: &Self) -> bool {
        self.0.ge(&other.0)
    }
}

impl PartialEq for Cid {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Cid {}

impl Display for Cid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Encode for Cid {
    fn encode<W: encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), encode::Error<W::Error>> {
        e.tag(minicbor::data::Tag::Unassigned(42))?;
        // Prefix binary with the multibase '0' to signify binary encoding
        let mut bytes = vec![0];
        self.0.write_bytes(&mut bytes).unwrap();
        e.bytes(&bytes)?;
        Ok(())
    }
}

impl Decode<'_> for Cid {
    fn decode(d: &mut minicbor::Decoder<'_>) -> Result<Self, decode::Error> {
        let tag = d.tag()?;
        if tag != Tag::Unassigned(42) {
            return Err(decode::Error::TypeMismatch(Type::Tag, "Unknown tag found!"));
        }
        let cid = ExtCid::read_bytes(&d.bytes()?[1..]);
        match cid {
            Ok(cid) => Ok(Cid(cid)),
            Err(_) => Err(decode::Error::Message("Could not parse invalid CID")),
        }
    }
}
