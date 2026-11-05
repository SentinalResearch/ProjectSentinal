//! Which daemon install speaks for each Machine, and refused claimants.

use crate::{Store, StoreError};
use protocol::{MachineId, Timestamp};
use rusqlite::{params, OptionalExtension};

/// The install currently speaking for a Machine, and the last one refused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MachineInstance {
    /// Random ID of the accepted install.
    pub instance_id: Option<String>,
    /// Commit the accepted install was built from.
    pub build_commit: Option<String>,
    /// When another install last claimed this Machine and was refused.
    pub conflict_at: Option<Timestamp>,
    /// That install's version.
    pub conflict_version: Option<String>,
    /// That install's commit.
    pub conflict_commit: Option<String>,
    /// That install's port.
    pub conflict_port: Option<u16>,
}

impl Store {
    /// What is known about the install speaking for `machine_id`.
    pub fn machine_instance(&self, machine_id: &MachineId) -> Result<MachineInstance, StoreError> {
        let row = self
            .lock()?
            .query_row(
                "SELECT instance_id, build_commit, conflict_at, conflict_version, conflict_commit,
                        conflict_port
                 FROM machine_instances WHERE machine_id = ?1",
                params![machine_id.0],
                |row| {
                    Ok(MachineInstance {
                        instance_id: row.get(0)?,
                        build_commit: row.get(1)?,
                        conflict_at: row.get::<_, Option<i64>>(2)?.map(Timestamp),
                        conflict_version: row.get(3)?,
                        conflict_commit: row.get(4)?,
                        conflict_port: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row.unwrap_or_default())
    }

    /// Remember the install whose heartbeat was accepted. An older daemon that
    /// reports no instance keeps the recorded one, so it cannot erase it.
    pub fn record_machine_instance(
        &self,
        machine_id: &MachineId,
        instance_id: Option<&str>,
        build_commit: Option<&str>,
    ) -> Result<(), StoreError> {
        self.lock()?.execute(
            "INSERT INTO machine_instances(machine_id, instance_id, build_commit)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(machine_id) DO UPDATE SET
                instance_id = COALESCE(excluded.instance_id, instance_id),
                build_commit = excluded.build_commit",
            params![machine_id.0, instance_id, build_commit],
        )?;
        Ok(())
    }

    /// Remember that another install claimed this Machine and was refused.
    pub fn record_machine_conflict(
        &self,
        machine_id: &MachineId,
        at: Timestamp,
        version: &str,
        build_commit: Option<&str>,
        port: u16,
    ) -> Result<(), StoreError> {
        self.lock()?.execute(
            "INSERT INTO machine_instances(
                machine_id, conflict_at, conflict_version, conflict_commit, conflict_port)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(machine_id) DO UPDATE SET
                conflict_at = excluded.conflict_at,
                conflict_version = excluded.conflict_version,
                conflict_commit = excluded.conflict_commit,
                conflict_port = excluded.conflict_port",
            params![machine_id.0, at.0, version, build_commit, port],
        )?;
        Ok(())
    }
}
