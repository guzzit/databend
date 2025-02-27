// Copyright 2022 Datafuse Labs.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! This mod defines a key space in state machine to store the index of keys with an expiration time.
//!
//! This secondary index is `(expire_time, seq) -> key`, as the key-value's primary index is `key -> (seq, expire_time, value)`.
//! Because `seq` in meta-store is globally unique, it may be used to identify every update to every key.

use std::fmt::Display;
use std::fmt::Formatter;
use std::mem::size_of_val;
use std::time::Duration;
use std::time::UNIX_EPOCH;

use byteorder::BigEndian;
use byteorder::ByteOrder;
use chrono::DateTime;
use chrono::Utc;
use common_meta_sled_store::sled::IVec;
use common_meta_sled_store::SledBytesError;
use common_meta_sled_store::SledOrderedSerde;

/// The identifier of the index for kv with expiration.
#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ExpireKey {
    /// The time in millisecond when a key will be expired.
    pub time_ms: u64,

    /// The `seq` of the value when the key is written.
    ///
    /// The `seq` of value is globally unique in meta-store.
    pub seq: u64,
}

/// The value of an expiration index is the record key.
#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExpireValue {
    key: String,
}

impl SledOrderedSerde for ExpireKey {
    fn ser(&self) -> Result<IVec, SledBytesError> {
        let size = size_of_val(self);
        let mut buf = vec![0; size];

        BigEndian::write_u64(&mut buf, self.time_ms);
        BigEndian::write_u64(&mut buf[size_of_val(&self.time_ms)..], self.seq);
        Ok(buf.into())
    }

    fn de<V: AsRef<[u8]>>(v: V) -> Result<Self, SledBytesError>
    where Self: Sized {
        let b = v.as_ref();

        let time_ms = BigEndian::read_u64(b);
        let seq = BigEndian::read_u64(&b[size_of_val(&time_ms)..]);

        Ok(Self { time_ms, seq })
    }
}

impl Display for ExpireKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let t = UNIX_EPOCH + Duration::from_millis(self.time_ms);
        let datetime: DateTime<Utc> = t.into();
        write!(f, "{}={}", datetime.format("%Y-%m-%d-%H-%M-%S"), self.seq)
    }
}

impl ExpireKey {
    pub fn new(time_ms: u64, seq: u64) -> Self {
        Self { time_ms, seq }
    }
}

#[cfg(test)]
mod tests {
    use common_meta_sled_store::SledOrderedSerde;

    use crate::state_machine::ExpireKey;

    #[test]
    fn test_expire_key_serde() -> anyhow::Result<()> {
        let k = ExpireKey::new(0x01020304, 0x04030201);
        let enc = <ExpireKey as SledOrderedSerde>::ser(&k)?;
        assert_eq!(
            vec![
                0u8, 0, 0, 0, 1, 2, 3, 4, //
                0u8, 0, 0, 0, 4, 3, 2, 1,
            ],
            enc.as_ref()
        );

        let got_dec = <ExpireKey as SledOrderedSerde>::de(enc)?;
        assert_eq!(k, got_dec);

        Ok(())
    }

    #[test]
    fn test_expire_key_display() -> anyhow::Result<()> {
        let ms = 1666670258202;
        let k = ExpireKey::new(ms, 1000);
        assert_eq!("2022-10-25-03-57-38=1000", format!("{}", k));

        Ok(())
    }
}
