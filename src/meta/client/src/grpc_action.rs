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

use std::convert::TryInto;
use std::fmt::Debug;

use common_meta_types::protobuf::meta_service_client::MetaServiceClient;
use common_meta_types::protobuf::ClientInfo;
use common_meta_types::protobuf::RaftRequest;
use common_meta_types::protobuf::WatchRequest;
use common_meta_types::protobuf::WatchResponse;
use common_meta_types::GetKVReply;
use common_meta_types::GetKVReq;
use common_meta_types::ListKVReply;
use common_meta_types::ListKVReq;
use common_meta_types::MGetKVReply;
use common_meta_types::MGetKVReq;
use common_meta_types::TxnReply;
use common_meta_types::TxnRequest;
use common_meta_types::UpsertKVReply;
use common_meta_types::UpsertKVReq;
use tonic::codegen::InterceptedService;
use tonic::transport::Channel;
use tonic::Request;

use crate::grpc_client::AuthInterceptor;
use crate::message::ExportReq;
use crate::message::GetClientInfo;
use crate::message::GetEndpoints;
use crate::message::MakeClient;

/// Bind a request type to its corresponding response type.
pub trait RequestFor {
    type Reply;
}

// TODO: reduce this and MetaGrpcReadReq into one enum?
// Action wrapper for do_action.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, derive_more::From)]
pub enum MetaGrpcWriteReq {
    UpsertKV(UpsertKVReq),
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, derive_more::From)]
pub enum MetaGrpcReadReq {
    GetKV(GetKVReq),
    MGetKV(MGetKVReq),
    ListKV(ListKVReq), // since 2022-05-23
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, derive_more::From)]
pub enum MetaGrpcReq {
    UpsertKV(UpsertKVReq),

    GetKV(GetKVReq),
    MGetKV(MGetKVReq),
    ListKV(ListKVReq),
}

impl TryInto<MetaGrpcReq> for Request<RaftRequest> {
    type Error = tonic::Status;

    fn try_into(self) -> Result<MetaGrpcReq, Self::Error> {
        let raft_request = self.into_inner();

        let json_str = raft_request.data.as_str();
        let req = serde_json::from_str::<MetaGrpcReq>(json_str)
            .map_err(|e| tonic::Status::internal(e.to_string()))?;
        Ok(req)
    }
}

impl tonic::IntoRequest<RaftRequest> for MetaGrpcWriteReq {
    fn into_request(self) -> Request<RaftRequest> {
        let raft_request = RaftRequest {
            data: serde_json::to_string(&self).expect("fail to serialize"),
        };
        tonic::Request::new(raft_request)
    }
}

impl TryInto<Request<RaftRequest>> for MetaGrpcReq {
    type Error = serde_json::Error;

    fn try_into(self) -> Result<Request<RaftRequest>, Self::Error> {
        let raft_request = RaftRequest {
            data: serde_json::to_string(&self)?,
        };

        let request = tonic::Request::new(raft_request);
        Ok(request)
    }
}

impl TryInto<Request<RaftRequest>> for MetaGrpcWriteReq {
    type Error = serde_json::Error;

    fn try_into(self) -> Result<Request<RaftRequest>, Self::Error> {
        let raft_request = RaftRequest {
            data: serde_json::to_string(&self)?,
        };

        let request = tonic::Request::new(raft_request);
        Ok(request)
    }
}

impl TryInto<Request<RaftRequest>> for MetaGrpcReadReq {
    type Error = serde_json::Error;

    fn try_into(self) -> Result<Request<RaftRequest>, Self::Error> {
        let get_req = RaftRequest {
            data: serde_json::to_string(&self)?,
        };

        let request = tonic::Request::new(get_req);
        Ok(request)
    }
}

impl RequestFor for GetKVReq {
    type Reply = GetKVReply;
}

impl RequestFor for MGetKVReq {
    type Reply = MGetKVReply;
}

impl RequestFor for ListKVReq {
    type Reply = ListKVReply;
}

impl RequestFor for UpsertKVReq {
    type Reply = UpsertKVReply;
}

impl RequestFor for WatchRequest {
    type Reply = tonic::codec::Streaming<WatchResponse>;
}

impl RequestFor for ExportReq {
    type Reply = tonic::codec::Streaming<WatchResponse>;
}

impl RequestFor for MakeClient {
    type Reply = MetaServiceClient<InterceptedService<Channel, AuthInterceptor>>;
}

impl RequestFor for GetEndpoints {
    type Reply = Vec<String>;
}

impl RequestFor for TxnRequest {
    type Reply = TxnReply;
}

impl RequestFor for GetClientInfo {
    type Reply = ClientInfo;
}
