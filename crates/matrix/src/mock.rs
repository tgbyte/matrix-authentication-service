// Copyright 2023 The Matrix.org Foundation C.I.C.
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

use std::collections::{HashMap, HashSet};

use anyhow::Context;
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::{HomeserverConnection, MatrixUser, ProvisionRequest};

struct MockUser {
    sub: String,
    avatar_url: Option<String>,
    displayname: Option<String>,
    devices: HashSet<String>,
    emails: Option<Vec<String>>,
}

/// A Mock implementation of a [`HomeserverConnection`], which never fails and
/// doesn't do anything.
pub struct MockHomeserverConnection {
    homeserver: String,
    users: RwLock<HashMap<String, MockUser>>,
}

impl MockHomeserverConnection {
    /// Create a new [`MockHomeserverConnection`].
    pub fn new<H>(homeserver: H) -> Self
    where
        H: Into<String>,
    {
        Self {
            homeserver: homeserver.into(),
            users: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl HomeserverConnection for MockHomeserverConnection {
    type Error = anyhow::Error;

    fn homeserver(&self) -> &str {
        &self.homeserver
    }

    async fn query_user(&self, mxid: &str) -> Result<MatrixUser, Self::Error> {
        let users = self.users.read().await;
        let user = users.get(mxid).context("User not found")?;
        Ok(MatrixUser {
            displayname: user.displayname.clone(),
            avatar_url: user.avatar_url.clone(),
        })
    }

    async fn provision_user(&self, request: &ProvisionRequest) -> Result<bool, Self::Error> {
        let mut users = self.users.write().await;
        let inserted = !users.contains_key(request.mxid());
        let user = users.entry(request.mxid().to_owned()).or_insert(MockUser {
            sub: request.sub().to_owned(),
            avatar_url: None,
            displayname: None,
            devices: HashSet::new(),
            emails: None,
        });

        anyhow::ensure!(
            user.sub == request.sub(),
            "User already provisioned with different sub"
        );

        request.on_emails(|emails| {
            user.emails = emails.map(ToOwned::to_owned);
        });

        request.on_displayname(|displayname| {
            user.displayname = displayname.map(ToOwned::to_owned);
        });

        request.on_avatar_url(|avatar_url| {
            user.avatar_url = avatar_url.map(ToOwned::to_owned);
        });

        Ok(inserted)
    }

    async fn create_device(&self, mxid: &str, device_id: &str) -> Result<(), Self::Error> {
        let mut users = self.users.write().await;
        let user = users.get_mut(mxid).context("User not found")?;
        user.devices.insert(device_id.to_owned());
        Ok(())
    }

    async fn delete_device(&self, mxid: &str, device_id: &str) -> Result<(), Self::Error> {
        let mut users = self.users.write().await;
        let user = users.get_mut(mxid).context("User not found")?;
        user.devices.remove(device_id);
        Ok(())
    }

    async fn delete_user(&self, mxid: &str, erase: bool) -> Result<(), Self::Error> {
        let mut users = self.users.write().await;
        let user = users.get_mut(mxid).context("User not found")?;
        user.devices.clear();
        user.emails = None;
        if erase {
            user.avatar_url = None;
            user.displayname = None;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_connection() {
        let conn = MockHomeserverConnection::new("example.org");

        let mxid = "@test:example.org";
        let device = "test";
        assert_eq!(conn.homeserver(), "example.org");
        assert_eq!(conn.mxid("test"), mxid);

        assert!(conn.query_user(mxid).await.is_err());
        assert!(conn.create_device(mxid, device).await.is_err());
        assert!(conn.delete_device(mxid, device).await.is_err());

        let request = ProvisionRequest::new("@test:example.org", "test")
            .set_displayname("Test User".into())
            .set_avatar_url("mxc://example.org/1234567890".into())
            .set_emails(vec!["test@example.org".to_owned()]);

        let inserted = conn.provision_user(&request).await.unwrap();
        assert!(inserted);

        let user = conn.query_user("@test:example.org").await.unwrap();
        assert_eq!(user.displayname, Some("Test User".into()));
        assert_eq!(user.avatar_url, Some("mxc://example.org/1234567890".into()));

        // Deleting a non-existent device should not fail
        assert!(conn.delete_device(mxid, device).await.is_ok());

        // Create the device
        assert!(conn.create_device(mxid, device).await.is_ok());
        // Create the same device again
        assert!(conn.create_device(mxid, device).await.is_ok());

        // XXX: there is no API to query devices yet in the trait
        // Delete the device
        assert!(conn.delete_device(mxid, device).await.is_ok());
    }
}
