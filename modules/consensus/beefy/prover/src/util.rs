// Copyright (C) 2022 Polytope Labs.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use anyhow::anyhow;
use beefy_verifier_primitives::{Hash, SignatureWithAuthorityIndex};
use codec::{Compact, Decode, Encode};
use frame_support::sp_runtime::traits::Convert;
use rs_merkle::MerkleTree;
use sp_io::hashing::keccak_256;
use sp_runtime::traits::Keccak256;
use sp_trie::{LayoutV0, Recorder, Trie, TrieDBBuilder, TrieDBMutBuilder, TrieMut};
use std::collections::HashSet;
use subxt::{Config, OnlineClient};

/// Holds the timestamp inherent alongside a merkle-patricia trie proof of its existence in a given
/// block.
pub struct TimeStampExtWithProof {
    /// The timestamp inherent SCALE-encoded bytes. Decode with [`UncheckedExtrinsic`]
    pub ext: Vec<u8>,
    /// Merkle-patricia trie existence proof for the extrinsic, this is generated by the relayer.
    pub proof: Vec<Vec<u8>>,
}

/// This holds the signatures of a BEEFY commitment, along side a merkle multi-proof of the
/// existence of the ethereum addresses associated with the signatures.
pub struct AuthorityProofWithSignatures {
    /// Merkle multi-proof
    pub authority_proof: Vec<Vec<(usize, Hash)>>,
    /// The actual signatures alongside the authority index, used in verifying the merkle proof.
    pub signatures: Vec<SignatureWithAuthorityIndex>,
}

/// This holds the proof that a parachain header was included in the parachain heads root (extra
/// data) field in an mmr leaf.
pub struct ParaHeadsProof {
    /// Merkle proof of existence of parachain header
    pub parachain_heads_proof: Vec<Hash>,
    /// This is the actual parachain header, SCALE-encoded
    pub para_head: Vec<u8>,
    /// This is the index of the parachain header in the parachain heads tree.
    pub heads_leaf_index: u32,
    /// This is the total count of parachain headers in the parachain header tree.
    pub heads_total_count: u32,
}

/// Fetch timestamp extrinsic and it's proof
pub async fn fetch_timestamp_extrinsic_with_proof<T: Config>(
    client: &OnlineClient<T>,
    block_hash: Option<T::Hash>,
) -> Result<TimeStampExtWithProof, anyhow::Error> {
    let block = client.rpc().block(block_hash).await?.ok_or_else(|| {
        anyhow!("[get_parachain_headers] Block with hash :{block_hash:?} not found",)
    })?;

    let extrinsics = block.block.extrinsics.into_iter().map(|e| e.0.encode()).collect::<Vec<_>>();

    let (ext, proof) = {
        if extrinsics.is_empty() {
            return Err(anyhow!("Block has no extrinsics"));
        }
        let timestamp_ext = extrinsics[0].clone();
        let mut db = sp_trie::MemoryDB::<Keccak256>::default();

        let root = {
            let mut root = Default::default();
            let mut trie = <TrieDBMutBuilder<LayoutV0<Keccak256>>>::new(&mut db, &mut root).build();

            for (i, ext) in extrinsics.into_iter().enumerate() {
                let key = Compact(i as u32).encode();
                trie.insert(&key, &ext)?;
            }
            *trie.root()
        };

        let key = Compact::<u32>(0u32).encode();
        let proof = {
            let mut recorder = Recorder::<LayoutV0<Keccak256>>::new();
            let triedb = TrieDBBuilder::<LayoutV0<Keccak256>>::new(&db, &root)
                .with_recorder(&mut recorder)
                .build();
            triedb.get(&key).unwrap().unwrap();
            recorder
                .drain()
                .into_iter()
                .map(|f| f.data)
                .collect::<HashSet<_>>() // dedupe nodes
                .into_iter()
                .collect::<Vec<_>>()
        };

        (timestamp_ext, proof)
    };

    Ok(TimeStampExtWithProof { ext, proof })
}

/// Get the proof for authority set that signed this commitment
pub fn prove_authority_set(
    signed_commitment: &sp_consensus_beefy::SignedCommitment<
        u32,
        sp_consensus_beefy::ecdsa_crypto::Signature,
    >,
    authority_address_hashes: Vec<Hash>,
) -> Result<AuthorityProofWithSignatures, anyhow::Error> {
    let signatures = signed_commitment
        .signatures
        .iter()
        .enumerate()
        .map(|(index, x)| {
            if let Some(sig) = x {
                let mut temp = [0u8; 65];
                if sig.len() == 65 {
                    temp.copy_from_slice(&*sig.encode());
                    let last = temp.last_mut().unwrap();
                    *last = *last + 27;
                    Some(SignatureWithAuthorityIndex { index: index as u32, signature: temp })
                } else {
                    None
                }
            } else {
                None
            }
        })
        .filter_map(|x| x)
        .collect::<Vec<_>>();

    let signature_indices = signatures.iter().map(|x| x.index as usize).collect::<Vec<_>>();
    let authority_proof = merkle_proof(&authority_address_hashes, &signature_indices);

    Ok(AuthorityProofWithSignatures { authority_proof, signatures })
}

/// Hash encoded authority public keys
pub fn hash_authority_addresses(
    encoded_public_keys: Vec<Vec<u8>>,
) -> Result<Vec<Hash>, anyhow::Error> {
    let authority_address_hashes = encoded_public_keys
        .into_iter()
        .map(|x| {
            sp_consensus_beefy::ecdsa_crypto::AuthorityId::decode(&mut &*x)
                .map(|id| keccak_256(&pallet_beefy_mmr::BeefyEcdsaToEthereum::convert(id)))
        })
        .collect::<Result<Vec<_>, codec::Error>>()?;
    Ok(authority_address_hashes)
}

/// Merkle Hasher for mmr library
#[derive(Clone)]
pub struct MerkleHasher;

impl rs_merkle::Hasher for MerkleHasher {
    type Hash = Hash;
    fn hash(data: &[u8]) -> Self::Hash {
        keccak_256(data)
    }
}

/// Generates a 2D-merkle proof for the given leaves & indices
pub fn merkle_proof(leaves: &[Hash], indices: &[usize]) -> Vec<Vec<(usize, Hash)>> {
    let tree = MerkleTree::<MerkleHasher>::from_leaves(leaves);

    tree.proof_2d(indices)
}
