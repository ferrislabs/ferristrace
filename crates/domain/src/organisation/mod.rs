mod entities;
mod errors;
mod org_settings;
#[cfg(test)]
mod org_settings_tests;
mod ports;
mod roster;
#[cfg(test)]
mod roster_tests;
mod squad;
#[cfg(test)]
mod squad_tests;
mod switching;
#[cfg(test)]
mod switching_tests;
mod value_objects;
mod whoami;
#[cfg(test)]
mod whoami_tests;

pub use entities::{Holder, Joined, Member, Membership, Organisation, Team};
pub use errors::{
    BadName, BadTeamName, RosterError, SettingError, SwitchError, TeamError, WhoamiError,
};
pub use org_settings::OrgSettings;
pub use ports::{Memberships, Orgs, Roll, Squads};
pub use roster::Roster;
pub use squad::Squad;
pub use switching::Switching;
pub use value_objects::{Invitation, Named, OrgName, OrgRole, Owned, TeamId, TeamName, Who};
pub use whoami::Whoami;
