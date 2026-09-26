use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, Generate, Key, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};

const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("could not read or create the encryption key at {}: {source}", path.display())]
    Key { path: PathBuf, source: io::Error },
    #[error("a saved secret could not be decrypted; save it again")]
    Unreadable,
}

/// Encrypts secrets, such as API keys, before they're stored in the database.
pub struct Vault(ChaCha20Poly1305);

impl Vault {
    /// The key lives in its own file next to the database rather than in it, so a copy of the
    /// database alone doesn't reveal the secrets. The file is created on first use.
    pub fn beside(db: &Path) -> Result<Self, VaultError> {
        let mut path = db.as_os_str().to_owned();
        path.push(".key");
        let path = PathBuf::from(path);
        let key = load_or_create_key(&path).map_err(|source| VaultError::Key {
            path: path.clone(),
            source,
        })?;
        Ok(Self(ChaCha20Poly1305::new(&key)))
    }

    /// A fresh random nonce, followed by the ciphertext.
    pub fn seal(&self, secret: &str) -> Vec<u8> {
        let nonce = Nonce::generate();
        let ciphertext = self
            .0
            .encrypt(&nonce, secret.as_bytes())
            .expect("encrypting in memory cannot fail");
        [nonce.as_slice(), &ciphertext].concat()
    }

    pub fn open(&self, sealed: &[u8]) -> Result<String, VaultError> {
        if sealed.len() < NONCE_LEN {
            return Err(VaultError::Unreadable);
        }
        let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce).map_err(|_| VaultError::Unreadable)?;
        let plaintext = self
            .0
            .decrypt(&nonce, ciphertext)
            .map_err(|_| VaultError::Unreadable)?;
        String::from_utf8(plaintext).map_err(|_| VaultError::Unreadable)
    }
}

fn load_or_create_key(path: &Path) -> io::Result<Key<ChaCha20Poly1305>> {
    match fs::read(path) {
        Ok(bytes) => Key::<ChaCha20Poly1305>::try_from(bytes.as_slice()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "the key file is not a valid key",
            )
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let key = Key::<ChaCha20Poly1305>::generate();
            write_private(path, &key)?;
            Ok(key)
        }
        Err(error) => Err(error),
    }
}

/// `create_new` so an existing key is never replaced, which would make saved secrets unreadable.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(bytes)
}
