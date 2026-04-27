//! EVM hardfork definitions for Foundry.
//!
//! Provides [`FoundryHardfork`], a unified enum over Ethereum and Tempo hardforks
//! with `FromStr`/`Serialize`/`Deserialize` support for CLI and config usage.

use std::str::FromStr;

use alloy_chains::Chain;
use alloy_rpc_types::BlockNumberOrTag;
use foundry_compilers::artifacts::EvmVersion;
use revm::primitives::hardfork::SpecId;
use serde::{Deserialize, Serialize};

pub use alloy_hardforks::EthereumHardfork;
pub use tempo_chainspec::hardfork::TempoHardfork;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(into = "String")]
pub enum FoundryHardfork {
    Ethereum(EthereumHardfork),
    Tempo(TempoHardfork),
}

impl From<FoundryHardfork> for String {
    fn from(fork: FoundryHardfork) -> Self {
        match fork {
            FoundryHardfork::Ethereum(h) => format!("{h}"),
            FoundryHardfork::Tempo(h) => format!("tempo:{h}"),
        }
    }
}

impl<'de> Deserialize<'de> for FoundryHardfork {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl FromStr for FoundryHardfork {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let raw = s.trim();

        let Some((ns, fork_raw)) = raw.split_once(':') else {
            return EthereumHardfork::from_str(raw)
                .map(Self::Ethereum)
                .map_err(|_| format!("unknown ethereum hardfork '{raw}'"));
        };

        let ns = ns.trim().to_ascii_lowercase();
        let fork = fork_raw.trim().to_ascii_lowercase().replace(['-', ' '], "_");

        match ns.as_str() {
            "eth" | "ethereum" => EthereumHardfork::from_str(&fork)
                .map(Self::Ethereum)
                .map_err(|_| format!("unknown ethereum hardfork '{fork_raw}'")),

            "t" | "tempo" => TempoHardfork::from_str(&fork)
                .map(Self::Tempo)
                .map_err(|_| format!("unknown tempo hardfork '{fork_raw}'")),
            _ => EthereumHardfork::from_str(&fork)
                .map(Self::Ethereum)
                .map_err(|_| format!("unknown hardfork '{raw}'")),
        }
    }
}

impl FoundryHardfork {
    pub const fn ethereum(h: EthereumHardfork) -> Self {
        Self::Ethereum(h)
    }

    pub const fn tempo(h: TempoHardfork) -> Self {
        Self::Tempo(h)
    }

    /// Returns the hardfork name without a network namespace prefix.
    pub fn name(&self) -> String {
        match self {
            Self::Ethereum(h) => format!("{h}"),
            Self::Tempo(h) => format!("{h}"),
        }
    }

    /// Returns the network namespace for this hardfork, or `None` for plain Ethereum.
    pub const fn namespace(&self) -> Option<&'static str> {
        match self {
            Self::Ethereum(_) => None,
            Self::Tempo(_) => Some("tempo"),
        }
    }

    /// Auto-detect the active hardfork for a given chain at a specific timestamp.
    ///
    /// For Ethereum chains, walks `EthereumHardfork::from_chain_and_timestamp`.
    /// For known OP-stack chains (OP mainnet, Base, Zora, World), maps to a conservative
    /// Ethereum spec (Cancun before ~May 2025, Prague after) so that OP RPC forks still
    /// resolve a usable spec for replay of regular user txs.
    pub fn from_chain_and_timestamp(chain_id: u64, timestamp: u64) -> Option<Self> {
        let chain = Chain::from_id(chain_id);
        if let Some(fork) = EthereumHardfork::from_chain_and_timestamp(chain, timestamp) {
            return Some(Self::Ethereum(fork));
        }
        // Known OP-stack chains: 10 = OP mainnet, 8453 = Base, 7777777 = Zora, 480 = World.
        // Map them to a conservative Ethereum spec by timestamp so plain user-tx replay works.
        match chain_id {
            10 | 8453 | 7777777 | 480 => {
                // Prague mainnet activation: 1746612311 (May 7 2025). Use a conservative cutoff.
                let prague_cutoff: u64 = 1_746_000_000;
                let fork = if timestamp >= prague_cutoff {
                    EthereumHardfork::Prague
                } else {
                    EthereumHardfork::Cancun
                };
                Some(Self::Ethereum(fork))
            }
            _ => None,
        }
    }
}

impl From<EthereumHardfork> for FoundryHardfork {
    fn from(value: EthereumHardfork) -> Self {
        Self::Ethereum(value)
    }
}

impl From<FoundryHardfork> for EthereumHardfork {
    fn from(fork: FoundryHardfork) -> Self {
        match fork {
            FoundryHardfork::Ethereum(hardfork) => hardfork,
            _ => Self::default(),
        }
    }
}

impl From<TempoHardfork> for FoundryHardfork {
    fn from(value: TempoHardfork) -> Self {
        Self::Tempo(value)
    }
}

impl From<FoundryHardfork> for TempoHardfork {
    fn from(fork: FoundryHardfork) -> Self {
        match fork {
            FoundryHardfork::Tempo(hardfork) => hardfork,
            _ => Self::default(),
        }
    }
}

impl From<FoundryHardfork> for SpecId {
    fn from(fork: FoundryHardfork) -> Self {
        match fork {
            FoundryHardfork::Ethereum(hardfork) => spec_id_from_ethereum_hardfork(hardfork),
            FoundryHardfork::Tempo(hardfork) => hardfork.into(),
        }
    }
}

/// Map an `EthereumHardfork` enum into its corresponding `SpecId`.
pub fn spec_id_from_ethereum_hardfork(hardfork: EthereumHardfork) -> SpecId {
    match hardfork {
        EthereumHardfork::Frontier => SpecId::FRONTIER,
        EthereumHardfork::Homestead => SpecId::HOMESTEAD,
        EthereumHardfork::Dao => SpecId::DAO_FORK,
        EthereumHardfork::Tangerine => SpecId::TANGERINE,
        EthereumHardfork::SpuriousDragon => SpecId::SPURIOUS_DRAGON,
        EthereumHardfork::Byzantium => SpecId::BYZANTIUM,
        EthereumHardfork::Constantinople => SpecId::CONSTANTINOPLE,
        EthereumHardfork::Petersburg => SpecId::PETERSBURG,
        EthereumHardfork::Istanbul => SpecId::ISTANBUL,
        EthereumHardfork::MuirGlacier => SpecId::MUIR_GLACIER,
        EthereumHardfork::Berlin => SpecId::BERLIN,
        EthereumHardfork::London => SpecId::LONDON,
        EthereumHardfork::ArrowGlacier => SpecId::ARROW_GLACIER,
        EthereumHardfork::GrayGlacier => SpecId::GRAY_GLACIER,
        EthereumHardfork::Paris => SpecId::MERGE,
        EthereumHardfork::Shanghai => SpecId::SHANGHAI,
        EthereumHardfork::Cancun => SpecId::CANCUN,
        EthereumHardfork::Prague => SpecId::PRAGUE,
        EthereumHardfork::Osaka => SpecId::OSAKA,
        EthereumHardfork::Bpo1 | EthereumHardfork::Bpo2 => SpecId::OSAKA,
        EthereumHardfork::Bpo3 | EthereumHardfork::Bpo4 | EthereumHardfork::Bpo5 => {
            unimplemented!()
        }
        f => unreachable!("unimplemented {}", f),
    }
}

/// Trait for converting an [`EvmVersion`] into a network-specific spec type.
pub trait FromEvmVersion: From<FoundryHardfork> {
    fn from_evm_version(version: EvmVersion) -> Self;
}

impl FromEvmVersion for SpecId {
    fn from_evm_version(version: EvmVersion) -> Self {
        match version {
            EvmVersion::Homestead => Self::HOMESTEAD,
            EvmVersion::TangerineWhistle => Self::TANGERINE,
            EvmVersion::SpuriousDragon => Self::SPURIOUS_DRAGON,
            EvmVersion::Byzantium => Self::BYZANTIUM,
            EvmVersion::Constantinople => Self::CONSTANTINOPLE,
            EvmVersion::Petersburg => Self::PETERSBURG,
            EvmVersion::Istanbul => Self::ISTANBUL,
            EvmVersion::Berlin => Self::BERLIN,
            EvmVersion::London => Self::LONDON,
            EvmVersion::Paris => Self::MERGE,
            EvmVersion::Shanghai => Self::SHANGHAI,
            EvmVersion::Cancun => Self::CANCUN,
            EvmVersion::Prague => Self::PRAGUE,
            EvmVersion::Osaka => Self::OSAKA,
        }
    }
}

impl FromEvmVersion for TempoHardfork {
    fn from_evm_version(_: EvmVersion) -> Self {
        Self::default()
    }
}

/// Returns the spec id derived from [`EvmVersion`] for a given spec type.
pub fn evm_spec_id<SPEC: FromEvmVersion>(evm_version: EvmVersion) -> SPEC {
    SPEC::from_evm_version(evm_version)
}

/// Convert a `BlockNumberOrTag` into an `EthereumHardfork`.
pub fn ethereum_hardfork_from_block_tag(block: impl Into<BlockNumberOrTag>) -> EthereumHardfork {
    let num = match block.into() {
        BlockNumberOrTag::Earliest => 0,
        BlockNumberOrTag::Number(num) => num,
        _ => u64::MAX,
    };

    EthereumHardfork::from_mainnet_block_number(num)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_hardforks::ethereum::mainnet::*;

    #[test]
    fn test_ethereum_spec_id_mapping() {
        assert_eq!(spec_id_from_ethereum_hardfork(EthereumHardfork::Frontier), SpecId::FRONTIER);
        assert_eq!(spec_id_from_ethereum_hardfork(EthereumHardfork::Homestead), SpecId::HOMESTEAD);
        assert_eq!(spec_id_from_ethereum_hardfork(EthereumHardfork::Cancun), SpecId::CANCUN);
        assert_eq!(spec_id_from_ethereum_hardfork(EthereumHardfork::Prague), SpecId::PRAGUE);
        assert_eq!(spec_id_from_ethereum_hardfork(EthereumHardfork::Osaka), SpecId::OSAKA);
    }

    #[test]
    fn test_tempo_spec_id_mapping() {
        assert_eq!(SpecId::from(TempoHardfork::Genesis), SpecId::OSAKA);
    }

    #[test]
    fn test_hardfork_from_block_tag_numbers() {
        assert_eq!(
            ethereum_hardfork_from_block_tag(MAINNET_HOMESTEAD_BLOCK - 1),
            EthereumHardfork::Frontier
        );
        assert_eq!(
            ethereum_hardfork_from_block_tag(MAINNET_LONDON_BLOCK + 1),
            EthereumHardfork::London
        );
    }

    #[test]
    fn test_from_chain_and_timestamp_ethereum_mainnet() {
        assert_eq!(
            FoundryHardfork::from_chain_and_timestamp(1, 0),
            Some(FoundryHardfork::Ethereum(EthereumHardfork::Frontier))
        );
    }

    #[test]
    fn test_from_chain_and_timestamp_op_mainnet_maps_to_eth() {
        // OP chains map to a conservative Ethereum spec for plain-tx replay.
        assert!(matches!(
            FoundryHardfork::from_chain_and_timestamp(10, u64::MAX),
            Some(FoundryHardfork::Ethereum(_))
        ));
    }

    #[test]
    fn test_from_chain_and_timestamp_base_maps_to_eth() {
        assert!(matches!(
            FoundryHardfork::from_chain_and_timestamp(8453, u64::MAX),
            Some(FoundryHardfork::Ethereum(_))
        ));
    }

    #[test]
    fn test_from_chain_and_timestamp_unknown_chain() {
        assert_eq!(FoundryHardfork::from_chain_and_timestamp(999999, 0), None);
    }
}
