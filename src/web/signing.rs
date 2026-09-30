//! Signed updates: every Aurion binary installed on the Pi (downloaded from
//! GitHub or sent from the phone) must carry an Ed25519 signature made with
//! the project's release key.
//!
//! Two public keys are built in (`keys/`): the release key, whose private half
//! signs every release in the Release workflow (GitHub environment "release",
//! approved by hand), and a backup key kept offline. If the release key is
//! lost or leaked, a version signed with the backup key replaces it: a closed
//! camera box never needs to be opened for that.
//!
//! Signature file: the raw 64-byte signature of the whole binary, as written
//! by `openssl pkeyutl -sign -rawin -inkey <key> -in aurion-arm64 -out aurion-arm64.sig`.

use ed25519_compact::{PublicKey, Signature};

/// Name of the signature published next to the binary.
pub const SIGNATURE_ASSET: &str = "aurion-arm64.sig";

/// Public keys trusted in production (PEM), release key first.
pub const RELEASE_KEYS: [&str; 2] = [
    include_str!("../../keys/aurion-signing.pub"),
    include_str!("../../keys/aurion-secours.pub"),
];

/// The built-in keys, parsed.
pub fn release_keys() -> Vec<PublicKey> {
    RELEASE_KEYS.iter().filter_map(|pem| PublicKey::from_pem(pem).ok()).collect()
}

/// `data` was signed by one of `keys`. The error explains the refusal in
/// French for the Diagnostics page.
pub fn verify(data: &[u8], signature: &[u8], keys: &[PublicKey]) -> Result<(), String> {
    if signature.is_empty() {
        return Err("Signature absente : seules les versions signées du projet sont installées".into());
    }
    let sig = Signature::from_slice(signature)
        .map_err(|_| "Fichier de signature invalide (64 octets attendus) : mise à jour refusée".to_string())?;
    if keys.iter().any(|k| k.verify(data, &sig).is_ok()) {
        Ok(())
    } else {
        Err("Signature invalide : ce binaire n'a pas été publié par le projet, mise à jour refusée".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_compact::{KeyPair, Seed};

    fn key(n: u8) -> KeyPair {
        KeyPair::from_seed(Seed::new([n; 32]))
    }

    #[test]
    fn both_built_in_keys_are_valid() {
        assert_eq!(release_keys().len(), 2, "keys/aurion-signing.pub and keys/aurion-secours.pub");
    }

    #[test]
    fn signature_made_by_openssl_is_verified() {
        // Written exactly as the Release workflow does it:
        // openssl pkeyutl -sign -rawin -inkey <key> -in message -out message.sig
        let pem = include_str!("../../testdata/signing/openssl-test.pub");
        let key = PublicKey::from_pem(pem).unwrap();
        let msg = include_bytes!("../../testdata/signing/message");
        let sig = include_bytes!("../../testdata/signing/message.sig");
        assert!(verify(msg, sig, &[key]).is_ok());
        assert!(verify(b"Aurion test binary, modified\n", sig, &[key]).is_err());
    }

    #[test]
    fn only_a_trusted_key_is_accepted() {
        let (release, backup, stranger) = (key(1), key(2), key(3));
        let trusted = [release.pk, backup.pk];
        let bin = b"\x7fELF aurion";
        assert!(verify(bin, release.sk.sign(bin, None).as_ref(), &trusted).is_ok());
        assert!(verify(bin, backup.sk.sign(bin, None).as_ref(), &trusted).is_ok(), "backup key");
        assert!(verify(bin, stranger.sk.sign(bin, None).as_ref(), &trusted).is_err(), "someone else's key");
        let other = b"\x7fELF modified";
        assert!(verify(other, release.sk.sign(bin, None).as_ref(), &trusted).is_err(), "binary changed after signing");
        assert!(verify(bin, &[], &trusted).is_err(), "no signature");
        assert!(verify(bin, b"not a signature", &trusted).is_err(), "garbage");
    }
}
