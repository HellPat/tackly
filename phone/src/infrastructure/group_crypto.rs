//! The family as an MLS group (RFC 9420, via OpenMLS). Every phone is a
//! member with its own signing key; every event is an MLS application
//! message: encrypted to the group's current key, signed by its sender, and
//! numbered, so a dropped or reordered one shows up. Adding or removing a
//! member is a commit, which moves the group to a new key.
//!
//! OpenMLS keeps the group state and all key material in a storage provider;
//! this one lives in memory and is written to the secrets folder after every
//! change, so the phone finds its group again after a restart.

use anyhow::{Context, Result, anyhow, bail, ensure};
use openmls::prelude::{tls_codec::Deserialize as _, *};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::{MemoryStorage, RustCrypto};
use openmls_traits::OpenMlsProvider;
use uuid::Uuid;

use crate::infrastructure::{
    digit_code::{AgentCode, ConfirmationCode},
    secrets::Secrets,
};

const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
const STORAGE: &str = "mls-storage";
const SIGNER: &str = "mls-signer";
/// The public key of a fresh signing key while its rotation commit is out.
const SIGNER_NEXT: &str = "mls-signer-next";

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
    /// A commit by `sender`, merged only when it came from the head.
    /// `removed_me`: this phone is out of the group now. `from_outside`:
    /// the head came back from a backup and needs the history again.
    Commit {
        sender: Uuid,
        merged: bool,
        removed_me: bool,
        from_outside: bool,
    },
    /// A proposal or something else this app does not use.
    Other,
}

/// A commit tells who sent it in one of two ways: a member, by its leaf, or
/// someone (re)joining from outside, by the leaf it brings along.
enum Commiter<'a> {
    Member(LeafNodeIndex),
    Outsider(&'a LeafNode),
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
                values.insert(
                    crate::infrastructure::crypto::decode(&key)?,
                    crate::infrastructure::crypto::decode(&value)?,
                );
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
            .map(|(key, value)| {
                (
                    crate::infrastructure::crypto::encode(key),
                    crate::infrastructure::crypto::encode(value),
                )
            })
            .collect();
        self.secrets.write(STORAGE, &serde_json::to_vec(&map)?)
    }

    /// Forgets the group and makes a new signing key (leaving the family).
    pub fn clear(&mut self, device_id: Uuid) -> Result<()> {
        self.secrets.delete(STORAGE)?;
        self.secrets.delete(SIGNER)?;
        self.secrets.delete(SIGNER_NEXT)?;
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

    /// Adds `device` to the group: the commit (for the group) and the welcome
    /// (for the device). The commit is pending until [`Self::merge_pending`].
    /// The key package must be that device's, and the device must be new:
    /// a leaf claiming to be the head, or another member, is refused.
    pub fn add_member(
        &mut self,
        family_id: Uuid,
        device: Uuid,
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
        ensure!(
            device_of(key_package.leaf_node().credential()) == Some(device),
            "the key package is not that device's"
        );
        let mut group = self.group(family_id)?;
        ensure!(
            !group
                .members()
                .any(|member| device_of(&member.credential) == Some(device)),
            "that device is in the family already"
        );
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

    /// Takes a device out of the group: the commit, pending until
    /// [`Self::merge_pending`]. The device's keys stop at this epoch.
    pub fn remove_member(&mut self, family_id: Uuid, device: Uuid) -> Result<Vec<u8>> {
        let mut group = self.group(family_id)?;
        let leaf = group
            .members()
            .find(|member| device_of(&member.credential) == Some(device))
            .map(|member| member.index)
            .context("that device is not in the family")?;
        let (commit, _, _) = group
            .remove_members(&self.provider, &self.signer, &[leaf])
            .map_err(|error| anyhow!("remove the member: {error:?}"))?;
        self.persist()?;
        commit
            .to_bytes()
            .map_err(|error| anyhow!("encode the commit: {error:?}"))
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

    /// Decrypts and checks a message from the group. Only the head changes
    /// who is in the family, so a commit is merged only when it comes from
    /// the head's leaf, or from outside with the head's own signing key (the
    /// head's phone was lost and comes back, see [`Self::rejoin`]).
    pub fn decrypt(&mut self, family_id: Uuid, bytes: &[u8], head: Uuid) -> Result<Incoming> {
        let message = MlsMessageIn::tls_deserialize_exact(bytes)
            .context("read the message")?
            .try_into_protocol_message()
            .map_err(|error| anyhow!("not a group message: {error:?}"))?;
        let mut group = self.group(family_id)?;
        let processed = group
            .process_message(&self.provider, message)
            .map_err(|error| anyhow!("message failed verification: {error:?}"))?;
        let sender = device_of(processed.credential()).context("sender without a device ID")?;
        // A member commits by its leaf; anyone else who commits joins from
        // outside (others only propose, which this app does not use).
        let commiter = match processed.sender() {
            Sender::Member(leaf) => Some(Commiter::Member(*leaf)),
            _ => None,
        };
        let incoming = match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(message) => Incoming::Application {
                sender,
                bytes: message.into_bytes(),
            },
            ProcessedMessageContent::StagedCommitMessage(commit) => {
                let commiter = match commiter {
                    Some(commiter) => commiter,
                    None => Commiter::Outsider(
                        commit
                            .update_path_leaf_node()
                            .context("an outsider's commit without its leaf")?,
                    ),
                };
                let merged = Self::is_head(&group, head, &commiter);
                let from_outside = matches!(commiter, Commiter::Outsider(_));
                let removed_me = merged && commit.self_removed();
                if merged {
                    group
                        .merge_staged_commit(&self.provider, *commit)
                        .map_err(|error| anyhow!("merge the commit: {error:?}"))?;
                }
                Incoming::Commit {
                    sender,
                    merged,
                    removed_me,
                    from_outside,
                }
            }
            _ => Incoming::Other,
        };
        self.persist()?;
        Ok(incoming)
    }

    /// Whether a commit comes from the head: from the head's leaf, or from
    /// outside with the head's current signing key (nobody else has it).
    fn is_head(group: &MlsGroup, head: Uuid, commiter: &Commiter<'_>) -> bool {
        let head_leaf = group
            .members()
            .find(|member| device_of(&member.credential) == Some(head));
        match commiter {
            Commiter::Member(leaf) => head_leaf.is_some_and(|member| member.index == *leaf),
            Commiter::Outsider(leaf) => {
                device_of(leaf.credential()) == Some(head)
                    && head_leaf.is_some_and(|member| {
                        member.signature_key == leaf.signature_key().as_slice()
                    })
            }
        }
    }

    // ---- for the head's recovery ------------------------------------------------------

    /// This phone's signing key, for the head's recovery backup.
    pub fn export_signer(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&self.signer)?)
    }

    /// The group's public state with its tree: what a phone that lost
    /// everything needs to come back by an external commit.
    pub fn export_group_info(&self, family_id: Uuid) -> Result<Vec<u8>> {
        self.group(family_id)?
            .export_group_info(self.provider.crypto(), &self.signer, true)
            .map_err(|error| anyhow!("export the group info: {error:?}"))?
            .to_bytes()
            .map_err(|error| anyhow!("encode the group info: {error:?}"))
    }

    /// Takes over a backed-up signing key (the lost phone's), so that this
    /// phone can come back as the head.
    pub fn adopt_signer(&mut self, signer: &[u8], device_id: Uuid) -> Result<()> {
        let signer: SignatureKeyPair =
            serde_json::from_slice(signer).context("read the backed-up signing key")?;
        signer
            .store(self.provider.storage())
            .map_err(|error| anyhow!("store the signing key: {error:?}"))?;
        self.secrets.write(SIGNER, signer.public())?;
        self.credential = CredentialWithKey {
            credential: BasicCredential::new(device_id.as_bytes().to_vec()).into(),
            signature_key: signer.to_public_vec().into(),
        };
        self.signer = signer;
        self.persist()
    }

    /// Comes back into the group from outside, by an external commit with
    /// the (adopted) head key: the old leaf with that key is removed by the
    /// same commit. The commit is for the relay; the group is merged already.
    pub fn rejoin(&mut self, family_id: Uuid, group_info: &[u8]) -> Result<Vec<u8>> {
        let group_info = match MlsMessageIn::tls_deserialize_exact(group_info)
            .context("read the group info")?
            .extract()
        {
            MlsMessageBodyIn::GroupInfo(group_info) => group_info,
            _ => bail!("not a group info"),
        };
        ensure!(
            group_info.group_id().as_slice() == family_id.as_bytes(),
            "the group info is for another family"
        );
        let (group, bundle) = MlsGroup::external_commit_builder()
            .with_config(Self::join_config())
            .build_group(&self.provider, group_info, self.credential.clone())
            .map_err(|error| anyhow!("prepare the external commit: {error:?}"))?
            .load_psks(self.provider.storage())
            .map_err(|error| anyhow!("prepare the external commit: {error:?}"))?
            .build(
                self.provider.rand(),
                self.provider.crypto(),
                &self.signer,
                |_| true,
            )
            .map_err(|error| anyhow!("build the external commit: {error:?}"))?
            .finalize(&self.provider)
            .map_err(|error| anyhow!("finish the external commit: {error:?}"))?;
        drop(group);
        self.persist()?;
        bundle
            .commit()
            .to_bytes()
            .map_err(|error| anyhow!("encode the commit: {error:?}"))
    }

    /// Moves this phone to a fresh signing key, signed by the old one: after
    /// this commit the lost phone's key is no longer the head's. Pending
    /// until [`Self::merge_pending`]; the new key is kept aside until then.
    pub fn rotate_signer(&mut self, family_id: Uuid, device_id: Uuid) -> Result<Vec<u8>> {
        let scheme = CIPHERSUITE.signature_algorithm();
        let fresh = SignatureKeyPair::new(scheme)
            .map_err(|error| anyhow!("make a signing key: {error:?}"))?;
        fresh
            .store(self.provider.storage())
            .map_err(|error| anyhow!("store the signing key: {error:?}"))?;
        let credential = CredentialWithKey {
            credential: BasicCredential::new(device_id.as_bytes().to_vec()).into(),
            signature_key: fresh.to_public_vec().into(),
        };
        let mut group = self.group(family_id)?;
        let bundle = group
            .self_update_with_new_signer(
                &self.provider,
                &self.signer,
                NewSignerBundle {
                    signer: &fresh,
                    credential_with_key: credential.clone(),
                },
                LeafNodeParameters::default(),
            )
            .map_err(|error| anyhow!("rotate the signing key: {error:?}"))?;
        self.secrets.write(SIGNER_NEXT, fresh.public())?;
        self.persist()?;
        bundle
            .commit()
            .to_bytes()
            .map_err(|error| anyhow!("encode the commit: {error:?}"))
    }

    /// The relay took the rotation commit: the fresh key is ours now.
    pub fn merge_rotation(&mut self, family_id: Uuid, device_id: Uuid) -> Result<()> {
        let scheme = CIPHERSUITE.signature_algorithm();
        let public = self
            .secrets
            .read(SIGNER_NEXT)?
            .context("no rotation is pending")?;
        let fresh = SignatureKeyPair::read(self.provider.storage(), &public, scheme)
            .context("the fresh signing key is gone")?;
        self.merge_pending(family_id)?;
        self.secrets.write(SIGNER, &public)?;
        self.secrets.delete(SIGNER_NEXT)?;
        self.credential = CredentialWithKey {
            credential: BasicCredential::new(device_id.as_bytes().to_vec()).into(),
            signature_key: fresh.to_public_vec().into(),
        };
        self.signer = fresh;
        self.persist()
    }
}

/// The device a credential stands for: its ID is the credential's identity.
fn device_of(credential: &Credential) -> Option<Uuid> {
    let basic = BasicCredential::try_from(credential.clone()).ok()?;
    Uuid::from_slice(basic.identity()).ok()
}

/// The six digits both phones derive from the invitation's secret and the
/// joining phone's key package. The relay knows neither the secret nor how to
/// make a key package that gives the same digits.
pub fn confirmation_code(secret: &[u8], key_package: &[u8]) -> ConfirmationCode {
    ConfirmationCode::from_hash(&crate::infrastructure::crypto::derive(
        "confirmation",
        &[secret, key_package],
    ))
}

/// The twelve digits the head's phone and the AI agent both derive from the
/// agent's key package. There is no shared secret to bind them to, so they
/// are longer than the six of an invitation: a relay that swaps the key
/// cannot find one that gives the same digits.
pub fn agent_code(key_package: &[u8]) -> AgentCode {
    AgentCode::from_hash(&crate::infrastructure::crypto::derive(
        "agent-code",
        &[key_package],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What came in: a message (its sender and text), a commit (merged,
    /// removed me), or neither.
    type Read = (Option<(Uuid, Vec<u8>)>, Option<(bool, bool)>);

    fn read(incoming: Incoming) -> Read {
        match incoming {
            Incoming::Application { sender, bytes } => (Some((sender, bytes)), None),
            Incoming::Commit {
                merged, removed_me, ..
            } => (None, Some((merged, removed_me))),
            Incoming::Other => (None, None),
        }
    }

    fn committed(incoming: Incoming) -> Option<(bool, bool)> {
        read(incoming).1
    }

    /// Who sent a message, and what it says.
    fn said(incoming: Incoming) -> Option<(Uuid, Vec<u8>)> {
        read(incoming).0
    }

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
        assert!(
            head.add_member(family, Uuid::new_v4(), &key_package)
                .is_err(),
            "a key package under another device's name is refused"
        );
        head.clear_pending(family).expect("clear");
        let (commit, welcome) = head
            .add_member(family, joiner_id, &key_package)
            .expect("add");
        head.merge_pending(family).expect("merge");
        let (joined, sender) = joiner.join(&welcome).expect("join");
        assert_eq!((joined, sender), (family, head_id));
        assert_eq!(head.members(family).expect("members").len(), 2);

        let secret = head.encrypt(family, b"milk").expect("encrypt");
        assert_eq!(
            said(joiner.decrypt(family, &secret, head_id).expect("decrypt")),
            Some((head_id, b"milk".to_vec()))
        );
        assert!(
            stranger.decrypt(family, &secret, head_id).is_err(),
            "no group, no reading"
        );
        assert!(
            joiner.decrypt(family, &commit, head_id).is_err(),
            "the commit that added us is not for us"
        );
    }

    /// Head, one member in, and the member's view of the group.
    fn family() -> (Mls, Uuid, Mls, Uuid, Uuid, Vec<tempfile::TempDir>) {
        let family = Uuid::new_v4();
        let (mut head, head_id, a) = phone("head");
        let (mut member, member_id, b) = phone("member");
        head.create_group(family).expect("create");
        let key_package = member.key_package().expect("key package");
        let (_, welcome) = head
            .add_member(family, member_id, &key_package)
            .expect("add");
        head.merge_pending(family).expect("merge");
        member.join(&welcome).expect("join");
        (head, head_id, member, member_id, family, vec![a, b])
    }

    #[test]
    fn only_the_heads_commits_count_and_a_removed_phone_reads_nothing_new() {
        let (mut head, head_id, mut member, member_id, family, _dirs) = family();
        let (mut third, third_id, _c) = phone("third");
        let key_package = third.key_package().expect("key package");
        let (added, welcome) = head
            .add_member(family, third_id, &key_package)
            .expect("add");
        head.merge_pending(family).expect("merge");
        third.join(&welcome).expect("join");
        assert_eq!(
            committed(member.decrypt(family, &added, head_id).expect("decrypt"))
                .map(|(merged, _)| merged),
            Some(true),
            "the head added someone"
        );

        // The member tries to throw the head out: nobody listens.
        let coup = member.remove_member(family, head_id).expect("commit");
        assert_eq!(
            committed(third.decrypt(family, &coup, head_id).expect("decrypt"))
                .map(|(merged, _)| merged),
            Some(false),
            "not from the head"
        );
        assert_eq!(third.members(family).expect("members").len(), 3);
        member.clear_pending(family).expect("clear");

        // The head removes the third: it is out, the member sees that.
        let out = head.remove_member(family, third_id).expect("commit");
        head.merge_pending(family).expect("merge");
        assert_eq!(
            committed(third.decrypt(family, &out, head_id).expect("decrypt")),
            Some((true, true))
        );
        let later = head.encrypt(family, b"after").expect("encrypt");
        assert!(third.decrypt(family, &later, head_id).is_err());
        assert_eq!(head.members(family).expect("members").len(), 2);
        let _ = member_id;
    }

    #[test]
    fn the_head_comes_back_from_a_backup_and_rotates_its_key() {
        let (head, head_id, mut member, _member_id, family, _dirs) = family();
        let signer = head.export_signer().expect("signer");
        let group_info = head.export_group_info(family).expect("group info");
        let (mut thief, _, _t) = phone("thief");

        // A new phone with the backed-up key comes back; the member takes it
        // for the head. A phone without the head's key is turned away.
        let (mut back, _, _d) = phone("back");
        back.adopt_signer(&signer, head_id).expect("adopt");
        let commit = back.rejoin(family, &group_info).expect("rejoin");
        assert_eq!(
            committed(member.decrypt(family, &commit, head_id).expect("decrypt"))
                .map(|(merged, _)| merged),
            Some(true),
            "the head's key"
        );
        assert_eq!(member.members(family).expect("members").len(), 2);
        let hello = back.encrypt(family, b"back").expect("encrypt");
        assert_eq!(
            said(member.decrypt(family, &hello, head_id).expect("decrypt")),
            Some((head_id, b"back".to_vec()))
        );

        // Now the head moves to a fresh key: the old one (still on the lost
        // phone) is no longer the head's.
        let rotation = back.rotate_signer(family, head_id).expect("rotate");
        back.merge_rotation(family, head_id).expect("merge");
        assert_eq!(
            committed(member.decrypt(family, &rotation, head_id).expect("decrypt"))
                .map(|(merged, _)| merged),
            Some(true)
        );
        let fresh_info = back.export_group_info(family).expect("group info");
        thief.adopt_signer(&signer, head_id).expect("adopt");
        let stale = thief.rejoin(family, &fresh_info).expect("rejoin");
        assert_eq!(
            committed(member.decrypt(family, &stale, head_id).expect("decrypt"))
                .map(|(merged, _)| merged),
            Some(false),
            "the old key is out"
        );
        let after = back.encrypt(family, b"still me").expect("encrypt");
        assert_eq!(
            said(member.decrypt(family, &after, head_id).expect("decrypt")),
            Some((head_id, b"still me".to_vec()))
        );
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
    fn the_agent_code_follows_the_key_package() {
        assert_eq!(agent_code(b"kp"), agent_code(b"kp"));
        assert_ne!(agent_code(b"kp"), agent_code(b"other"));
    }

    #[test]
    fn the_digits_change_with_the_key_package() {
        assert_eq!(
            confirmation_code(b"s", b"kp"),
            confirmation_code(b"s", b"kp")
        );
        assert_ne!(
            confirmation_code(b"s", b"kp"),
            confirmation_code(b"s", b"other")
        );
        assert_ne!(
            confirmation_code(b"s", b"kp"),
            confirmation_code(b"t", b"kp")
        );
    }

    #[test]
    fn a_message_of_the_wrong_kind_is_refused() {
        let (mut head, _, mut member, member_id, family, _dirs) = family();
        let key_package = member.key_package().expect("key package");
        let welcome_bytes = {
            let (mut other, other_id, _dir) = phone("other");
            let other_package = other.key_package().expect("key package");
            let (_, welcome) = head
                .add_member(family, other_id, &other_package)
                .expect("add");
            head.clear_pending(family).expect("clear");
            welcome
        };
        let refused = |result: Result<_>, why: &str| {
            let error = result.expect_err("refused").to_string();
            assert!(error.contains(why), "{error}");
        };
        refused(
            head.add_member(family, member_id, &welcome_bytes)
                .map(|_| ()),
            "not a key package",
        );
        refused(member.join(&key_package).map(|_| ()), "not a welcome");
        refused(
            head.rejoin(family, &key_package).map(|_| ()),
            "not a group info",
        );
    }

    #[test]
    fn a_proposal_is_something_else() {
        let (mut head, head_id, member, _, family, _dirs) = family();
        let mut group = member.group(family).expect("group");
        let (proposal, _) = group
            .propose_self_update(
                &member.provider,
                &member.signer,
                LeafNodeParameters::default(),
            )
            .expect("propose");
        let bytes = proposal.to_bytes().expect("bytes");
        assert_eq!(
            committed(head.decrypt(family, &bytes, head_id).expect("read")),
            None
        );
    }
}
