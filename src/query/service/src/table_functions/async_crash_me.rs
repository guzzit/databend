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

use std::any::Any;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

use chrono::NaiveDateTime;
use common_catalog::table_function::TableFunction;
use common_datablocks::DataBlock;
use common_datavalues::chrono::TimeZone;
use common_datavalues::chrono::Utc;
use common_datavalues::prelude::*;
use common_exception::Result;
use common_legacy_expression::LegacyExpression;
use common_legacy_planners::Extras;
use common_legacy_planners::Partitions;
use common_legacy_planners::ReadDataSourcePlan;
use common_legacy_planners::Statistics;
use common_meta_app::schema::TableIdent;
use common_meta_app::schema::TableInfo;
use common_meta_app::schema::TableMeta;
use futures::Stream;

use crate::pipelines::processors::port::OutputPort;
use crate::pipelines::processors::processor::ProcessorPtr;
use crate::pipelines::processors::AsyncSource;
use crate::pipelines::processors::AsyncSourcer;
use crate::pipelines::Pipe;
use crate::pipelines::Pipeline;
use crate::sessions::TableContext;
use crate::storages::Table;
use crate::table_functions::table_function_factory::TableArgs;

pub struct AsyncCrashMeTable {
    table_info: TableInfo,
    panic_message: Option<String>,
}

impl AsyncCrashMeTable {
    pub fn create(
        database_name: &str,
        _table_func_name: &str,
        table_id: u64,
        table_args: TableArgs,
    ) -> Result<Arc<dyn TableFunction>> {
        let mut panic_message = None;
        if let Some(args) = &table_args {
            if args.len() == 1 {
                let arg = &args[0];
                if let LegacyExpression::Literal { value, .. } = arg {
                    panic_message = Some(String::from_utf8(value.as_string()?)?);
                }
            }
        }

        let table_info = TableInfo {
            ident: TableIdent::new(table_id, 0),
            desc: format!("'{}'.'{}'", database_name, "async_crash_me"),
            name: String::from("async_crash_me"),
            meta: TableMeta {
                schema: Arc::new(DataSchema::empty()),
                engine: String::from("async_crash_me"),
                // Assuming that created_on is unnecessary for function table,
                // we could make created_on fixed to pass test_shuffle_action_try_into.
                created_on: Utc.from_utc_datetime(&NaiveDateTime::from_timestamp(0, 0)),
                updated_on: Utc.from_utc_datetime(&NaiveDateTime::from_timestamp(0, 0)),
                ..Default::default()
            },
            ..Default::default()
        };

        Ok(Arc::new(AsyncCrashMeTable {
            table_info,
            panic_message,
        }))
    }
}

#[async_trait::async_trait]
impl Table for AsyncCrashMeTable {
    fn is_local(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn get_table_info(&self) -> &TableInfo {
        &self.table_info
    }

    async fn read_partitions(
        &self,
        _: Arc<dyn TableContext>,
        _: Option<Extras>,
    ) -> Result<(Statistics, Partitions)> {
        // dummy statistics
        Ok((Statistics::new_exact(1, 1, 1, 1), vec![]))
    }

    fn table_args(&self) -> Option<Vec<LegacyExpression>> {
        Some(vec![LegacyExpression::create_literal(DataValue::UInt64(0))])
    }

    fn read_data(
        &self,
        ctx: Arc<dyn TableContext>,
        _plan: &ReadDataSourcePlan,
        pipeline: &mut Pipeline,
    ) -> Result<()> {
        let output = OutputPort::create();
        pipeline.add_pipe(Pipe::SimplePipe {
            inputs_port: vec![],
            outputs_port: vec![output.clone()],
            processors: vec![AsyncCrashMeSource::create(
                ctx,
                output,
                self.panic_message.clone(),
            )?],
        });

        Ok(())
    }
}

struct AsyncCrashMeSource {
    message: Option<String>,
}

impl AsyncCrashMeSource {
    pub fn create(
        ctx: Arc<dyn TableContext>,
        output: Arc<OutputPort>,
        message: Option<String>,
    ) -> Result<ProcessorPtr> {
        AsyncSourcer::create(ctx, output, AsyncCrashMeSource { message })
    }
}

#[async_trait::async_trait]
impl AsyncSource for AsyncCrashMeSource {
    const NAME: &'static str = "async_crash_me";

    #[async_trait::unboxed_simple]
    async fn generate(&mut self) -> Result<Option<DataBlock>> {
        match &self.message {
            None => panic!("async crash me panic"),
            Some(message) => panic!("{}", message),
        }
    }
}

impl TableFunction for AsyncCrashMeTable {
    fn function_name(&self) -> &str {
        self.name()
    }

    fn as_table<'a>(self: Arc<Self>) -> Arc<dyn Table + 'a>
    where Self: 'a {
        self
    }
}

struct AsyncCrashMeStream {
    message: Option<String>,
}

impl Stream for AsyncCrashMeStream {
    type Item = Result<DataBlock>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match &self.message {
            None => panic!("async crash me panic"),
            Some(message) => panic!("{}", message),
        }
    }
}
