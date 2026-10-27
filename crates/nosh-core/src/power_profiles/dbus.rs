//! The `power-profiles-daemon` D-Bus backend.
//!
//! Reads `org.freedesktop.UPower.PowerProfiles` on the system bus. The daemon
//! is a soft dependency: a missing service only makes the control unavailable.

use std::collections::HashMap;

use async_channel::Sender;
use async_trait::async_trait;
use futures_util::StreamExt;
use tokio::sync::OnceCell;
use zbus::zvariant::OwnedValue;

use crate::error::BackendError;
use crate::power_profiles::backend::{PowerProfilesBackend, PowerProfilesBackendEvent};
use crate::power_profiles::model::{default_profiles, PowerProfile, PowerProfilesModel};

pub const POWER_PROFILES_SERVICE: &str = "org.freedesktop.UPower.PowerProfiles";
pub const POWER_PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
pub const POWER_PROFILES_INTERFACE: &str = "org.freedesktop.UPower.PowerProfiles";
pub const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

/// Reads and writes the active profile over the system bus.
pub struct DbusPowerProfilesBackend {
    connection: OnceCell<zbus::Connection>,
}

impl DbusPowerProfilesBackend {
    pub fn new() -> Self {
        Self {
            connection: OnceCell::new(),
        }
    }

    async fn connection(&self) -> Result<&zbus::Connection, BackendError> {
        self.connection
            .get_or_try_init(|| async {
                zbus::Connection::system()
                    .await
                    .map_err(|error| BackendError::Unavailable(error.to_string()))
            })
            .await
    }

    async fn properties(
        &self,
        connection: &zbus::Connection,
    ) -> Result<HashMap<String, OwnedValue>, BackendError> {
        let proxy = zbus::Proxy::new(
            connection,
            POWER_PROFILES_SERVICE,
            POWER_PROFILES_PATH,
            PROPERTIES_INTERFACE,
        )
        .await
        .map_err(dbus_error)?;
        proxy
            .call("GetAll", &(POWER_PROFILES_INTERFACE,))
            .await
            .map_err(dbus_error)
    }
}

impl Default for DbusPowerProfilesBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PowerProfilesBackend for DbusPowerProfilesBackend {
    fn name(&self) -> &'static str {
        "power-profiles-daemon"
    }

    async fn probe(&self) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        self.properties(connection).await?;
        Ok(())
    }

    async fn read(&self) -> Result<PowerProfilesModel, BackendError> {
        let connection = self.connection().await?;
        let values = self.properties(connection).await?;
        Ok(model_from_map(&values))
    }

    async fn set(&self, profile: &PowerProfile) -> Result<(), BackendError> {
        let connection = self.connection().await?;

        // Current daemons expose `ActiveProfile` as a writable property.
        let properties = zbus::Proxy::new(
            connection,
            POWER_PROFILES_SERVICE,
            POWER_PROFILES_PATH,
            PROPERTIES_INTERFACE,
        )
        .await
        .map_err(dbus_error)?;
        let value = zbus::zvariant::Value::from(profile.as_str());
        let result = properties
            .call::<_, _, ()>("Set", &(POWER_PROFILES_INTERFACE, "ActiveProfile", value))
            .await;
        match result {
            Ok(()) => return Ok(()),
            Err(error) if missing_property_setter(&error) => {
                tracing::debug!(
                    %error,
                    "the daemon has no writable ActiveProfile, trying the legacy method"
                );
            }
            Err(error) => return Err(dbus_error(error)),
        }

        // Older daemons expose `SetActiveProfile` instead.
        let proxy = zbus::Proxy::new(
            connection,
            POWER_PROFILES_SERVICE,
            POWER_PROFILES_PATH,
            POWER_PROFILES_INTERFACE,
        )
        .await
        .map_err(dbus_error)?;
        proxy
            .call::<_, _, ()>("SetActiveProfile", &(profile.as_str(),))
            .await
            .map_err(dbus_error)
    }

    async fn watch(&self, events: Sender<PowerProfilesBackendEvent>) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let proxy = zbus::Proxy::new(
            connection,
            POWER_PROFILES_SERVICE,
            POWER_PROFILES_PATH,
            PROPERTIES_INTERFACE,
        )
        .await
        .map_err(dbus_error)?;
        let mut stream = proxy
            .receive_signal("PropertiesChanged")
            .await
            .map_err(dbus_error)?;
        let mut owner_changed = proxy.receive_owner_changed().await.map_err(dbus_error)?;

        loop {
            tokio::select! {
                message = stream.next() => {
                    let Some(message) = message else {
                        break;
                    };
                    let Ok((interface, _, _)) = message
                        .body()
                        .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
                    else {
                        continue;
                    };

                    if interface != POWER_PROFILES_INTERFACE {
                        continue;
                    }

                    if events
                        .send(PowerProfilesBackendEvent::Changed)
                        .await
                        .is_err()
                    {
                        return Ok(());
                    }
                }
                owner = owner_changed.next() => {
                    match owner {
                        Some(Some(_)) => {
                            if events
                                .send(PowerProfilesBackendEvent::Reconnected)
                                .await
                                .is_err()
                            {
                                return Ok(());
                            }
                        }
                        Some(None) | None => break,
                    }
                }
            }
        }

        Err(BackendError::Unavailable(
            "the power profiles service left the bus".to_string(),
        ))
    }
}

fn dbus_error(error: zbus::Error) -> BackendError {
    BackendError::Unavailable(error.to_string())
}

/// Whether the daemon rejected the property write because it only has the
/// older `SetActiveProfile` method.
fn missing_property_setter(error: &zbus::Error) -> bool {
    let zbus::Error::MethodError(name, _, _) = error else {
        return false;
    };
    matches!(
        name.as_str(),
        "org.freedesktop.DBus.Error.UnknownMethod"
            | "org.freedesktop.DBus.Error.UnknownProperty"
            | "org.freedesktop.DBus.Error.PropertyReadOnly"
            | "org.freedesktop.DBus.Error.InvalidArgs"
    )
}

fn model_from_map(values: &HashMap<String, OwnedValue>) -> PowerProfilesModel {
    let active = get_string(values, "ActiveProfile")
        .map(|value| PowerProfile::parse(&value))
        .unwrap_or_default();
    let profiles = profiles_from_map(values);
    let degraded = get_string(values, "PerformanceDegraded").filter(|value| !value.is_empty());

    PowerProfilesModel {
        active,
        profiles,
        degraded,
    }
}

fn profiles_from_map(values: &HashMap<String, OwnedValue>) -> Vec<PowerProfile> {
    let Some(value) = values.get("Profiles") else {
        return default_profiles();
    };
    let Ok(entries) = Vec::<HashMap<String, OwnedValue>>::try_from(value.clone()) else {
        return default_profiles();
    };

    let profiles: Vec<PowerProfile> = entries
        .iter()
        .filter_map(|entry| profile_name(entry).map(|value| PowerProfile::parse(&value)))
        .collect();
    if profiles.is_empty() {
        default_profiles()
    } else {
        profiles
    }
}

fn get_string(values: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    values
        .get(key)
        .and_then(|value| String::try_from(value.clone()).ok())
}

/// The entry key is `Profile` in current daemons. Accept the lower-case name
/// too so older dictionaries parse.
fn profile_name(entry: &HashMap<String, OwnedValue>) -> Option<String> {
    get_string(entry, "Profile").or_else(|| get_string(entry, "profile"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, OwnedValue)]) -> HashMap<String, OwnedValue> {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect()
    }

    fn value<T: Into<zbus::zvariant::Value<'static>>>(value: T) -> OwnedValue {
        OwnedValue::try_from(value.into()).unwrap()
    }

    fn profile_entry(name: &str) -> HashMap<String, OwnedValue> {
        map(&[("Profile", value(name.to_string()))])
    }

    #[test]
    fn lower_case_profile_keys_are_accepted() {
        let values = map(&[
            ("ActiveProfile", value("balanced")),
            (
                "Profiles",
                value(vec![HashMap::from([(
                    "profile".to_string(),
                    value("balanced"),
                )])]),
            ),
        ]);

        assert_eq!(
            model_from_map(&values).profiles,
            vec![PowerProfile::Balanced]
        );
    }

    #[test]
    fn all_properties_are_read() {
        let values = map(&[
            ("ActiveProfile", value("performance")),
            (
                "Profiles",
                value(vec![
                    profile_entry("power-saver"),
                    profile_entry("balanced"),
                    profile_entry("performance"),
                ]),
            ),
            ("PerformanceDegraded", value("CPU throttled")),
        ]);

        let model = model_from_map(&values);
        assert_eq!(model.active, PowerProfile::Performance);
        assert_eq!(model.profiles, default_profiles());
        assert_eq!(model.degraded.as_deref(), Some("CPU throttled"));
    }

    #[test]
    fn an_empty_degraded_reason_is_none() {
        let values = map(&[
            ("ActiveProfile", value("balanced")),
            ("PerformanceDegraded", value("")),
        ]);

        assert_eq!(model_from_map(&values).degraded, None);
    }

    #[test]
    fn a_missing_profile_list_falls_back_to_the_canonical_order() {
        let values = map(&[("ActiveProfile", value("balanced"))]);

        assert_eq!(model_from_map(&values).profiles, default_profiles());
    }

    #[test]
    fn unknown_profiles_are_kept() {
        let values = map(&[
            ("ActiveProfile", value("turbo")),
            ("Profiles", value(vec![profile_entry("turbo")])),
        ]);

        let model = model_from_map(&values);
        assert_eq!(model.active, PowerProfile::Unknown("turbo".to_string()));
        assert_eq!(
            model.profiles,
            vec![PowerProfile::Unknown("turbo".to_string())]
        );
    }

    #[test]
    fn missing_properties_use_the_default_model() {
        assert_eq!(
            model_from_map(&HashMap::new()),
            PowerProfilesModel::default()
        );
    }
}
