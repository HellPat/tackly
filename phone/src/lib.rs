//! Everything a phone does without a screen: the use cases (`device`), keeping
//! in sync (`live`), and the technical parts they stand on (`infrastructure`).

pub mod device;
pub mod infrastructure;
pub mod live;

pub use device::{
    AGENT_NAME, Device, InviteProgress, InviteTicket, JoinRequest, LiveTarget, Membership,
    RecoveryKey,
};
pub use infrastructure::geocoder::Geocoder;
pub use live::{SharedDevice, run_live};
