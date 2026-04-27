use alloy_consensus::{
    Signed, TransactionEnvelope, TxEip1559, TxEip2930, TxEnvelope, TxLegacy, TxType, Typed2718,
    crypto::RecoveryError,
    transaction::{
        SignerRecoverable, TxEip7702, TxHashRef,
        eip4844::{TxEip4844Variant, TxEip4844WithSidecar},
    },
};
use alloy_evm::{FromRecoveredTx, FromTxWithEncoded};
use alloy_network::{AnyRpcTransaction, AnyTxEnvelope, TransactionResponse};
use alloy_primitives::{Address, B256, Bytes, TxHash};
use alloy_rpc_types::ConversionError;
use revm::context::TxEnv;
use tempo_primitives::{AASigned, TempoTransaction};
use tempo_revm::TempoTxEnv;

//
/// Container type for signed, typed transactions.
// NOTE(onbjerg): Boxing `Tempo(AASigned)` breaks `TransactionEnvelope` derive macro trait bounds.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, TransactionEnvelope)]
#[envelope(
    tx_type_name = FoundryTxType,
    typed = FoundryTypedTx,
)]
pub enum FoundryTxEnvelope {
    /// Legacy transaction type
    #[envelope(ty = 0)]
    Legacy(Signed<TxLegacy>),
    /// [EIP-2930] transaction.
    ///
    /// [EIP-2930]: https://eips.ethereum.org/EIPS/eip-2930
    #[envelope(ty = 1)]
    Eip2930(Signed<TxEip2930>),
    /// [EIP-1559] transaction.
    ///
    /// [EIP-1559]: https://eips.ethereum.org/EIPS/eip-1559
    #[envelope(ty = 2)]
    Eip1559(Signed<TxEip1559>),
    /// [EIP-4844] transaction.
    ///
    /// [EIP-4844]: https://eips.ethereum.org/EIPS/eip-4844
    #[envelope(ty = 3)]
    Eip4844(Signed<TxEip4844Variant>),
    /// [EIP-7702] transaction.
    ///
    /// [EIP-7702]: https://eips.ethereum.org/EIPS/eip-7702
    #[envelope(ty = 4)]
    Eip7702(Signed<TxEip7702>),
    /// Tempo transaction type.
    ///
    /// See <https://docs.tempo.xyz/protocol/transactions>.
    #[envelope(ty = 0x76, typed = TempoTransaction)]
    Tempo(AASigned),
}

impl FoundryTxEnvelope {
    /// Converts the transaction into an Ethereum [`TxEnvelope`].
    ///
    /// Returns an error if the transaction is not part of the standard Ethereum transaction types.
    pub fn try_into_eth(self) -> Result<TxEnvelope, Self> {
        match self {
            Self::Legacy(tx) => Ok(TxEnvelope::Legacy(tx)),
            Self::Eip2930(tx) => Ok(TxEnvelope::Eip2930(tx)),
            Self::Eip1559(tx) => Ok(TxEnvelope::Eip1559(tx)),
            Self::Eip4844(tx) => Ok(TxEnvelope::Eip4844(tx)),
            Self::Eip7702(tx) => Ok(TxEnvelope::Eip7702(tx)),
            Self::Tempo(_) => Err(self),
        }
    }

    pub const fn sidecar(&self) -> Option<&TxEip4844WithSidecar> {
        match self {
            Self::Eip4844(signed_variant) => match signed_variant.tx() {
                TxEip4844Variant::TxEip4844WithSidecar(with_sidecar) => Some(with_sidecar),
                _ => None,
            },
            _ => None,
        }
    }

    /// Returns the hash of the transaction.
    pub fn hash(&self) -> B256 {
        match self {
            Self::Legacy(t) => *t.hash(),
            Self::Eip2930(t) => *t.hash(),
            Self::Eip1559(t) => *t.hash(),
            Self::Eip4844(t) => *t.hash(),
            Self::Eip7702(t) => *t.hash(),
            Self::Tempo(t) => *t.hash(),
        }
    }

    /// Returns `true` if this is a Tempo transaction.
    pub const fn is_tempo(&self) -> bool {
        matches!(self, Self::Tempo(_))
    }

    /// Recovers the Ethereum address which was used to sign the transaction.
    pub fn recover(&self) -> Result<Address, RecoveryError> {
        Ok(match self {
            Self::Legacy(tx) => tx.recover_signer()?,
            Self::Eip2930(tx) => tx.recover_signer()?,
            Self::Eip1559(tx) => tx.recover_signer()?,
            Self::Eip4844(tx) => tx.recover_signer()?,
            Self::Eip7702(tx) => tx.recover_signer()?,
            Self::Tempo(tx) => tx.signature().recover_signer(&tx.signature_hash())?,
        })
    }
}

impl TxHashRef for FoundryTxEnvelope {
    fn tx_hash(&self) -> &TxHash {
        match self {
            Self::Legacy(t) => t.hash(),
            Self::Eip2930(t) => t.hash(),
            Self::Eip1559(t) => t.hash(),
            Self::Eip4844(t) => t.hash(),
            Self::Eip7702(t) => t.hash(),
            Self::Tempo(t) => t.hash(),
        }
    }
}

impl SignerRecoverable for FoundryTxEnvelope {
    fn recover_signer(&self) -> Result<Address, RecoveryError> {
        self.recover()
    }

    fn recover_signer_unchecked(&self) -> Result<Address, RecoveryError> {
        self.recover()
    }
}

impl TryFrom<FoundryTxEnvelope> for TxEnvelope {
    type Error = FoundryTxEnvelope;

    fn try_from(envelope: FoundryTxEnvelope) -> Result<Self, Self::Error> {
        envelope.try_into_eth()
    }
}

impl From<TxEnvelope> for FoundryTxEnvelope {
    fn from(tx: TxEnvelope) -> Self {
        match tx {
            TxEnvelope::Legacy(tx) => Self::Legacy(tx),
            TxEnvelope::Eip2930(tx) => Self::Eip2930(tx),
            TxEnvelope::Eip1559(tx) => Self::Eip1559(tx),
            TxEnvelope::Eip4844(tx) => Self::Eip4844(tx),
            TxEnvelope::Eip7702(tx) => Self::Eip7702(tx),
        }
    }
}

impl From<tempo_primitives::TempoTxEnvelope> for FoundryTxEnvelope {
    fn from(tx: tempo_primitives::TempoTxEnvelope) -> Self {
        match tx {
            tempo_primitives::TempoTxEnvelope::Legacy(tx) => Self::Legacy(tx),
            tempo_primitives::TempoTxEnvelope::Eip2930(tx) => Self::Eip2930(tx),
            tempo_primitives::TempoTxEnvelope::Eip1559(tx) => Self::Eip1559(tx),
            tempo_primitives::TempoTxEnvelope::Eip7702(tx) => Self::Eip7702(tx),
            tempo_primitives::TempoTxEnvelope::AA(tx) => Self::Tempo(tx),
        }
    }
}

impl TryFrom<AnyRpcTransaction> for FoundryTxEnvelope {
    type Error = ConversionError;

    fn try_from(value: AnyRpcTransaction) -> Result<Self, Self::Error> {
        let transaction = value.into_inner();
        let _from = transaction.from();
        match transaction.into_inner() {
            AnyTxEnvelope::Ethereum(tx) => match tx {
                TxEnvelope::Legacy(tx) => Ok(Self::Legacy(tx)),
                TxEnvelope::Eip2930(tx) => Ok(Self::Eip2930(tx)),
                TxEnvelope::Eip1559(tx) => Ok(Self::Eip1559(tx)),
                TxEnvelope::Eip4844(tx) => Ok(Self::Eip4844(tx)),
                TxEnvelope::Eip7702(tx) => Ok(Self::Eip7702(tx)),
            },
            AnyTxEnvelope::Unknown(tx) => {
                let tx_type = tx.ty();
                Err(ConversionError::Custom(format!("Unknown transaction type: 0x{tx_type:02X}")))
            }
        }
    }
}

impl FromRecoveredTx<FoundryTxEnvelope> for TxEnv {
    fn from_recovered_tx(tx: &FoundryTxEnvelope, caller: Address) -> Self {
        match tx {
            FoundryTxEnvelope::Legacy(signed_tx) => Self::from_recovered_tx(signed_tx, caller),
            FoundryTxEnvelope::Eip2930(signed_tx) => Self::from_recovered_tx(signed_tx, caller),
            FoundryTxEnvelope::Eip1559(signed_tx) => Self::from_recovered_tx(signed_tx, caller),
            FoundryTxEnvelope::Eip4844(signed_tx) => Self::from_recovered_tx(signed_tx, caller),
            FoundryTxEnvelope::Eip7702(signed_tx) => Self::from_recovered_tx(signed_tx, caller),
            FoundryTxEnvelope::Tempo(_) => unreachable!("Tempo tx in Ethereum context"),
        }
    }
}

impl FromTxWithEncoded<FoundryTxEnvelope> for TxEnv {
    fn from_encoded_tx(tx: &FoundryTxEnvelope, sender: Address, _encoded: Bytes) -> Self {
        Self::from_recovered_tx(tx, sender)
    }
}

impl FromRecoveredTx<FoundryTxEnvelope> for TempoTxEnv {
    fn from_recovered_tx(tx: &FoundryTxEnvelope, caller: Address) -> Self {
        match tx {
            FoundryTxEnvelope::Legacy(signed_tx) => {
                Self::from(TxEnv::from_recovered_tx(signed_tx, caller))
            }
            FoundryTxEnvelope::Eip2930(signed_tx) => {
                Self::from(TxEnv::from_recovered_tx(signed_tx, caller))
            }
            FoundryTxEnvelope::Eip1559(signed_tx) => {
                Self::from(TxEnv::from_recovered_tx(signed_tx, caller))
            }
            FoundryTxEnvelope::Eip4844(signed_tx) => {
                Self::from(TxEnv::from_recovered_tx(signed_tx, caller))
            }
            FoundryTxEnvelope::Eip7702(signed_tx) => {
                Self::from(TxEnv::from_recovered_tx(signed_tx, caller))
            }
            FoundryTxEnvelope::Tempo(aa_signed) => Self::from_recovered_tx(aa_signed, caller),
        }
    }
}

impl FromTxWithEncoded<FoundryTxEnvelope> for TempoTxEnv {
    fn from_encoded_tx(tx: &FoundryTxEnvelope, sender: Address, _encoded: Bytes) -> Self {
        Self::from_recovered_tx(tx, sender)
    }
}

impl std::fmt::Display for FoundryTxType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Legacy => write!(f, "legacy"),
            Self::Eip2930 => write!(f, "eip2930"),
            Self::Eip1559 => write!(f, "eip1559"),
            Self::Eip4844 => write!(f, "eip4844"),
            Self::Eip7702 => write!(f, "eip7702"),
            Self::Tempo => write!(f, "tempo"),
        }
    }
}

impl From<TxType> for FoundryTxType {
    fn from(tx: TxType) -> Self {
        match tx {
            TxType::Legacy => Self::Legacy,
            TxType::Eip2930 => Self::Eip2930,
            TxType::Eip1559 => Self::Eip1559,
            TxType::Eip4844 => Self::Eip4844,
            TxType::Eip7702 => Self::Eip7702,
        }
    }
}

impl From<FoundryTxEnvelope> for FoundryTypedTx {
    fn from(envelope: FoundryTxEnvelope) -> Self {
        match envelope {
            FoundryTxEnvelope::Legacy(signed_tx) => Self::Legacy(signed_tx.strip_signature()),
            FoundryTxEnvelope::Eip2930(signed_tx) => Self::Eip2930(signed_tx.strip_signature()),
            FoundryTxEnvelope::Eip1559(signed_tx) => Self::Eip1559(signed_tx.strip_signature()),
            FoundryTxEnvelope::Eip4844(signed_tx) => Self::Eip4844(signed_tx.strip_signature()),
            FoundryTxEnvelope::Eip7702(signed_tx) => Self::Eip7702(signed_tx.strip_signature()),
            FoundryTxEnvelope::Tempo(signed_tx) => Self::Tempo(signed_tx.strip_signature()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy_primitives::{TxKind, U256, b256, hex};
    use alloy_rlp::Decodable;
    use alloy_signer::Signature;

    use super::*;

    #[test]
    fn test_decode_call() {
        let bytes_first = &mut &hex::decode("f86b02843b9aca00830186a094d3e8763675e4c425df46cc3b5c0f6cbdac39604687038d7ea4c68000802ba00eb96ca19e8a77102767a41fc85a36afd5c61ccb09911cec5d3e86e193d9c5aea03a456401896b1b6055311536bf00a718568c744d8c1f9df59879e8350220ca18").unwrap()[..];
        let decoded = FoundryTxEnvelope::decode(&mut &bytes_first[..]).unwrap();

        let tx = TxLegacy {
            nonce: 2u64,
            gas_price: 1000000000u128,
            gas_limit: 100000,
            to: TxKind::Call(Address::from_slice(
                &hex::decode("d3e8763675e4c425df46cc3b5c0f6cbdac396046").unwrap()[..],
            )),
            value: U256::from(1000000000000000u64),
            input: Bytes::default(),
            chain_id: Some(4),
        };

        let signature = Signature::from_str("0eb96ca19e8a77102767a41fc85a36afd5c61ccb09911cec5d3e86e193d9c5ae3a456401896b1b6055311536bf00a718568c744d8c1f9df59879e8350220ca182b").unwrap();

        let tx = FoundryTxEnvelope::Legacy(Signed::new_unchecked(
            tx,
            signature,
            b256!("0xa517b206d2223278f860ea017d3626cacad4f52ff51030dc9a96b432f17f8d34"),
        ));

        assert_eq!(tx, decoded);
    }

    #[test]
    fn can_recover_sender() {
        let bytes = hex::decode("02f872018307910d808507204d2cb1827d0094388c818ca8b9251b393131c08a736a67ccb19297880320d04823e2701c80c001a0cf024f4815304df2867a1a74e9d2707b6abda0337d2d54a4438d453f4160f190a07ac0e6b3bc9395b5b9c8b9e6d77204a236577a5b18467b9175c01de4faa208d9").unwrap();

        let Ok(FoundryTxEnvelope::Eip1559(tx)) = FoundryTxEnvelope::decode(&mut &bytes[..]) else {
            panic!("decoding FoundryTxEnvelope failed");
        };

        assert_eq!(
            tx.hash(),
            &"0x86718885c4b4218c6af87d3d0b0d83e3cc465df2a05c048aa4db9f1a6f9de91f"
                .parse::<B256>()
                .unwrap()
        );
        assert_eq!(
            tx.recover_signer().unwrap(),
            "0x95222290DD7278Aa3Ddd389Cc1E1d165CC4BAfe5".parse::<Address>().unwrap()
        );
    }

    #[test]
    fn deser_to_type_tx() {
        let tx = r#"
        {
            "type": "0x2",
            "chainId": "0x7a69",
            "nonce": "0x0",
            "gas": "0x5209",
            "maxFeePerGas": "0x77359401",
            "maxPriorityFeePerGas": "0x1",
            "to": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            "value": "0x0",
            "accessList": [],
            "input": "0x",
            "r": "0x85c2794a580da137e24ccc823b45ae5cea99371ae23ee13860fcc6935f8305b0",
            "s": "0x41de7fa4121dab284af4453d30928241208bafa90cdb701fe9bc7054759fe3cd",
            "yParity": "0x0",
            "hash": "0x8c9b68e8947ace33028dba167354fde369ed7bbe34911b772d09b3c64b861515"
        }"#;

        let _typed_tx: FoundryTxEnvelope = serde_json::from_str(tx).unwrap();
    }
}
