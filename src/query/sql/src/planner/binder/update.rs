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

use std::collections::HashMap;

use common_ast::ast::TableReference;
use common_ast::ast::UpdateStmt;
use common_exception::ErrorCode;
use common_exception::Result;

use crate::binder::Binder;
use crate::binder::ScalarBinder;
use crate::normalize_identifier;
use crate::plans::Plan;
use crate::plans::UpdatePlan;
use crate::BindContext;

impl<'a> Binder {
    pub(in crate::planner::binder) async fn bind_update(
        &mut self,
        bind_context: &BindContext,
        stmt: &UpdateStmt<'a>,
    ) -> Result<Plan> {
        let UpdateStmt {
            table,
            update_list,
            selection,
        } = stmt;

        let (catalog_name, database_name, table_name) = if let TableReference::Table {
            catalog,
            database,
            table,
            ..
        } = table
        {
            (
                catalog
                    .as_ref()
                    .map_or_else(|| self.ctx.get_current_catalog(), |i| i.name.clone()),
                database
                    .as_ref()
                    .map_or_else(|| self.ctx.get_current_database(), |i| i.name.clone()),
                table.name.clone(),
            )
        } else {
            // we do not support USING clause yet
            return Err(ErrorCode::LogicalError(
                "should not happen, parser should have report error already",
            ));
        };

        let (_, context) = self.bind_table_reference(bind_context, table).await?;

        let table = self
            .ctx
            .get_table(&catalog_name, &database_name, &table_name)
            .await?;
        let table_id = table.get_id();

        let mut scalar_binder = ScalarBinder::new(
            &context,
            self.ctx.clone(),
            &self.name_resolution_ctx,
            self.metadata.clone(),
            &[],
        );
        let schema = table.schema();
        let mut update_columns = HashMap::with_capacity(update_list.len());
        for update_expr in update_list {
            let col_name = normalize_identifier(&update_expr.name, &self.name_resolution_ctx).name;
            let index = schema.index_of(&col_name)?;
            if update_columns.contains_key(&index) {
                return Err(ErrorCode::BadArguments(format!(
                    "Multiple assignments in the single statement to column `{}`",
                    col_name
                )));
            }

            let (scalar, _) = scalar_binder.bind(&update_expr.expr).await?;
            update_columns.insert(index, scalar);
        }

        let push_downs = if let Some(expr) = selection {
            let (scalar, _) = scalar_binder.bind(expr).await?;
            Some(scalar)
        } else {
            None
        };

        let plan = UpdatePlan {
            catalog: catalog_name,
            database: database_name,
            table: table_name,
            table_id,
            update_list: update_columns,
            selection: push_downs,
        };
        Ok(Plan::Update(Box::new(plan)))
    }
}
