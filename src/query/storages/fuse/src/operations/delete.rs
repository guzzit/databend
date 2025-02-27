//  Copyright 2022 Datafuse Labs.
//
//  Licensed under the Apache License, Version 2.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.

use std::sync::Arc;

use common_catalog::table_context::TableContext;
use common_datavalues::DataSchemaRefExt;
use common_exception::ErrorCode;
use common_exception::Result;
use common_fuse_meta::meta::TableSnapshot;
use common_legacy_expression::LegacyExpression;
use common_legacy_parser::ExpressionParser;
use common_legacy_planners::DeletePlan;
use common_legacy_planners::Extras;
use common_pipeline_transforms::processors::ExpressionExecutor;
use tracing::debug;

use crate::operations::mutation::delete_from_block;
use crate::operations::mutation::deletion_mutator::Deletion;
use crate::operations::mutation::DeletionMutator;
use crate::pruning::BlockPruner;
use crate::statistics::ClusterStatsGenerator;
use crate::FuseTable;

impl FuseTable {
    pub async fn do_delete(&self, ctx: Arc<dyn TableContext>, plan: &DeletePlan) -> Result<()> {
        let snapshot_opt = self.read_table_snapshot().await?;

        // check if table is empty
        let snapshot = if let Some(val) = snapshot_opt {
            val
        } else {
            // no snapshot, no deletion
            return Ok(());
        };

        if snapshot.summary.row_count == 0 {
            // empty snapshot, no deletion
            return Ok(());
        }

        // check if unconditional deletion
        if let Some(filter) = &plan.selection {
            let expr = ExpressionParser::parse_exprs(filter)?;
            if expr.is_empty() {
                return Err(ErrorCode::IndexOutOfBounds(
                    "expression should be valid, but not",
                ));
            }
            self.delete_rows(ctx, &snapshot, &expr[0], plan).await
        } else {
            // deleting the whole table... just a truncate
            let purge = false;
            debug!(
                "unconditionally delete from table, {}.{}.{}",
                plan.catalog_name, plan.database_name, plan.table_name
            );
            self.do_truncate(ctx.clone(), purge).await
        }
    }

    async fn delete_rows(
        &self,
        ctx: Arc<dyn TableContext>,
        snapshot: &Arc<TableSnapshot>,
        filter: &LegacyExpression,
        plan: &DeletePlan,
    ) -> Result<()> {
        let cluster_stats_gen = self.cluster_stats_gen(ctx.clone())?;
        let mut deletion_collector = DeletionMutator::try_create(
            ctx.clone(),
            self.get_operator(),
            self.meta_location_generator.clone(),
            snapshot.clone(),
            cluster_stats_gen,
        )?;
        let schema = self.table_info.schema();
        // TODO refine pruner
        let extras = Extras {
            projection: Some(plan.projection.clone()),
            filters: vec![filter.clone()],
            prewhere: None, // TBD: if delete rows need prewhere optimization
            limit: None,
            order_by: vec![],
        };
        let push_downs = Some(extras);
        let segments_location = snapshot.segments.clone();
        let block_metas = BlockPruner::prune(
            &ctx,
            self.operator.clone(),
            schema,
            &push_downs,
            segments_location,
        )
        .await?;

        // delete block one by one.
        // this could be executed in a distributed manner (till new planner, pipeline settled down)
        for (seg_idx, block_meta) in block_metas {
            let proj = plan.projection.clone();
            match delete_from_block(self, &block_meta, &ctx, proj, filter).await? {
                Deletion::NothingDeleted => {
                    // false positive, we should keep the whole block
                    continue;
                }
                Deletion::Remains(r) => {
                    // after deletion, the data block `r` remains, let keep it  by replacing the block
                    // located at `block_meta.location`, of segment indexed by `seg_idx`, with a new block `r`
                    deletion_collector
                        .replace_with(
                            seg_idx,
                            block_meta.location.clone(),
                            block_meta.cluster_stats.clone(),
                            r,
                        )
                        .await?
                }
            }
        }

        self.commit_deletion(ctx, deletion_collector).await
    }

    async fn commit_deletion(
        &self,
        ctx: Arc<dyn TableContext>,
        del_holder: DeletionMutator,
    ) -> Result<()> {
        let (segments, summary, abort_operation) = del_holder.generate_segments().await?;

        self.commit_mutation(
            &ctx,
            del_holder.base_snapshot(),
            segments,
            summary,
            abort_operation,
        )
        .await
    }

    fn cluster_stats_gen(&self, ctx: Arc<dyn TableContext>) -> Result<ClusterStatsGenerator> {
        if self.cluster_key_meta.is_none() {
            return Ok(ClusterStatsGenerator::default());
        }

        let len = self.cluster_keys.len();
        let cluster_key_id = self.cluster_key_meta.clone().unwrap().0;

        let input_schema = self.table_info.schema();
        let input_fields = input_schema.fields().clone();

        let mut cluster_key_index = Vec::with_capacity(len);
        let mut output_fields = Vec::with_capacity(len);
        let mut exists = true;
        for expr in &self.cluster_keys {
            output_fields.push(expr.to_data_field(&input_schema)?);

            if exists {
                match input_fields
                    .iter()
                    .position(|x| x.name() == &expr.column_name())
                {
                    None => exists = false,
                    Some(idx) => cluster_key_index.push(idx),
                };
            }
        }

        let mut expression_executor = None;
        if !exists {
            cluster_key_index = (0..len).collect();
            let output_schema = DataSchemaRefExt::create(output_fields);
            let executor = ExpressionExecutor::try_create(
                ctx,
                "expression executor for generator cluster statistics",
                input_schema,
                output_schema,
                self.cluster_keys.clone(),
                true,
            )?;
            expression_executor = Some(executor);
        }

        Ok(ClusterStatsGenerator::new(
            cluster_key_id,
            cluster_key_index,
            expression_executor,
            0,
            self.get_block_compactor(),
        ))
    }
}
