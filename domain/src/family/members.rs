//! The family itself and who is in it: its head, the people, the AI agents.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    aggregate::not_empty,
    commands::FamilyCommand,
    errors::FamilyError,
    events::{DomainEvent, FamilyEvent, Picture},
    state::Family,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub device_id: Uuid,
    pub name: String,
    pub owner: bool,
    pub picture: Option<Picture>,
    /// An AI agent, reading along until then.
    pub agent_until: Option<DateTime<Utc>>,
}

impl Member {
    pub fn is_agent(&self) -> bool {
        self.agent_until.is_some()
    }
}

impl Family {
    /// The head of the family: whoever created it.
    pub fn head(&self) -> Option<Uuid> {
        self.members
            .values()
            .find(|member| member.owner)
            .map(|member| member.device_id)
    }

    pub fn is_head(&self, device_id: Uuid) -> bool {
        self.members
            .get(&device_id)
            .is_some_and(|member| member.owner)
    }

    pub fn is_agent(&self, device_id: Uuid) -> bool {
        self.members.get(&device_id).is_some_and(Member::is_agent)
    }

    /// The people in the family, without the AI agents.
    pub fn people(&self) -> impl Iterator<Item = &Member> {
        self.members.values().filter(|member| !member.is_agent())
    }

    pub fn agents(&self) -> impl Iterator<Item = &Member> {
        self.members.values().filter(|member| member.is_agent())
    }

    pub fn member_name(&self, device_id: Uuid) -> &str {
        self.members
            .get(&device_id)
            .map_or("Someone", |member| member.name.as_str())
    }

    /// Checks the members commands and returns the events they produce, as (subject, event).
    pub(super) fn decide_members(
        &self,
        command: FamilyCommand,
        me: Uuid,
    ) -> Result<Vec<(Uuid, DomainEvent)>, FamilyError> {
        use DomainEvent as E;
        use FamilyCommand as C;
        let family = || self.family_id.ok_or(FamilyError::NoFamily);
        let one = |subject, event| Ok(vec![(subject, event)]);
        match command {
            C::CreateFamily {
                family_id,
                name,
                owner_name,
            } => {
                if self.family_id.is_some() {
                    return Err(FamilyError::AlreadyCreated);
                }
                one(
                    family_id,
                    E::FamilyCreated {
                        name: not_empty(&name)?,
                        owner_name: not_empty(&owner_name)?,
                    },
                )
            }
            C::Join { name } => one(
                family()?,
                E::MemberJoined {
                    name: not_empty(&name)?,
                },
            ),
            C::Rename { name } => one(
                family()?,
                E::MemberRenamed {
                    name: not_empty(&name)?,
                },
            ),
            C::SetPicture { picture } => {
                if !picture.photo_is_valid() {
                    return Err(FamilyError::BadPhoto);
                }
                one(family()?, E::MemberPictureChanged { picture })
            }
            C::RemoveMember { member_id } => {
                if !self.is_head(me) {
                    return Err(FamilyError::NotHead);
                }
                if member_id == me {
                    return Err(FamilyError::HeadStays);
                }
                self.members
                    .get(&member_id)
                    .ok_or(FamilyError::UnknownMember)?;
                one(family()?, E::MemberRemoved { member_id })
            }
            C::AdmitAgent {
                member_id,
                name,
                until,
            } => {
                if !self.is_head(me) {
                    return Err(FamilyError::NotHead);
                }
                if member_id == me {
                    return Err(FamilyError::HeadStays);
                }
                one(
                    family()?,
                    E::AgentAdmitted {
                        member_id,
                        name: not_empty(&name)?,
                        until,
                    },
                )
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Applies the members events; `false` when the event is not one of them.
    pub(super) fn apply_members(&mut self, envelope: &FamilyEvent) -> bool {
        let (subject, by, _) = (
            envelope.subject_id,
            envelope.origin_device_id,
            envelope.occurred_at,
        );
        match &envelope.event {
            DomainEvent::FamilyCreated { name, owner_name } => {
                if self.family_id.is_some() {
                    return true;
                }
                self.family_id = Some(subject);
                self.name = Some(name.clone());
                self.members.insert(
                    by,
                    Member {
                        device_id: by,
                        name: owner_name.clone(),
                        owner: true,
                        picture: None,
                        agent_until: None,
                    },
                );
            }
            DomainEvent::MemberJoined { name } => {
                let member = self.members.entry(by).or_insert(Member {
                    device_id: by,
                    name: String::new(),
                    owner: false,
                    picture: None,
                    agent_until: None,
                });
                member.name = name.clone();
            }
            DomainEvent::MemberRemoved { member_id } => {
                if self.is_head(by) && *member_id != by {
                    self.members.remove(member_id);
                }
            }
            DomainEvent::AgentAdmitted {
                member_id,
                name,
                until,
            } => {
                if self.is_head(by) && *member_id != by {
                    self.members.insert(
                        *member_id,
                        Member {
                            device_id: *member_id,
                            name: name.clone(),
                            owner: false,
                            picture: None,
                            agent_until: Some(*until),
                        },
                    );
                }
            }
            DomainEvent::MemberRenamed { name } => {
                if let Some(member) = self.members.get_mut(&by) {
                    member.name = name.clone();
                }
            }
            DomainEvent::MemberPictureChanged { picture } => {
                if let Some(member) = self.members.get_mut(&by) {
                    member.picture = Some(picture.clone());
                }
            }
            _ => return false,
        }
        true
    }
}
