use alloy::primitives::B256;
use ethereum_consensus::crypto::{PublicKey as BlsPublicKey, Signature as BlsSignature};
use eyre::{bail, Result};
use futures::{stream, StreamExt};
use tracing::{debug, warn};

use crate::delegation::dirk::recover_signature::recover_signature_from_shards;

use super::{pb::eth2_signer_api::{self, Endpoint}, Dirk, DirkTlsCredentials};

#[derive(Debug)]
pub struct DistributedDirkAccount {
    credentials: DirkTlsCredentials,
    participants: Vec<Endpoint>,
    threshold: usize,
    composite_public_key: BlsPublicKey,
}

impl DistributedDirkAccount {
    /// Create a new distributed account.
    pub fn new(
        acc: eth2_signer_api::DistributedAccount,
        credentials: DirkTlsCredentials,
    ) -> Result<Self> {
        let composite_public_key = BlsPublicKey::try_from(acc.composite_public_key.as_ref())?;

        Ok(Self {
            credentials,
            composite_public_key,
            participants: acc.participants,
            threshold: acc.signing_threshold as usize,
        })
    }

    /// Obtain the composite public key of the distributed account.
    pub fn composite_public_key(&self) -> &BlsPublicKey {
        &self.composite_public_key
    }

    /// Obtain a threshold signature from the quorum of participants in the distributed account.
    pub async fn threshold_sign(
        &self,
        account_name: String,
        hash: B256,
        domain: B256,
    ) -> Result<BlsSignature> {
        let mut futures = Vec::with_capacity(self.participants.len());

        for participant in &self.participants {
            let cred = self.credentials.clone();
            let account_name = account_name.clone();
            futures.push(async move {
                let url = participant_url(participant);
                let Ok(mut conn) = Dirk::connect(url.clone(), cred).await else {
                    warn!("Failed to connect to remote signer at {}", url);
                    return None;
                };

                // Every remote signer must sign the same message
                match conn.request_signature(account_name, hash, domain).await {
                    Ok(signature) => Some((signature, participant.id)),
                    Err(err) => {
                        warn!(?err, "Failed to sign message with remote signer at {}", url);
                        None
                    }
                }
            });
        }

        // Buffer the futures to process up to 8 at a time (to avoid overwhelming Dirk servers)
        let mut signatures_with_ids = Vec::with_capacity(self.threshold);
        let mut futures_stream = stream::iter(futures.into_iter()).buffer_unordered(8);

        // Short-circuit when we have enough signatures to reach the threshold
        while let Some(result) = futures_stream.next().await {
            if let Some(signature_with_id) = result {
                signatures_with_ids.push(signature_with_id);
                if signatures_with_ids.len() >= self.threshold {
                    break;
                }
            }
        }

        debug!("Received {} signatures from the distributed account", signatures_with_ids.len());

        if signatures_with_ids.len() < self.threshold {
            bail!(
                "Insufficient signatures: got {}, expected {}",
                signatures_with_ids.len(),
                self.threshold
            );
        }

        let (sigs, ids): (Vec<BlsSignature>, Vec<u64>) = signatures_with_ids.into_iter().unzip();
        let Some(agg_signature) = recover_signature_from_shards(&sigs, &ids) else {
            bail!("Failed to recover the combined signature from partial signatures");
        };

        Ok(agg_signature)
    }
}

/// Build the participant URL for the given endpoint.
fn participant_url(endpoint: &Endpoint) -> String {
    // Note: the Dirk endpoint address must be parsed as "https://name:port".
    // Sauce: https://github.com/wealdtech/go-eth2-wallet-dirk/blob/263190301ef3352fbda43f91363145f175a12cf6/grpc.go#L1706
    format!("https://{}:{}", endpoint.name, endpoint.port)
}
