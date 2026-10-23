//! Probe transactions. Every probe has the same shape, so the leader's cost model gives every probe the same cost
//! and only the compute unit price differs:
//!
//! 1. `SetComputeUnitLimit(cu_limit)`, the same for all probes;
//! 2. `SetComputeUnitPrice(cu_price)`, the variable under test;
//! 3. a system transfer of 0 lamports from the payer to `target` (a fresh address per probe, or one shared address
//!    for the probes of the conflict variant, which then all write-lock it);
//! 4. a memo with the probe tag, so the probe is recognisable in the block.

use anyhow::Result;
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_hash::Hash;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_signer::Signer;
use solana_transaction::Transaction;

/// SPL Memo v1. Memo v2 logs the memo and checks signers, which costs ~11 100 CU on mainnet (`simulate`, 23.10.2026);
/// v1 keeps the tag in the instruction data for ~360 CU, so the whole probe needs ~810 CU and the limit stays small.
pub const MEMO_PROGRAM: Pubkey = Pubkey::from_str_const("Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo");

pub struct ProbeSpec {
    pub tag: String,
    pub cu_limit: u32,
    pub cu_price: u64,
    pub target: Pubkey,
}

pub struct BuiltProbe {
    pub signature: Signature,
    pub wire: Vec<u8>,
}

pub fn instructions(payer: &Pubkey, spec: &ProbeSpec) -> Vec<Instruction> {
    vec![
        ComputeBudgetInstruction::set_compute_unit_limit(spec.cu_limit),
        ComputeBudgetInstruction::set_compute_unit_price(spec.cu_price),
        solana_system_interface::instruction::transfer(payer, &spec.target, 0),
        Instruction { program_id: MEMO_PROGRAM, accounts: vec![], data: spec.tag.as_bytes().to_vec() },
    ]
}

pub fn build(payer: &Keypair, spec: &ProbeSpec, blockhash: Hash) -> Result<BuiltProbe> {
    let tx = Transaction::new_signed_with_payer(&instructions(&payer.pubkey(), spec), Some(&payer.pubkey()), &[payer], blockhash);
    let wire = wincode::serialize(&tx)?;
    Ok(BuiltProbe { signature: tx.signatures[0], wire })
}

/// Fees a probe can cost at most, in lamports: the base fee of its one signature plus its priority fee.
pub fn max_fee_lamports(spec: &ProbeSpec) -> u64 {
    const LAMPORTS_PER_SIGNATURE: u64 = 5_000;
    LAMPORTS_PER_SIGNATURE + (spec.cu_limit as u64 * spec.cu_price).div_ceil(1_000_000)
}
