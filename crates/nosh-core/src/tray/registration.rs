//! Parsing of `RegisterStatusNotifierItem` arguments.
//!
//! A registration string can be a bus name, a bus name and an object path, or
//! only an object path. The path-only form needs the sender from the D-Bus
//! message header.

/// One status notifier item address.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Registration {
    pub bus_name: String,
    pub object_path: String,
}

/// The default object path for a registration that only names a bus.
pub const DEFAULT_OBJECT_PATH: &str = "/StatusNotifierItem";

impl Registration {
    /// Parses a registration string in the documented order.
    ///
    /// 1. A string starting with `/` is an object path. The sender supplies the
    ///    bus name.
    /// 2. A string containing `/` is a bus name followed by an object path.
    /// 3. Anything else is a bus name and the default object path is used.
    pub fn parse(service: &str, sender: Option<&str>) -> Option<Self> {
        let service = service.trim();
        if service.is_empty() {
            return None;
        }

        if service.starts_with('/') {
            let sender = sender?.trim();
            if sender.is_empty() || service.len() < 2 {
                return None;
            }
            return Some(Self {
                bus_name: sender.to_string(),
                object_path: service.to_string(),
            });
        }

        if let Some((bus_name, rest)) = service.split_once('/') {
            if bus_name.is_empty() || rest.is_empty() {
                return None;
            }
            return Some(Self {
                bus_name: bus_name.to_string(),
                object_path: format!("/{rest}"),
            });
        }

        Some(Self {
            bus_name: service.to_string(),
            object_path: DEFAULT_OBJECT_PATH.to_string(),
        })
    }

    /// The identifier the watcher and the UI use. The bus name cannot contain a
    /// slash, so the concatenation is unambiguous.
    pub fn item_id(&self) -> String {
        format!("{}{}", self.bus_name, self.object_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_path_uses_the_sender_as_the_bus_name() {
        assert_eq!(
            Registration::parse("/StatusNotifierItem", Some(":1.42")),
            Some(Registration {
                bus_name: ":1.42".to_string(),
                object_path: "/StatusNotifierItem".to_string(),
            })
        );
        assert_eq!(
            Registration::parse("/org/example/Tray", Some(":1.42")),
            Some(Registration {
                bus_name: ":1.42".to_string(),
                object_path: "/org/example/Tray".to_string(),
            })
        );
    }

    #[test]
    fn an_object_path_without_a_sender_is_rejected() {
        assert_eq!(Registration::parse("/StatusNotifierItem", None), None);
        assert_eq!(Registration::parse("/StatusNotifierItem", Some("")), None);
    }

    #[test]
    fn a_combined_service_and_path_is_split_at_the_first_slash() {
        assert_eq!(
            Registration::parse("org.example.App/StatusNotifierItem", None),
            Some(Registration {
                bus_name: "org.example.App".to_string(),
                object_path: "/StatusNotifierItem".to_string(),
            })
        );
        assert_eq!(
            Registration::parse(":1.42/org/example/Tray", None),
            Some(Registration {
                bus_name: ":1.42".to_string(),
                object_path: "/org/example/Tray".to_string(),
            })
        );
    }

    #[test]
    fn a_bare_service_uses_the_default_path() {
        assert_eq!(
            Registration::parse("org.example.App", None),
            Some(Registration {
                bus_name: "org.example.App".to_string(),
                object_path: DEFAULT_OBJECT_PATH.to_string(),
            })
        );
        assert_eq!(
            Registration::parse(":1.42", None),
            Some(Registration {
                bus_name: ":1.42".to_string(),
                object_path: DEFAULT_OBJECT_PATH.to_string(),
            })
        );
    }

    #[test]
    fn malformed_registrations_are_rejected() {
        assert_eq!(Registration::parse("", None), None);
        assert_eq!(Registration::parse("   ", None), None);
        assert_eq!(Registration::parse("/", Some(":1.2")), None);
        assert_eq!(Registration::parse("/app", None), None);
        assert_eq!(Registration::parse("bus/", None), None);
        assert_eq!(Registration::parse("/bus/path", None), None);
    }

    #[test]
    fn the_item_id_is_the_bus_name_and_path() {
        let registration = Registration::parse(":1.42/StatusNotifierItem", None).unwrap();
        assert_eq!(registration.item_id(), ":1.42/StatusNotifierItem");
    }
}
