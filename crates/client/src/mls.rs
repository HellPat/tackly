//! The family as an MLS group (RFC 9420, via OpenMLS). Every phone is a
//! member with its own signing key; every event is an MLS application
//! message: encrypted to the group's current key, signed by its sender, and
//! numbered, so a dropped or reordered one shows up. Adding or removing a
//! member is a commit, which moves the group to a new key.
//!
//! OpenMLS keeps the group state and all key material in a storage provider;
//! this one lives in memory and is written to the secrets folder after every
//! change, so the phone finds its group again after a restart.

use anyhow::{Context, Result, anyhow, bail};
use openmls::prelude::{tls_codec::Deserialize as _, *};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::{MemoryStorage, RustCrypto};
use openmls_traits::OpenMlsProvider;
use uuid::Uuid;

use crate::secrets::Secrets;

const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
const STORAGE: &str = "mls-storage";
const SIGNER: &str = "mls-signer";

/// OpenMLS' crypto with a storage this module persists itself.
#[derive(Default)]
struct Provider {
    crypto: RustCrypto,
    storage: MemoryStorage,
}

impl OpenMlsProvider for Provider {
    type CryptoProvider = RustCrypto;
    type RandProvider = RustCrypto;
    type StorageProvider = MemoryStorage;

    fn storage(&self) -> &MemoryStorage {
        &self.storage
    }

    fn crypto(&self) -> &RustCrypto {
        &self.crypto
    }

    fn rand(&self) -> &RustCrypto {
        &self.crypto
    }
}

/// What a message from the group turned out to be.
pub enum Incoming {
    /// An application message: `sender` wrote `bytes`.
    Application { sender: Uuid, bytes: Vec<u8> },
    /// A commit by `sender`; merged when `allowed` said so.
    Commit { sender: Uuid, merged: bool },
    /// A proposal or something else this app does not use.
    Other,
}

pub struct Mls {
    provider: Provider,
    signer: SignatureKeyPair,
    credential: CredentialWithKey,
    secrets: Secrets,
}

impl Mls {
    /// This phone's MLS state: its signing key and, once in a family, the group.
    pub fn open(secrets: &Secrets, device_id: Uuid) -> Result<Self> {
        let provider = Provider::default();
        if let Some(bytes) = secrets.read(STORAGE)? {
            let map: std::collections::HashMap<String, String> =
                serde_json::from_slice(&bytes).context("MLS storage")?;
            let mut values = provider
                .storage
                .values
                .write()
                .map_err(|_| anyhow!("MLS storage lock"))?;
            for (key, value) in map {
                values.insert(crate::crypto::decode(&key)?, crate::crypto::decode(&value)?);
            }
        }
        // The key pair itself lives in the (persisted) storage; the secrets
        // folder only remembers which public key is ours.
        let scheme = CIPHERSUITE.signature_algorithm();
        let known = secrets
            .read(SIGNER)?
            .and_then(|public| SignatureKeyPair::read(provider.storage(), &public, scheme));
        let signer = match known {
            Some(signer) => signer,
            None => {
                let signer = SignatureKeyPair::new(scheme)
                    .map_err(|error| anyhow!("make a signing key: {error:?}"))?;
                signer
                    .store(provider.storage())
                    .map_err(|error| anyhow!("store the signing key: {error:?}"))?;
                secrets.write(SIGNER, signer.public())?;
                signer
            }
        };
        let credential = CredentialWithKey {
            credential: BasicCredential::new(device_id.as_bytes().to_vec()).into(),
            signature_key: signer.to_public_vec().into(),
        };
        let mls = Self {
            provider,
            signer,
            credential,
            secrets: secrets.clone(),
        };
        mls.persist()?;
        Ok(mls)
    }

    /// Writes the storage to disk (after every change).
    fn persist(&self) -> Result<()> {
        let values = self
            .provider
            .storage
            .values
            .read()
            .map_err(|_| anyhow!("MLS storage lock"))?;
        let map: std::collections::HashMap<String, String> = values
            .iter()
            .map(|(key, value)| (crate::crypto::encode(key), crate::crypto::encode(value)))
            .collect();
        self.secrets.write(STORAGE, &serde_json::to_vec(&map)?)
    }

    /// Forgets the group and makes a new signing key (leaving the family).
    pub fn clear(&mut self, device_id: Uuid) -> Result<()> {
        self.secrets.delete(STORAGE)?;
        self.secrets.delete(SIGNER)?;
        *self = Self::open(&self.secrets.clone(), device_id)?;
        Ok(())
    }

    fn group_id(family_id: Uuid) -> GroupId {
        GroupId::from_slice(family_id.as_bytes())
    }

    fn group(&self, family_id: Uuid) -> Result<MlsGroup> {
        MlsGroup::load(self.provider.storage(), &Self::group_id(family_id))
            .map_err(|error| anyhow!("load the MLS group: {error:?}"))?
            .context("this phone has no MLS group for the family")
    }

    fn join_config() -> MlsGroupJoinConfig {
        MlsGroupJoinConfig::builder()
            .use_ratchet_tree_extension(true)
            // Messages encrypted just before a commit may still arrive after it.
            .max_past_epochs(2)
            .build()
    }

    /// Starts the family's group with this phone as its only member.
    pub fn create_group(&mut self, family_id: Uuid) -> Result<()> {
        let config = MlsGroupCreateConfig::builder()
            .ciphersuite(CIPHERSUITE)
            .use_ratchet_tree_extension(true)
            .max_past_epochs(2)
            .build();
        MlsGroup::new_with_group_id(
            &self.provider,
            &self.signer,
            &config,
            Self::group_id(family_id),
            self.credential.clone(),
        )
        .map_err(|error| anyhow!("create the MLS group: {error:?}"))?;
        self.persist()
    }

    pub fn has_group(&self, family_id: Uuid) -> bool {
        self.group(family_id).is_ok()
    }

    pub fn epoch(&self, family_id: Uuid) -> Result<u64> {
        Ok(self.group(family_id)?.epoch().as_u64())
    }

    /// The devices in the group.
    pub fn members(&self, family_id: Uuid) -> Result<Vec<Uuid>> {
        Ok(self
            .group(family_id)?
            .members()
            .filter_map(|member| device_of(&member.credential))
            .collect())
    }

    /// A fresh key package for joining a family: what the head adds to the group.
    pub fn key_package(&mut self) -> Result<Vec<u8>> {
        let bundle = KeyPackage::builder()
            .build(
                CIPHERSUITE,
                &self.provider,
                &self.signer,
                self.credential.clone(),
            )
            .map_err(|error| anyhow!("make a key package: {error:?}"))?;
        self.persist()?;
        MlsMessageOut::from(bundle)
            .to_bytes()
            .map_err(|error| anyhow!("encode the key package: {error:?}"))
    }

    /// Adds a device to the group: the commit (for the group) and the welcome
    /// (for the device). The commit is pending until [`Self::merge_pending`].
    pub fn add_member(
        &mut self,
        family_id: Uuid,
        key_package: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        let key_package = match MlsMessageIn::tls_deserialize_exact(key_package)
            .context("read the key package")?
            .extract()
        {
            MlsMessageBodyIn::KeyPackage(key_package) => key_package
                .validate(self.provider.crypto(), ProtocolVersion::Mls10)
                .map_err(|error| anyhow!("invalid key package: {error:?}"))?,
            _ => bail!("not a key package"),
        };
        let mut group = self.group(family_id)?;
        let (commit, welcome, _) = group
            .add_members(&self.provider, &self.signer, &[key_package])
            .map_err(|error| anyhow!("add the member: {error:?}"))?;
        self.persist()?;
        Ok((
            commit
                .to_bytes()
                .map_err(|error| anyhow!("encode the commit: {error:?}"))?,
            welcome
                .to_bytes()
                .map_err(|error| anyhow!("encode the welcome: {error:?}"))?,
        ))
    }

    /// The relay accepted this phone's commit: the group moves on.
    pub fn merge_pending(&mut self, family_id: Uuid) -> Result<()> {
        self.group(family_id)?
            .merge_pending_commit(&self.provider)
            .map_err(|error| anyhow!("merge the commit: {error:?}"))?;
        self.persist()
    }

    /// The relay refused this phone's commit (another one came first): forget it.
    pub fn clear_pending(&mut self, family_id: Uuid) -> Result<()> {
        self.group(family_id)?
            .clear_pending_commit(self.provider.storage())
            .map_err(|error| anyhow!("drop the commit: {error:?}"))?;
        self.persist()
    }

    /// Joins the group a welcome message lets this phone into. Returns the
    /// family and who sent the welcome (the head: snapshots are accepted from it).
    pub fn join(&mut self, welcome: &[u8]) -> Result<(Uuid, Uuid)> {
        let welcome = match MlsMessageIn::tls_deserialize_exact(welcome)
            .context("read the welcome")?
            .extract()
        {
            MlsMessageBodyIn::Welcome(welcome) => welcome,
            _ => bail!("not a welcome message"),
        };
        let staged =
            StagedWelcome::new_from_welcome(&self.provider, &Self::join_config(), welcome, None)
                .map_err(|error| anyhow!("open the welcome: {error:?}"))?;
        let sender = staged
            .welcome_sender()
            .ok()
            .and_then(|leaf| device_of(leaf.credential()))
            .context("the welcome names no sender")?;
        let group = staged
            .into_group(&self.provider)
            .map_err(|error| anyhow!("join the group: {error:?}"))?;
        let family_id = Uuid::from_slice(group.group_id().as_slice()).context("group ID")?;
        self.persist()?;
        Ok((family_id, sender))
    }

    /// Encrypts bytes for the group as an application message.
    pub fn encrypt(&mut self, family_id: Uuid, plaintext: &[u8]) -> Result<Vec<u8>> {
        let message = self
            .group(family_id)?
            .create_message(&self.provider, &self.signer, plaintext)
            .map_err(|error| anyhow!("encrypt for the family: {error:?}"))?;
        self.persist()?;
        message
            .to_bytes()
            .map_err(|error| anyhow!("encode the message: {error:?}"))
    }

    /// Decrypts and checks a message from the group. A commit is merged only
    /// when `allow_commit` says its sender may change the family.
    pub fn decrypt(
        &mut self,
        family_id: Uuid,
        bytes: &[u8],
        allow_commit: impl Fn(Uuid) -> bool,
    ) -> Result<Incoming> {
        let message = MlsMessageIn::tls_deserialize_exact(bytes)
            .context("read the message")?
            .try_into_protocol_message()
            .map_err(|error| anyhow!("not a group message: {error:?}"))?;
        let mut group = self.group(family_id)?;
        let processed = group
            .process_message(&self.provider, message)
            .map_err(|error| anyhow!("message failed verification: {error:?}"))?;
        let sender = device_of(processed.credential()).context("sender without a device ID")?;
        let incoming = match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(message) => Incoming::Application {
                sender,
                bytes: message.into_bytes(),
            },
            ProcessedMessageContent::StagedCommitMessage(commit) => {
                let merged = allow_commit(sender);
                if merged {
                    group
                        .merge_staged_commit(&self.provider, *commit)
                        .map_err(|error| anyhow!("merge the commit: {error:?}"))?;
                }
                Incoming::Commit { sender, merged }
            }
            _ => Incoming::Other,
        };
        self.persist()?;
        Ok(incoming)
    }
}

/// The device a credential stands for: its ID is the credential's identity.
fn device_of(credential: &Credential) -> Option<Uuid> {
    let basic = BasicCredential::try_from(credential.clone()).ok()?;
    Uuid::from_slice(basic.identity()).ok()
}

/// Six digits both phones derive from the invitation's secret and the joining
/// phone's key package. The relay knows neither the secret nor how to make
/// a key package that gives the same digits.
pub fn confirmation_code(secret: &[u8], key_package: &[u8]) -> String {
    let hash = crate::crypto::derive("confirmation", &[secret, key_package]);
    let number = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]) % 1_000_000;
    format!("{number:06}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phone(name: &str) -> (Mls, Uuid, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let id = Uuid::new_v4();
        let mls = Mls::open(&Secrets::new(&dir.path().join(name)), id).expect("open");
        (mls, id, dir)
    }

    #[test]
    fn two_phones_share_a_group_and_a_third_is_kept_out() {
        let family = Uuid::new_v4();
        let (mut head, head_id, _a) = phone("head");
        let (mut joiner, joiner_id, _b) = phone("joiner");
        let (mut stranger, _, _c) = phone("stranger");
        head.create_group(family).expect("create");

        let key_package = joiner.key_package().expect("key package");
        let (commit, welcome) = head.add_member(family, &key_package).expect("add");
        head.merge_pending(family).expect("merge");
        let (joined, sender) = joiner.join(&welcome).expect("join");
        assert_eq!((joined, sender), (family, head_id));
        assert_eq!(head.members(family).expect("members").len(), 2);

        let secret = head.encrypt(family, b"milk").expect("encrypt");
        match joiner.decrypt(family, &secret, |_| false).expect("decrypt") {
            Incoming::Application { sender, bytes } => {
                assert_eq!((sender, bytes.as_slice()), (head_id, b"milk".as_slice()));
            }
            _ => panic!("expected an application message"),
        }
        assert!(
            stranger.decrypt(family, &secret, |_| false).is_err(),
            "no group, no reading"
        );
        assert!(
            joiner.decrypt(family, &commit, |_| true).is_err(),
            "the commit that added us is not for us"
        );
        let _ = joiner_id;
    }

    #[test]
    fn the_group_survives_a_restart() {
        let family = Uuid::new_v4();
        let dir = tempfile::tempdir().expect("temp dir");
        let secrets = Secrets::new(dir.path());
        let id = Uuid::new_v4();
        {
            let mut mls = Mls::open(&secrets, id).expect("open");
            mls.create_group(family).expect("create");
        }
        let again = Mls::open(&secrets, id).expect("open again");
        assert!(again.has_group(family));
        assert_eq!(again.members(family).expect("members"), vec![id]);
    }

    #[test]
    fn the_digits_change_with_the_key_package() {
        assert_eq!(confirmation_code(b"s", b"kp").len(), 6);
        assert_eq!(
            confirmation_code(b"s", b"kp"),
            confirmation_code(b"s", b"kp")
        );
        assert_ne!(
            confirmation_code(b"s", b"kp"),
            confirmation_code(b"s", b"other")
        );
    }
}
