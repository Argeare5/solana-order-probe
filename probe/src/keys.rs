//! Keypairs in the Solana CLI format (a JSON array of 64 bytes). They live in the secrets volume and never in the
//! repository or the logs.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use solana_keypair::Keypair;
use solana_signer::Signer;

pub fn load(path: &Path) -> Result<Keypair> {
    let text = fs::read_to_string(path).with_context(|| format!("reading keypair {}", path.display()))?;
    let bytes: Vec<u8> = serde_json::from_str(&text).with_context(|| format!("parsing keypair {}", path.display()))?;
    if bytes.len() != 64 {
        bail!("{}: expected 64 bytes, got {}", path.display(), bytes.len());
    }
    Keypair::try_from(bytes.as_slice()).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

pub fn save(path: &Path, keypair: &Keypair) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let bytes = keypair.to_bytes().to_vec();
    fs::write(path, serde_json::to_vec(&bytes)?).with_context(|| format!("writing keypair {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// The pool of fee payers: one payer per probe of a burst, so that probes never conflict on the payer account.
pub struct PayerPool {
    pub payers: Vec<Keypair>,
}

impl PayerPool {
    pub fn dir(secrets: &Path) -> PathBuf {
        secrets.join("payers")
    }

    /// Loads `count` payers, creating the missing ones.
    pub fn load_or_create(secrets: &Path, count: usize) -> Result<Self> {
        let dir = Self::dir(secrets);
        let mut payers = Vec::with_capacity(count);
        for i in 0..count {
            let path = dir.join(format!("payer-{i:02}.json"));
            let keypair = if path.exists() {
                load(&path)?
            } else {
                let keypair = Keypair::new();
                save(&path, &keypair)?;
                keypair
            };
            payers.push(keypair);
        }
        Ok(Self { payers })
    }

    pub fn pubkeys(&self) -> Vec<String> {
        self.payers.iter().map(|k| k.pubkey().to_string()).collect()
    }
}
