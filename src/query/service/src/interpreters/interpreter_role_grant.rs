// Copyright 2021 Datafuse Labs.
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

use std::sync::Arc;

use common_exception::Result;
use common_meta_types::PrincipalIdentity;
use common_planner::plans::GrantRolePlan;
use common_users::RoleCacheManager;
use common_users::UserApiProvider;

use crate::interpreters::Interpreter;
use crate::pipelines::PipelineBuildResult;
use crate::sessions::QueryContext;
use crate::sessions::TableContext;

#[derive(Debug)]
pub struct GrantRoleInterpreter {
    ctx: Arc<QueryContext>,
    plan: GrantRolePlan,
}

impl GrantRoleInterpreter {
    pub fn try_create(ctx: Arc<QueryContext>, plan: GrantRolePlan) -> Result<Self> {
        Ok(GrantRoleInterpreter { ctx, plan })
    }
}

#[async_trait::async_trait]
impl Interpreter for GrantRoleInterpreter {
    fn name(&self) -> &str {
        "GrantRoleInterpreter"
    }

    #[tracing::instrument(level = "debug", skip(self), fields(ctx.id = self.ctx.get_id().as_str()))]
    async fn execute2(&self) -> Result<PipelineBuildResult> {
        let plan = self.plan.clone();
        let tenant = self.ctx.get_tenant();
        let user_mgr = UserApiProvider::instance();

        // TODO: check privileges

        // Check if the grant role exists.
        user_mgr.get_role(&tenant, plan.role.clone()).await?;
        match plan.principal {
            PrincipalIdentity::User(user) => {
                user_mgr
                    .grant_role_to_user(&tenant, user, plan.role)
                    .await?;
            }
            PrincipalIdentity::Role(role) => {
                user_mgr
                    .grant_role_to_role(&tenant, role, plan.role)
                    .await?;
            }
        }

        RoleCacheManager::instance().force_reload(&tenant).await?;
        Ok(PipelineBuildResult::create())
    }
}
