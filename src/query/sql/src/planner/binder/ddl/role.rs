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

use common_exception::Result;
use common_planner::plans::SetRolePlan;

use crate::plans::Plan;
use crate::BindContext;
use crate::Binder;

impl<'a> Binder {
    pub(in crate::planner::binder) async fn bind_set_role(
        &mut self,
        _bind_context: &BindContext,
        is_default: bool,
        role_name: &str,
    ) -> Result<Plan> {
        Ok(Plan::SetRole(Box::new(SetRolePlan {
            is_default,
            role_name: role_name.to_string(),
        })))
    }
}
