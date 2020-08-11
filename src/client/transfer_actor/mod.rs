use safe_nd::{
    ClientFullId, Cmd, DebitAgreementProof, Message, PublicId, PublicKey, Query, QueryResponse,
    TransferCmd, TransferQuery,
};

use safe_transfers::{
    ActorEvent, ReplicaValidator, TransferActor as SafeTransferActor, TransferInitiated,
};

use crate::client::ConnectionManager;
use crate::client::{create_cmd_message, create_query_message, COST_OF_PUT};
use crate::errors::CoreError;
use crdts::Dot;
use futures::lock::Mutex;

use log::{debug, info, trace, warn};

use std::sync::Arc;
use threshold_crypto::PublicKeySet;

/// Module for Money balance management
pub mod balance_management;
/// Module for setting up SafeTransferActor
pub mod setup;
/// Module for simulating Money for testing
pub mod simulated_payouts;
/// Module containing all PUT apis
pub mod write_apis;

#[cfg(test)]
pub mod test_utils;

/// Handle Money Transfers, requests and locally stores a balance
pub struct TransferActor {
    transfer_actor: Arc<Mutex<SafeTransferActor<ClientTransferValidator>>>,
    full_id: ClientFullId,
    replicas_pk_set: PublicKeySet,
    simulated_farming_payout_dot: Dot<PublicKey>,
    connection_manager: ConnectionManager,
}

/// Simple client side validations
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientTransferValidator {}

impl ReplicaValidator for ClientTransferValidator {
    fn is_valid(&self, _replica_group: PublicKey) -> bool {
        true
    }
}

impl TransferActor {
    // fn wrap_money_request(req: MoneyRequest) -> ClientRequest {
    //     ClientRequest::System(SystemOp::Transfers(req))
    // }

    /// Get a payment proof
    pub async fn get_payment_proof(&mut self) -> Result<DebitAgreementProof, CoreError> {
        // --------------------------
        // Payment for PUT
        // --------------------------
        self.create_write_payment_proof().await
    }

    /// Retrieve the history of the acocunt from the network and apply to our local actor
    pub async fn get_history(&mut self) -> Result<(), CoreError> {
        let public_key = *self.full_id.public_key();
        info!("Getting SafeTransfers history for pk: {:?}", public_key);

        let msg_contents = Query::Transfer(TransferQuery::GetHistory {
            at: public_key,
            since_version: 0,
        });

        let message = create_query_message(msg_contents);

        let _bootstrapped = self.connection_manager.bootstrap().await;

        // This is a normal response manager request. We want quorum on this for now...
        let res = self.connection_manager.send_query(&message).await?;

        let history = match res {
            QueryResponse::GetHistory(history) => history.map_err(CoreError::from),
            _ => Err(CoreError::from(format!(
                "Unexpected response when retrieving account history {:?}",
                res
            ))),
        }?;

        let mut actor = self.transfer_actor.lock().await;
        match actor.synch(history) {
            Ok(synced_transfer_outcome) => {
                if let Some(transfers) = synced_transfer_outcome {
                    actor.apply(ActorEvent::TransfersSynched(transfers))?;
                }
            }
            Err(error) => {
                if !error
                    .clone()
                    .to_string()
                    .contains("No credits or debits found to sync to actor")
                {
                    return Err(CoreError::from(error));
                }

                warn!(
                    "No new transfer history  by TransferActor for pk: {:?}",
                    public_key
                );

                warn!("current balance {:?}", actor.balance());
            }
        }

        Ok(())
    }

    /// Validates a tranction for paying store_cost
    async fn create_write_payment_proof(&mut self) -> Result<DebitAgreementProof, CoreError> {
        info!("Sending requests for payment for write operation");

        //set up message
        let safe_key = self.full_id.clone();

        self.get_history().await?;

        let section_key = PublicKey::Bls(self.replicas_pk_set.public_key());
        // let mut actor = self.transfer_actor.lock().await;

        let signed_transfer = self
            .transfer_actor
            .lock()
            .await
            .transfer(COST_OF_PUT, section_key)?
            .ok_or_else(|| CoreError::from("No transfer produced by actor."))?
            .signed_transfer;

        let command = Cmd::Transfer(TransferCmd::ValidateTransfer(signed_transfer.clone()));

        debug!("Transfer to be sent: {:?}", &signed_transfer);

        let transfer_message = create_cmd_message(command);

        self.transfer_actor
            .lock()
            .await
            .apply(ActorEvent::TransferInitiated(TransferInitiated {
                signed_transfer,
            }))?;

        // setup connection manager
        let _bootstrapped = self.connection_manager.bootstrap().await;

        let payment_proof: DebitAgreementProof = self
            .await_validation(&safe_key.public_id(), &transfer_message)
            .await?;

        debug!("payment proof retrieved");
        Ok(payment_proof)
    }

    /// Send message and await validation and constructin of DebitAgreementProof
    async fn await_validation(
        &mut self,
        pub_id: &PublicId,
        message: &Message,
    ) -> Result<DebitAgreementProof, CoreError> {
        info!("Awaiting transfer validation");
        //let mut cm = self.connection_manager();

        //let proof = self.connection_manager.send_cmd(&pub_id, &message).await?;

        //Ok(proof)
        unimplemented!()
    }

    async fn send_query(&mut self, query: Query) -> Result<QueryResponse, CoreError> {
        // `sign` should be false for GETs on published data, true otherwise.

        println!("-->>Request going out: {:?}", query);

        let message = create_query_message(query);
        self.connection_manager.send_query(&message).await
    }
}

// TODO: Do we need "new" to actually instantiate with a transfer?...
#[cfg(all(test, feature = "simulated-payouts"))]
mod tests {

    use super::*;
    use test_utils::get_keys_and_connection_manager;

    #[tokio::test]
    async fn transfer_actor_creation__() {
        let (safe_key, cm) = get_keys_and_connection_manager().await;
        let _transfer_actor = TransferActor::new(safe_key, self.connection_manager.clone())
            .await
            .unwrap();

        assert!(true);
    }
}
