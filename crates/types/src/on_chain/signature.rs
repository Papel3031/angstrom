use std::ops::{Deref, DerefMut};

use bytes::{Bytes, BytesMut};
use ethers_core::{
    abi::{AbiArrayType, AbiType, ParamType, Token, Tokenizable, TokenizableItem},
    types::{Signature as ESignature, H256, U256}
};
use reth_primitives::{PeerId, H512};
use reth_rlp::{Decodable, DecodeError, Encodable, RlpDecodable, RlpEncodable};
use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Message, SECP256K1
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::trace;

use super::SafeTx;

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, ethers_contract::EthAbiCodec,
)]
#[repr(transparent)]
pub struct Signature(pub ESignature);

impl AbiType for Signature {
    fn param_type() -> ethers_core::abi::ParamType {
        ParamType::Bytes
    }

    fn minimum_size() -> usize {
        72
    }
}

impl TokenizableItem for Signature {}

impl AbiArrayType for Signature {}

impl Tokenizable for Signature {
    fn into_token(self) -> ethers_core::abi::Token {
        let mut buf = BytesMut::new();
        U256::from(self.v).to_big_endian(&mut buf);
        self.r.to_big_endian(&mut buf);
        self.s.to_big_endian(&mut buf);
        buf.freeze().into_token()
    }

    fn from_token(
        token: ethers_core::abi::Token
    ) -> Result<Self, ethers_core::abi::InvalidOutputType>
    where
        Self: Sized
    {
        let Token::Bytes(bytes) = token else {
            return Err(ethers_core::abi::InvalidOutputType("not bytes".to_string()))
        };
        let v = U256::from_big_endian(&bytes[0..31]).as_u64();
        let r = U256::from_big_endian(&bytes[32..63]);
        let s = U256::from_big_endian(&bytes[64..95]);

        Ok(Self(ESignature { v, s, r }))
    }
}

impl Deref for Signature {
    type Target = ESignature;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for Signature {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Encodable for Signature {
    fn encode(&self, out: &mut dyn open_fastrlp::BufMut) {
        open_fastrlp::Encodable::encode(&self.0, out);
    }

    fn length(&self) -> usize {
        open_fastrlp::Encodable::length(&self.0)
    }
}
impl Decodable for Signature {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let sig = open_fastrlp::Decodable::decode(buf)
            .map_err(|_| DecodeError::Custom("failed to decode sig"))?;

        Ok(Signature(sig))
    }
}

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("failed to decode signature: {0:#?}")]
    UnableToDecodeSignature(String),
    #[error("failed to decode signer: {0:#?}")]
    UnableToRecoverSigner(String),
    #[error("message doesn't match")]
    MessageDoesntMatch
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    RlpDecodable,
    RlpEncodable,
    ethers_contract::EthAbiType,
    ethers_contract::EthAbiCodec,
)]
pub struct BundleSignature {
    /// the signature of the bundle
    pub sig:  Signature,
    // hash of signed safe tx
    pub hash: H256
}

impl BundleSignature {
    pub fn recover_key(&self, safe_tx: &SafeTx) -> Result<PeerId, RecoveryError> {
        if self.hash != safe_tx.tx_hash() {
            return Err(RecoveryError::MessageDoesntMatch)
        }
        let sig = RecoverableSignature::from_compact(
            &self.sig.to_vec()[0..64],
            RecoveryId::from_i32(self.sig.to_vec()[64] as i32)
                .map_err(|e| RecoveryError::UnableToDecodeSignature(e.to_string()))?
        )
        .map_err(|err| RecoveryError::UnableToDecodeSignature(err.to_string()))?;

        trace!(?sig, "Validating Signature -- RECOVERING PUBLIC KEY");
        // secp256k1 public key
        SECP256K1
            .recover_ecdsa(
                &Message::from_slice(&self.hash[..32])
                    .map_err(|e| RecoveryError::UnableToRecoverSigner(e.to_string()))?,
                &sig
            )
            .map(|public_key| H512::from_slice(&public_key.serialize_uncompressed()[1..]))
            .map_err(|err| RecoveryError::UnableToRecoverSigner(err.to_string()))
    }
}
