use safe_nd::{ClientFullId, Query, QueryResponse, TransferQuery};

use crate::client::Client;
use crate::client::ConnectionManager;
use crate::errors::CoreError;

use log::trace;

use threshold_crypto::PublicKeySet;

/// Handle all Money transfers and Write API requests for a given ClientId.
impl Client {
    /// Get our replica instance PK set
    pub async fn get_replica_keys(
        full_id: ClientFullId,
        cm: &mut ConnectionManager,
    ) -> Result<PublicKeySet, CoreError> {
        trace!("Getting replica keys for {:?}", full_id);

        let keys_query_msg = Query::Transfer(TransferQuery::GetReplicaKeys(*full_id.public_key()));

        let message = Self::create_query_message(keys_query_msg);

        cm.bootstrap().await?;
        let res = cm.send_query(&message).await?;

        match res {
            QueryResponse::GetReplicaKeys(pk_set) => Ok(pk_set?),
            _ => Err(CoreError::from(format!(
                "Unexpected response when retrieving account replica keys for {:?}",
                full_id.public_key()
            ))),
        }
    }

    // /// Create a new Transfer Actor for a previously unused public key
    // pub async fn new(
    //     full_id: &ClientFullId,
    //     // mut connection_manager: ConnectionManager,
    // ) -> Result<Self, CoreError> {
    //     info!(
    //         "Initiating Safe Transfer Actor for PK {:?}",
    //         full_id.public_key()
    //     );
    //     let simulated_farming_payout_dot =
    //         Dot::new(PublicKey::from(SecretKey::random().public_key()), 0);

    //     let replicas_pk_set =
    //         TransferActor::get_replica_keys(full_id.clone(), &mut connection_manager).await?;

    //     let validator = ClientTransferValidator {};

    //     let transfer_actor = Arc::new(Mutex::new(SafeTransferActor::new(
    //         full_id.keypair().clone(),
    //         replicas_pk_set.clone(),
    //         validator,
    //     )));

    //     let actor = Self {
    //         full_id: full_id.clone(),
    //         transfer_actor,
    //         connection_manager,
    //         replicas_pk_set,
    //         simulated_farming_payout_dot, // replicas_sk_set
    //     };

    //     #[cfg(feature = "simulated-payouts")]
    //     {
    //         // we're testing, and currently a lot of tests expect 10 money to start
    //         let _ = actor
    //             .trigger_simulated_farming_payout(full_id.public_key(), Money::from_str("10")?)
    //             .await?;
    //     }
    //     Ok(actor)
    // }
}

// --------------------------------
// Tests
// ---------------------------------

// TODO: Do we need "new" to actually instantiate with a transfer?...
#[cfg(all(test, feature = "simulated-payouts"))]
mod tests {

    use super::*;
    use crate::crypto::shared_box;
    use safe_nd::Money;
    use std::str::FromStr;

    #[tokio::test]
    async fn transfer_actor_creation__() -> Result<(), CoreError> {
        let (sk, pk) = shared_box::gen_bls_keypair();
        let _transfer_actor = Client::new(Some(sk)).await?;

        assert!(true);

        Ok(())
    }

    #[tokio::test]
    async fn transfer_actor_creation_hydration_for_nonexistant_balance() -> Result<(), CoreError> {
        let (sk, pk) = shared_box::gen_bls_keypair();

        match Client::new(Some(sk)).await {
            Ok(actor) => {
                assert_eq!(actor.get_local_balance().await, Money::from_str("0").unwrap() );
                Ok(())
            },
            Err(e) => panic!("Should not error for nonexistant keys, only create a new instance with no history, we got: {:?}" , e )
        }
    }

    // TODO: only do this for real vault until we a local replica bank
    #[tokio::test]
    #[cfg(not(feature = "mock-network"))]
    async fn transfer_actor_creation_hydration_for_existing_balance() -> Result<(), CoreError> {
        let (sk, pk) = shared_box::gen_bls_keypair();
        let (sk2, pk2) = shared_box::gen_bls_keypair();

        let mut initial_actor = Client::new(Some(sk)).await?;

        let _ = initial_actor
            .trigger_simulated_farming_payout(Money::from_str("100")?)
            .await?;

        match Client::new(Some(sk2)).await {
            Ok(mut client) => {
                assert_eq!(
                    client.get_balance_from_network(None).await?,
                    Money::from_str("100")?
                );
                assert_eq!(client.get_local_balance().await, Money::from_str("100")?);

                Ok(())
            }
            Err(e) => panic!("Account should exist {:?}", e),
        }
    }
}
