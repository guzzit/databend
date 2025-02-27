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

use std::iter::TrustedLen;
use std::sync::atomic::Ordering;

use common_arrow::arrow::bitmap::Bitmap;
use common_arrow::arrow::bitmap::MutableBitmap;
use common_catalog::table_context::TableContext;
use common_datablocks::DataBlock;
use common_exception::ErrorCode;
use common_exception::Result;
use common_hashtable::HashMap;
use common_hashtable::HashTableKeyable;

use crate::pipelines::processors::transforms::hash_join::row::RowPtr;
use crate::pipelines::processors::transforms::hash_join::ProbeState;
use crate::pipelines::processors::JoinHashTable;
use crate::sql::plans::JoinType;

impl JoinHashTable {
    /// Used by right join/right semi(anti) join
    pub(crate) fn probe_right_join<Key, IT>(
        &self,
        hash_table: &HashMap<Key, Vec<RowPtr>>,
        probe_state: &mut ProbeState,
        keys_iter: IT,
        input: &DataBlock,
    ) -> Result<Vec<DataBlock>>
    where
        Key: HashTableKeyable + Clone + 'static,
        IT: Iterator<Item = Key> + TrustedLen,
    {
        let valids = &probe_state.valids;
        let block_size = self.ctx.get_settings().get_max_block_size()? as usize;

        // The right join will return multiple data blocks of similar size
        let mut probed_blocks = vec![];
        let mut probe_indexes = Vec::with_capacity(block_size);
        let mut local_build_indexes = Vec::with_capacity(block_size);

        let mut validity = MutableBitmap::with_capacity(block_size);

        let mut build_indexes = self.hash_join_desc.right_join_desc.build_indexes.write();
        for (i, key) in keys_iter.enumerate() {
            let probe_result_ptr = self.probe_key(hash_table, key, valids, i);

            if let Some(v) = probe_result_ptr {
                let probed_rows = v.get_value();

                if probe_indexes.len() + probed_rows.len() < probe_indexes.capacity() {
                    build_indexes.extend(probed_rows);
                    local_build_indexes.extend_from_slice(probed_rows);
                    probe_indexes.extend(std::iter::repeat(i as u32).take(probed_rows.len()));
                    validity.extend_constant(probed_rows.len(), true);
                } else {
                    let mut index = 0_usize;
                    let mut remain = probed_rows.len();

                    while index < probed_rows.len() {
                        if probe_indexes.len() + remain < probe_indexes.capacity() {
                            build_indexes.extend_from_slice(&probed_rows[index..]);
                            local_build_indexes.extend_from_slice(&probed_rows[index..]);
                            probe_indexes.extend(std::iter::repeat(i as u32).take(remain));
                            validity.extend_constant(remain, true);

                            index += remain;
                        } else {
                            if self.interrupt.load(Ordering::Relaxed) {
                                return Err(ErrorCode::AbortedQuery(
                                    "Aborted query, because the server is shutting down or the query was killed.",
                                ));
                            }

                            let addition = probe_indexes.capacity() - probe_indexes.len();
                            let new_index = index + addition;

                            build_indexes.extend_from_slice(&probed_rows[index..new_index]);
                            local_build_indexes.extend_from_slice(&probed_rows[index..new_index]);
                            probe_indexes.extend(std::iter::repeat(i as u32).take(addition));
                            validity.extend_constant(addition, true);

                            let build_block = self.row_space.gather(&local_build_indexes)?;
                            let mut probe_block =
                                DataBlock::block_take_by_indices(input, &probe_indexes)?;

                            // If join type is right join, need to wrap nullable for probe side
                            // If join type is semi/anti right join, directly merge `build_block` and `probe_block`
                            if self.hash_join_desc.join_type == JoinType::Right {
                                let validity: Bitmap = validity.into();
                                let nullable_columns = probe_block
                                    .columns()
                                    .iter()
                                    .map(|c| Self::set_validity(c, &validity))
                                    .collect::<Result<Vec<_>>>()?;
                                probe_block =
                                    DataBlock::create(self.probe_schema.clone(), nullable_columns);
                            }

                            if !probe_block.is_empty() {
                                probed_blocks
                                    .push(self.merge_eq_block(&build_block, &probe_block)?);
                            }

                            index = new_index;
                            remain -= addition;

                            probe_indexes.clear();
                            local_build_indexes.clear();
                            validity = MutableBitmap::new();
                        }
                    }
                }
            }
        }

        let build_block = self.row_space.gather(&local_build_indexes)?;
        let mut probe_block = DataBlock::block_take_by_indices(input, &probe_indexes)?;

        // If join type is right join, need to wrap nullable for probe side
        // If join type is semi/anti right join, directly merge `build_block` and `probe_block`
        if self.hash_join_desc.join_type == JoinType::Right {
            let validity: Bitmap = validity.into();
            let nullable_columns = probe_block
                .columns()
                .iter()
                .map(|c| Self::set_validity(c, &validity))
                .collect::<Result<Vec<_>>>()?;
            probe_block = DataBlock::create(self.probe_schema.clone(), nullable_columns);
        }

        // We need to return at least one data block
        if !probe_block.is_empty() || !build_block.is_empty() || probed_blocks.is_empty() {
            probed_blocks.push(self.merge_eq_block(&build_block, &probe_block)?);
        }

        Ok(probed_blocks)
    }
}
