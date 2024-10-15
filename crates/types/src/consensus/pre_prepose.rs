use alloy::primitives::{keccak256, BlockNumber};
use bytes::Bytes;
use reth_network_peers::PeerId;
use secp256k1::SecretKey;
use serde::{Deserialize, Serialize};

use crate::{
    orders::OrderSet,
    primitive::Signature,
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};

#[derive(Debug, Clone, Hash, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PreProposal {
    pub block_height: BlockNumber,
    pub source:       PeerId,
    pub limit:        Vec<OrderWithStorageData<GroupedVanillaOrder>>,
    pub searcher:     Vec<OrderWithStorageData<TopOfBlockOrder>>,
    /// The signature is over the ethereum height as well as the limit and
    /// searcher sets
    pub signature:    Signature
}

impl PreProposal {
    pub fn generate_pre_proposal(
        ethereum_height: BlockNumber,
        source: PeerId,
        limit: Vec<OrderWithStorageData<GroupedVanillaOrder>>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        sk: &SecretKey
    ) -> Self {
        let mut buf = Vec::new();

        buf.extend(bincode::serialize(&ethereum_height).unwrap());
        buf.extend(bincode::serialize(&limit).unwrap());
        buf.extend(bincode::serialize(&searcher).unwrap());

        let hash = keccak256(buf);
        let sig = reth_primitives::sign_message(sk.secret_bytes().into(), hash).unwrap();

        Self { limit, source, searcher, block_height: ethereum_height, signature: Signature(sig) }
    }

    pub fn new(
        ethereum_height: u64,
        sk: &SecretKey,
        source: PeerId,
        orders: OrderSet<GroupedVanillaOrder, TopOfBlockOrder>
    ) -> Self {
        let OrderSet { limit, searcher } = orders;
        Self::generate_pre_proposal(ethereum_height, source, limit, searcher, sk)
    }

    pub fn is_valid(&self) -> bool {
        let hash = keccak256(self.payload());
        let Ok(source) = self.signature.recover_signer_full_public_key(hash) else {
            return false;
        };
        source == self.source
    }

    fn payload(&self) -> Bytes {
        let mut buf = Vec::new();
        buf.extend(bincode::serialize(&self.block_height).unwrap());
        buf.extend(bincode::serialize(&self.limit).unwrap());
        buf.extend(bincode::serialize(&self.searcher).unwrap());
        Bytes::from_iter(buf)
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::FixedBytes;
    use rand::thread_rng;
    use reth_network_peers::pk2id;
    use secp256k1::Secp256k1;

    use super::{PreProposal, SecretKey};

    #[test]
    fn can_be_constructed() {
        let ethereum_height = 100;
        let limit = vec![];
        let searcher = vec![];
        let source = FixedBytes::<64>::default();
        let sk = SecretKey::new(&mut rand::thread_rng());
        PreProposal::generate_pre_proposal(ethereum_height, source, limit, searcher, &sk);
    }

    #[test]
    fn can_validate_self() {
        let ethereum_height = 100;
        let limit = vec![];
        let searcher = vec![];
        // Generate crypto stuff
        let mut rng = thread_rng();
        let sk = SecretKey::new(&mut rng);
        let secp = Secp256k1::new();
        let pk = sk.public_key(&secp);
        // Grab the source ID from the secret/public keypair
        let source = pk2id(&pk);
        let preproposal =
            PreProposal::generate_pre_proposal(ethereum_height, source, limit, searcher, &sk);

        assert!(preproposal.is_valid(), "Unable to validate self");
    }
}
