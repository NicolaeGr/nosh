//! The UPower display device backend.
//!
//! Reads `org.freedesktop.UPower.Device` on the system bus. When the display
//! device path is missing, it enumerates the devices and picks the first
//! battery that supplies power.

use std::collections::HashMap;

use async_channel::Sender;
use async_trait::async_trait;
use futures_util::StreamExt;
use tokio::sync::OnceCell;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::battery::backend::{BatteryBackend, BatteryBackendEvent};
use crate::battery::model::{BatteryProperties, TYPE_BATTERY};
use crate::error::BackendError;

pub const UPOWER_SERVICE: &str = "org.freedesktop.UPower";
pub const UPOWER_PATH: &str = "/org/freedesktop/UPower";
pub const UPOWER_INTERFACE: &str = "org.freedesktop.UPower";
pub const DEVICE_INTERFACE: &str = "org.freedesktop.UPower.Device";
pub const DISPLAY_DEVICE_PATH: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
pub const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

/// Reads the UPower display device over the system bus.
pub struct UpowerBackend {
    connection: OnceCell<zbus::Connection>,
}

impl UpowerBackend {
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

    /// The display device path, falling back to the first power-supplying
    /// battery in `EnumerateDevices`.
    async fn display_path(&self, connection: &zbus::Connection) -> Result<String, BackendError> {
        if read_properties(connection, DISPLAY_DEVICE_PATH)
            .await
            .is_ok()
        {
            return Ok(DISPLAY_DEVICE_PATH.to_string());
        }

        let upower = zbus::Proxy::new(connection, UPOWER_SERVICE, UPOWER_PATH, UPOWER_INTERFACE)
            .await
            .map_err(dbus_error)?;
        let paths: Vec<OwnedObjectPath> = upower
            .call("EnumerateDevices", &())
            .await
            .map_err(dbus_error)?;

        for path in paths {
            if let Ok(properties) = read_properties(connection, path.as_str()).await {
                if properties.kind == TYPE_BATTERY && properties.power_supply {
                    return Ok(path.as_str().to_string());
                }
            }
        }

        Err(BackendError::Unavailable(
            "UPower reports no battery device".to_string(),
        ))
    }
}

impl Default for UpowerBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BatteryBackend for UpowerBackend {
    fn name(&self) -> &'static str {
        "upower"
    }

    async fn probe(&self) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        self.display_path(connection).await?;
        Ok(())
    }

    async fn read(&self) -> Result<BatteryProperties, BackendError> {
        let connection = self.connection().await?;
        let path = self.display_path(connection).await?;
        read_properties(connection, &path).await
    }

    async fn watch(&self, events: Sender<BatteryBackendEvent>) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let path = self.display_path(connection).await?;
        let proxy = zbus::Proxy::new(
            connection,
            UPOWER_SERVICE,
            path.as_str(),
            PROPERTIES_INTERFACE,
        )
        .await
        .map_err(dbus_error)?;
        let mut stream = proxy
            .receive_signal("PropertiesChanged")
            .await
            .map_err(dbus_error)?;
        // zbus keeps a well-known-name signal stream open across an owner
        // restart. Watch the owner explicitly so UPower coming back produces a
        // fresh snapshot instead of waiting for a property to change.
        let mut owner_changed = proxy.receive_owner_changed().await.map_err(dbus_error)?;

        loop {
            tokio::select! {
                message = stream.next() => {
                    let Some(message) = message else {
                        break;
                    };
                    let body = message.body();
                    let Ok((interface, _, _)) =
                        body.deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
                    else {
                        continue;
                    };

                    if interface != DEVICE_INTERFACE {
                        continue;
                    }

                    if events.send(BatteryBackendEvent::Changed).await.is_err() {
                        return Ok(());
                    }
                }
                owner = owner_changed.next() => {
                    match owner {
                        // The name has a new owner. Re-read the device.
                        Some(Some(_)) => {
                            if events
                                .send(BatteryBackendEvent::Reconnected)
                                .await
                                .is_err()
                            {
                                return Ok(());
                            }
                        }
                        // UPower left the bus, or the owner stream ended.
                        Some(None) | None => break,
                    }
                }
            }
        }

        Err(BackendError::Unavailable(
            "the UPower service left the bus".to_string(),
        ))
    }
}

fn dbus_error(error: zbus::Error) -> BackendError {
    BackendError::Unavailable(error.to_string())
}

async fn read_properties(
    connection: &zbus::Connection,
    path: &str,
) -> Result<BatteryProperties, BackendError> {
    let proxy = zbus::Proxy::new(connection, UPOWER_SERVICE, path, PROPERTIES_INTERFACE)
        .await
        .map_err(dbus_error)?;
    let values: HashMap<String, OwnedValue> = proxy
        .call("GetAll", &(DEVICE_INTERFACE,))
        .await
        .map_err(dbus_error)?;

    Ok(properties_from_map(&values))
}

fn properties_from_map(values: &HashMap<String, OwnedValue>) -> BatteryProperties {
    BatteryProperties {
        kind: get::<u32>(values, "Type").unwrap_or_default(),
        percentage: get::<f64>(values, "Percentage").unwrap_or_default(),
        state: get::<u32>(values, "State").unwrap_or_default(),
        present: get::<bool>(values, "IsPresent").unwrap_or_default(),
        power_supply: get::<bool>(values, "PowerSupply").unwrap_or_default(),
        time_to_empty: get::<i64>(values, "TimeToEmpty").unwrap_or_default(),
        time_to_full: get::<i64>(values, "TimeToFull").unwrap_or_default(),
    }
}

fn get<T>(values: &HashMap<String, OwnedValue>, key: &str) -> Option<T>
where
    for<'a> T: TryFrom<&'a OwnedValue>,
{
    values.get(key).and_then(|value| T::try_from(value).ok())
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

    #[test]
    fn all_device_properties_are_read() {
        let values = map(&[
            ("Type", value(2u32)),
            ("Percentage", value(54.5f64)),
            ("State", value(2u32)),
            ("IsPresent", value(true)),
            ("PowerSupply", value(true)),
            ("TimeToEmpty", value(3600i64)),
            ("TimeToFull", value(0i64)),
        ]);

        assert_eq!(
            properties_from_map(&values),
            BatteryProperties {
                kind: 2,
                percentage: 54.5,
                state: 2,
                present: true,
                power_supply: true,
                time_to_empty: 3600,
                time_to_full: 0,
            }
        );
    }

    #[test]
    fn missing_properties_use_defaults() {
        let values = map(&[("Percentage", value(10.0f64))]);

        assert_eq!(
            properties_from_map(&values),
            BatteryProperties {
                percentage: 10.0,
                ..BatteryProperties::default()
            }
        );
    }

    #[test]
    fn wrong_property_types_use_defaults() {
        let values = map(&[("Percentage", value("not a number"))]);

        assert_eq!(properties_from_map(&values).percentage, 0.0);
    }
}
