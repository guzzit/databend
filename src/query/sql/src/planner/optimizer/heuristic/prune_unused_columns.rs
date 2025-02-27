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

use common_exception::ErrorCode;
use common_exception::Result;
use common_planner::MetadataRef;
use itertools::Itertools;

use crate::optimizer::ColumnSet;
use crate::optimizer::RelExpr;
use crate::optimizer::SExpr;
use crate::plans::Aggregate;
use crate::plans::EvalScalar;
use crate::plans::LogicalGet;
use crate::plans::RelOperator;
use crate::ScalarExpr;

pub struct UnusedColumnPruner {
    metadata: MetadataRef,
}

impl UnusedColumnPruner {
    pub fn new(metadata: MetadataRef) -> Self {
        Self { metadata }
    }

    pub fn remove_unused_columns(&self, expr: &SExpr, require_columns: ColumnSet) -> Result<SExpr> {
        self.keep_required_columns(expr, require_columns)
    }

    /// Keep columns referenced by parent plan node.
    /// `required` contains columns referenced by its ancestors. When a node has multiple children,
    /// the required columns for each child could be different and we may include columns not needed
    /// by a specific child. Columns should be skipped once we found it not exist in the subtree as we
    /// visit a plan node.
    fn keep_required_columns(&self, expr: &SExpr, mut required: ColumnSet) -> Result<SExpr> {
        match expr.plan() {
            RelOperator::LogicalGet(p) => {
                // Some table may not have any column,
                // e.g. `system.sync_crash_me`
                if p.columns.is_empty() {
                    return Ok(expr.clone());
                }
                let mut prewhere = p.prewhere.clone();
                let mut used: ColumnSet = required.intersection(&p.columns).cloned().collect();
                if let Some(ref mut pw) = prewhere {
                    debug_assert!(
                        pw.prewhere_columns.is_subset(&p.columns),
                        "prewhere columns should be a subset of scan columns"
                    );
                    // `used` is the columns which prewhere scan needs to output for its upper operator.
                    if used.is_empty() {
                        let smallest_index = if pw.prewhere_columns.is_empty() {
                            self.metadata
                                .read()
                                .find_smallest_column_by_table_index(p.table_index)
                        } else {
                            self.metadata.read().find_smallest_column(
                                pw.prewhere_columns
                                    .iter()
                                    .copied()
                                    .collect::<Vec<_>>()
                                    .as_slice(),
                            )
                        };
                        used.insert(smallest_index);
                    }
                    pw.output_columns = used.clone();
                    // `prune_columns` is after `prewhere_optimize`,
                    // so we need to add prewhere columns to scan columns.
                    used = used.union(&pw.prewhere_columns).cloned().collect();
                }

                if used.is_empty() {
                    let smallest_index = self
                        .metadata
                        .read()
                        .find_smallest_column_by_table_index(p.table_index);
                    used.insert(smallest_index);
                }

                Ok(SExpr::create_leaf(RelOperator::LogicalGet(LogicalGet {
                    table_index: p.table_index,
                    columns: used,
                    push_down_predicates: p.push_down_predicates.clone(),
                    limit: p.limit,
                    order_by: p.order_by.clone(),
                    statistics: p.statistics,
                    prewhere,
                })))
            }
            RelOperator::LogicalInnerJoin(p) => {
                // Include columns referenced in left conditions
                let left = p.left_conditions.iter().fold(required.clone(), |acc, v| {
                    acc.union(&v.used_columns()).cloned().collect()
                });
                // Include columns referenced in right conditions
                let right = p.right_conditions.iter().fold(required.clone(), |acc, v| {
                    acc.union(&v.used_columns()).cloned().collect()
                });

                let others = p.non_equi_conditions.iter().fold(required, |acc, v| {
                    acc.union(&v.used_columns()).cloned().collect()
                });

                Ok(SExpr::create_binary(
                    RelOperator::LogicalInnerJoin(p.clone()),
                    self.keep_required_columns(
                        expr.child(0)?,
                        left.union(&others).cloned().collect(),
                    )?,
                    self.keep_required_columns(
                        expr.child(1)?,
                        right.union(&others).cloned().collect(),
                    )?,
                ))
            }

            RelOperator::EvalScalar(p) => {
                let mut used = vec![];
                // Only keep columns needed by parent plan.
                for s in p.items.iter() {
                    if !required.contains(&s.index) {
                        continue;
                    }
                    used.push(s.clone());
                    s.scalar.used_columns().iter().for_each(|c| {
                        required.insert(*c);
                    })
                }
                if used.is_empty() {
                    // Eliminate unneccessary `EvalScalar`
                    self.keep_required_columns(expr.child(0)?, required)
                } else {
                    Ok(SExpr::create_unary(
                        RelOperator::EvalScalar(EvalScalar { items: used }),
                        self.keep_required_columns(expr.child(0)?, required)?,
                    ))
                }
            }
            RelOperator::Filter(p) => {
                let used = p.predicates.iter().fold(required, |acc, v| {
                    acc.union(&v.used_columns()).cloned().collect()
                });
                Ok(SExpr::create_unary(
                    RelOperator::Filter(p.clone()),
                    self.keep_required_columns(expr.child(0)?, used)?,
                ))
            }
            RelOperator::Aggregate(p) => {
                let mut used = vec![];
                for item in &p.aggregate_functions {
                    if required.contains(&item.index) {
                        for c in item.scalar.used_columns() {
                            required.insert(c);
                        }
                        used.push(item.clone());
                    }
                }

                // Require arbitrary column(which has the smallest column index) for scalar `count(*)`
                // aggregate to prevent empty input `DataBlock`.
                let rel_expr = RelExpr::with_s_expr(expr.child(0)?);
                let rel_prop = rel_expr.derive_relational_prop()?;
                if rel_prop.used_columns.is_empty()
                    && required
                        .intersection(&rel_prop.output_columns)
                        .next()
                        .is_none()
                    && p.group_items.is_empty()
                {
                    required.insert(
                        *rel_prop
                            .output_columns
                            .iter()
                            .sorted()
                            .take(1)
                            .next()
                            .ok_or_else(|| {
                                ErrorCode::LogicalError("Invalid children without output column")
                            })?,
                    );
                }

                p.group_items.iter().for_each(|i| {
                    // If the group item comes from a complex expression, we only include the final
                    // column index here. The used columns will be included in its EvalScalar child.
                    required.insert(i.index);
                });
                Ok(SExpr::create_unary(
                    RelOperator::Aggregate(Aggregate {
                        group_items: p.group_items.clone(),
                        aggregate_functions: used,
                        from_distinct: p.from_distinct,
                        mode: p.mode,
                    }),
                    self.keep_required_columns(expr.child(0)?, required)?,
                ))
            }
            RelOperator::Sort(p) => {
                p.items.iter().for_each(|s| {
                    required.insert(s.index);
                });
                Ok(SExpr::create_unary(
                    RelOperator::Sort(p.clone()),
                    self.keep_required_columns(expr.child(0)?, required)?,
                ))
            }
            RelOperator::Limit(p) => Ok(SExpr::create_unary(
                RelOperator::Limit(p.clone()),
                self.keep_required_columns(expr.child(0)?, required)?,
            )),

            RelOperator::DummyTableScan(_) | RelOperator::UnionAll(_) => Ok(expr.clone()),

            _ => Err(ErrorCode::LogicalError(
                "Attempting to prune columns of a physical plan is not allowed",
            )),
        }
    }
}
