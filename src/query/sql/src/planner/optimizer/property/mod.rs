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

mod builder;
mod column_stat;
mod enforcer;
#[allow(clippy::module_inception)]
mod property;
mod stat;

pub use builder::RelExpr;
pub use enforcer::require_property;
pub use property::ColumnSet;
pub use property::Distribution;
pub use property::PhysicalProperty;
pub use property::RelationalProperty;
pub use property::RequiredProperty;
