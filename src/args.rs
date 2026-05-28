use std::str::{self, FromStr, SplitAsciiWhitespace, Utf8Error};

use crate::BrokerError;

pub struct Args<'a> {
    data: &'a str,
    iter: SplitAsciiWhitespace<'a>,
}

impl<'a> Args<'a> {
    pub fn new(buf: &'a [u8]) -> Result<Self, Utf8Error> {
        let data = str::from_utf8(buf)?;
        Ok(Self {
            data,
            iter: data.split_ascii_whitespace(),
        })
    }

    pub fn as_str(&self) -> &'a str {
        self.data
    }

    pub fn next(&mut self) -> Option<&str> {
        self.iter.next()
    }

    pub fn parse<T>(&mut self, msg: &'static str) -> Result<T, BrokerError>
    where
        T: FromStr,
    {
        self.next()
            .ok_or(BrokerError::Missing(msg))?
            .parse()
            .map_err(|_| BrokerError::Parse(msg))
    }
}
