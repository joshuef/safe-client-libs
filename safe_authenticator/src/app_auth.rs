// Copyright 2018 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! App authentication routines

use super::{AuthError, AuthFuture};
use crate::access_container;
use crate::access_container::update_container_perms;
use crate::app_container;
use crate::client::AuthClient;
use crate::config::{self, AppInfo, Apps};
use futures::future::{self, Either};
use futures::Future;
use log::trace;
use safe_core::client;
use safe_core::core_structs::{AccessContInfo, AccessContainerEntry, AppKeys};
use safe_core::ipc::req::{AuthReq, ContainerPermissions, Permission};
use safe_core::ipc::resp::AuthGranted;
use safe_core::{
    app_container_name, client::AuthActions, recoverable_apis, Client, FutureExt, MDataInfo,
};
use safe_core::{btree_set, err, fry, ok};
use safe_nd::{AppPermissions, AuthToken, GET_BALANCE, PERFORM_MUTATIONS, TRANSFER_COINS};
use std::collections::HashMap;
use tiny_keccak::sha3_256;
use unwrap::unwrap;

/// Represents current app state
#[derive(Debug, Eq, PartialEq)]
pub enum AppState {
    /// Exists in the authenticator config, access container, and registered in MaidManagers
    Authenticated,
    /// Exists in the authenticator config but not in access container and MaidManagers
    Revoked,
    /// Doesn't exist in the authenticator config
    NotAuthenticated,
}

/// Return a current app state (`Authenticated` if it has an entry
/// in the config file AND the access container, `Revoked` if it has
/// an entry in the config but not in the access container, and `NotAuthenticated`
/// if it's not registered anywhere).
pub fn app_state(client: &AuthClient, apps: &Apps, app_id: &str) -> Box<AuthFuture<AppState>> {
    let app_id_hash = sha3_256(app_id.as_bytes());

    if let Some(app) = apps.get(&app_id_hash) {
        let app_keys = app.keys.clone();

        access_container::fetch_entry(client, app_id, app_keys)
            .then(move |res| {
                match res {
                    Ok((_version, Some(_))) => Ok(AppState::Authenticated),
                    Ok((_, None)) => {
                        // App is not in access container, so it is revoked
                        Ok(AppState::Revoked)
                    }
                    Err(e) => Err(e),
                }
            })
            .into_box()
    } else {
        ok!(AppState::NotAuthenticated)
    }
}

/// Check whether `permissions` has an app container entry for `app_id` and that all permissions are
/// set.
fn app_container_exists(permissions: &AccessContainerEntry, app_id: &str) -> bool {
    match permissions.get(&app_container_name(app_id)) {
        Some(&(_, ref access)) => {
            *access
                == btree_set![
                    Permission::Read,
                    Permission::Insert,
                    Permission::Update,
                    Permission::Delete,
                    Permission::ManagePermissions,
                ]
        }
        None => false,
    }
}

/// Insert info about the app's dedicated container into the access container entry
fn insert_app_container(
    mut app_entry: AccessContainerEntry,
    app_id: &str,
    app_container_info: MDataInfo,
) -> AccessContainerEntry {
    let access = btree_set![
        Permission::Read,
        Permission::Insert,
        Permission::Update,
        Permission::Delete,
        Permission::ManagePermissions,
    ];
    let _ = app_entry.insert(app_container_name(app_id), (app_container_info, access));
    app_entry
}

fn update_access_container(
    client: &AuthClient,
    app: &AppInfo,
    app_entry: AccessContainerEntry,
) -> Box<AuthFuture<()>> {
    let c2 = client.clone();

    let app_id = app.info.id.clone();
    let app_keys = app.keys.clone();

    trace!("Updating access container entry for app {}...", app_id);
    access_container::fetch_entry(client, &app_id, app_keys.clone())
        .then(move |res| {
            let version = match res {
                // Updating an existing entry
                Ok((version, Some(_))) => version + 1,
                // Adding a new access container entry
                Ok((_, None)) => 0,
                // Error has occurred while trying to get an existing entry
                Err(e) => return Err(e),
            };
            Ok((version, app_keys, app_entry))
        })
        .and_then(move |(version, app_keys, app_entry)| {
            access_container::put_entry(&c2, &app_id, &app_keys, &app_entry, version)
        })
        .into_box()
}

/// Authenticate an app.
///
/// 1. First, this function searches for an app info in the access container.
/// 2. TODO: If found... Generate a new token? / Ask for permissions again? / Some other warning...
/// 2.Prev: If the app is found, then the `AuthGranted` struct is returned based on that information.
/// 3. If the app is not found in the access container, then it will be authenticated.
pub fn authenticate(client: &AuthClient, auth_req: AuthReq) -> Box<AuthFuture<AuthGranted>> {
    let app_id = auth_req.app.id.clone();
    // TODO: Update this to be label_permissions
    let container_permissions = auth_req.containers.clone();
    let AuthReq {
        app_container,
        app_permissions,
        ..
    } = auth_req;

    let c2 = client.clone();
    let c3 = client.clone();
    let c4 = client.clone();

    config::list_apps(client)
        .join(check_revocation(client, app_id.clone()))
        .and_then(move |((apps_version, apps), ())| {
            app_state(&c2, &apps, &app_id)
                .map(move |app_state| (apps_version, apps, app_state, app_id))
        })
        .and_then(move |(apps_version, mut apps, app_state, app_id)| {
            // Determine an app state. If it's revoked we can reuse existing
            // keys stored in the config. And if it is authorised, we just
            // return the app info from the config.
            match app_state {
                AppState::NotAuthenticated => {
                    let public_id = c3.public_id();
                    // Safe to unwrap as the auth client will have a client public id.
                    let keys = AppKeys::new(unwrap!(public_id.client_public_id()).clone());
                    let app = AppInfo {
                        info: auth_req.app,
                        keys,
                        perms: auth_req.app_permissions,
                    };
                    config::insert_app(&c3, apps, config::next_version(apps_version), app.clone())
                        .map(move |_| (app, app_state, app_id))
                        .into_box()
                }
                AppState::Authenticated | AppState::Revoked => {
                    let app_entry_name = sha3_256(app_id.as_bytes());
                    if let Some(app) = apps.remove(&app_entry_name) {
                        ok!((app, app_state, app_id))
                    } else {
                        err!(AuthError::from(
                            "Logical error - couldn't find a revoked app in config"
                        ))
                    }
                }
            }
        })
        .and_then(move |(app, app_state, app_id)| {
            match app_state {
                AppState::Authenticated => {
                    // Return info of the already registered app
                    // TODO: What do we do when a token is already issued for an app?
                    // Probably generate _another_ token...?
                    authenticated_app(&c4, app, app_id, app_container, app_permissions)
                }
                AppState::NotAuthenticated | AppState::Revoked => {
                    // Register a new app or restore a previously registered app
                    authenticate_new_app(
                        &c4,
                        app,
                        app_container,
                        app_permissions,
                        container_permissions,
                    )
                }
            }
        })
        .into_box()
}

/// Return info of an already registered app.
/// If `app_container` is `true` then we also create/update the dedicated container.
fn authenticated_app(
    client: &AuthClient,
    app: AppInfo,
    app_id: String,
    app_container: bool,
    _app_permissions: AppPermissions,
) -> Box<AuthFuture<AuthGranted>> {
    let c2 = client.clone();
    let c3 = client.clone();

    let app_keys = app.keys.clone();
    let app_pk = app.keys.public_key();
    let bootstrap_config = fry!(client::bootstrap_config());

    // TODO: Here, we should regenerate a token based upon reqs...
    access_container::fetch_entry(client, &app_id, app_keys.clone())
        .and_then(move |(_version, perms)| {
            let perms = perms.unwrap_or_else(AccessContainerEntry::default);

            // TODO: check if we need to update app permissions

            // Check whether we need to create/update dedicated container
            if app_container && !app_container_exists(&perms, &app_id) {
                let future = app_container::fetch_or_create(&c2, &app_id, app_pk).and_then(
                    move |mdata_info| {
                        let perms = insert_app_container(perms, &app_id, mdata_info);
                        update_access_container(&c2, &app, perms.clone()).map(move |_| perms)
                    },
                );
                Either::A(future)
            } else {
                Either::B(future::ok(perms))
            }
        })
        .and_then(move |perms| {
            let access_container_info = c3.access_container();
            let access_container_info = AccessContInfo::from_mdata_info(&access_container_info)?;

            // TODO: Add caveats as makes sense...
            // (where to get them from ? )
            let token = AuthToken::new().expect("Problems creating new token");

            trace!("Token retrieved from existing app, {:?}", token);

            Ok(AuthGranted {
                token,
                app_keys,
                bootstrap_config,
                access_container_info,
                access_container_entry: perms,
            })
        })
        .into_box()
}

/// Register a new or revoked app in Client Handlers and in the access container.
///
/// 1. Generateand sign Token with app keys
/// 2. Insert app's key to Client Handlers
/// 3. TODO: Remove: Update container permissions for requested containers
/// 4. TODO: Remove: Create the app container (if it's been requested)
/// 5. Insert or update the access container entry for an app, with App's PK
/// 6. Return `AuthGranted`
fn authenticate_new_app(
    client: &AuthClient,
    app: AppInfo,
    app_container: bool,
    app_permissions: AppPermissions,
    // TODO: Convert permission string into parseable caveats.
    // TODO: Remove ContainerPermissions
    // TODO: Add label permissions.
    permissions: HashMap<String, ContainerPermissions>,
) -> Box<AuthFuture<AuthGranted>> {
    let c2 = client.clone();
    let c3 = client.clone();
    let c4 = client.clone();
    let c5 = client.clone();
    let c6 = client.clone();

    let app_pk = app.keys.public_key();
    let app_keys = app.keys.clone();
    let app_keys_auth = app.keys.clone();
    let app_id = app.info.id.clone();
    let client_safe_key = client.full_id();

    let mut token = AuthToken::new().expect("Error creating new token.");

    let transfer_coins_caveat = (
        TRANSFER_COINS.to_string(),
        format!("{}", app_permissions.transfer_coins),
    );
    let mutate_caveat = (
        PERFORM_MUTATIONS.to_string(),
        format!("{}", app_permissions.perform_mutations),
    );
    let balance_caveat = (
        GET_BALANCE.to_string(),
        format!("{}", app_permissions.get_balance),
    );

    token
        .add_caveat(transfer_coins_caveat, &client_safe_key)
        .expect("Failed to add transfer caveat to token.");
    token
        .add_caveat(mutate_caveat, &client_safe_key)
        .expect("Failed to add mutate caveat to token.");
    token
        .add_caveat(balance_caveat, &client_safe_key)
        .expect("Failed to add get_balance caveat to token.");

    client
        .list_auth_keys_and_version()
        .and_then(move |(_, version)| {
            recoverable_apis::ins_auth_key_to_client_h(
                &c2,
                app_keys.public_key(),
                app_permissions,
                version + 1,
            )
        })
        .map_err(AuthError::from)
        .and_then(move |_| {
            if permissions.is_empty() {
                ok!((Default::default(), app_pk))
            } else {
                update_container_perms(&c3, permissions, app_pk)
                    .map(move |perms| (perms, app_pk))
                    .into_box()
            }
        })
        .and_then(move |(perms, app_pk)| {
            if app_container {
                app_container::fetch_or_create(&c4, &app_id, app_pk)
                    .and_then(move |mdata_info| {
                        ok!(insert_app_container(perms, &app_id, mdata_info))
                    })
                    .map(move |perms| (perms, app))
                    .into_box()
            } else {
                ok!((perms, app))
            }
        })
        .and_then(move |(perms, app)| {
            update_access_container(&c5, &app, perms.clone()).map(move |_| perms)
        })
        .and_then(move |access_container_entry| {
            let access_container_info = c6.access_container();
            let access_container_info = AccessContInfo::from_mdata_info(&access_container_info)?;

            Ok(AuthGranted {
                token,
                app_keys: app_keys_auth,
                bootstrap_config: client::bootstrap_config()?,
                access_container_info,
                access_container_entry,
            })
        })
        .into_box()
}

// TODO: Update for tokens
fn check_revocation(client: &AuthClient, app_id: String) -> Box<AuthFuture<()>> {
    config::get_app_revocation_queue(client)
        .and_then(move |(_, queue)| {
            if queue.contains(&app_id) {
                Err(AuthError::PendingRevocation)
            } else {
                Ok(())
            }
        })
        .into_box()
}
