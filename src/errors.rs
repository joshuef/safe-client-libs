// Copyright 2015 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under (1) the MaidSafe.net Commercial License,
// version 1.0 or later, or (2) The General Public License (GPL), version 3, depending on which
// licence you accepted on initial access to the Software (the "Licences").
//
// By contributing code to the SAFE Network Software, or to this project generally, you agree to be
// bound by the terms of the MaidSafe Contributor Agreement, version 1.0.  This, along with the
// Licenses can be found in the root directory of this project at LICENSE, COPYING and CONTRIBUTOR.
//
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.
//
// Please review the Licences for the specific language governing permissions and limitations
// relating to use of the SAFE Network Software.

use std::error::Error;

/// Client Errors
pub enum ClientError {
    /// StructuredData has no space available to fit in any user data inside it.
    StructuredDataHeaderSizeProhibitive,
    /// Could not Serialise or Deserialise
    UnsuccessfulEncodeDecode,
    /// Asymmetric Key Decryption Failed
    AsymmetricDecipherFailure,
    /// Symmetric Key Decryption Failed
    SymmetricDecipherFailure,
    /// Routing GET, PUT, POST, DELETE Immediate Failure
    RoutingFailure(::std::io::Error),
    /// ReceivedUnexpectedData
    ReceivedUnexpectedData,
    /// No such data found in local version cache
    VersionCacheMiss,
    /// No such data found in routing-filled cache
    RoutingMessageCacheMiss,
    /// Network operation failed
    NetworkOperationFailure(::routing::error::ResponseError),
    /// Cannot overwrite a root directory if it already exists
    RootDirectoryAlreadyExists,
    /// Generic I/O Error
    GenericIoError(::std::io::Error),
}

impl From<::cbor::CborError> for ClientError {
    fn from(_: ::cbor::CborError) -> ClientError {
        ClientError::UnsuccessfulEncodeDecode
    }
}

impl From<::routing::error::ResponseError> for ClientError {
    fn from(error: ::routing::error::ResponseError) -> ClientError {
        ClientError::NetworkOperationFailure(error)
    }
}

impl From<::std::io::Error> for ClientError {
    fn from(error: ::std::io::Error) -> ClientError {
        ClientError::GenericIoError(error)
    }
}

impl ::std::fmt::Debug for ClientError {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match *self {
            ClientError::StructuredDataHeaderSizeProhibitive => write!(f, "ClientError::StructuredDataHeaderSizeProhibitive"),
            ClientError::UnsuccessfulEncodeDecode            => write!(f, "ClientError::UnsuccessfulEncodeDecode"),
            ClientError::AsymmetricDecipherFailure           => write!(f, "ClientError::AsymmetricDecipherFailure"),
            ClientError::SymmetricDecipherFailure            => write!(f, "ClientError::SymmetricDecipherFailure"),
            ClientError::RoutingFailure(ref error)           => write!(f, "ClientError::RoutingFailure -> {:?}", error.description()),
            ClientError::ReceivedUnexpectedData              => write!(f, "ClientError::ReceivedUnexpectedData"),
            ClientError::VersionCacheMiss                    => write!(f, "ClientError::VersionCacheMiss"),
            ClientError::RoutingMessageCacheMiss             => write!(f, "ClientError::RoutingMessageCacheMiss"),
            ClientError::NetworkOperationFailure(ref error)  => write!(f, "ClientError::NetworkOperationFailure -> {:?}", error.description()),
            ClientError::RootDirectoryAlreadyExists          => write!(f, "ClientError::RootDirectoryAlreadyExists"),
            ClientError::GenericIoError(ref error)           => write!(f, "ClientError::GenericIoError -> {:?}", error.description()),
        }
    }
}