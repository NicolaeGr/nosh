//! In-menu key bindings.
//!
//! A binding is `MODIFIER+...+KEY`, for example `ctrl+return`. Modifiers are
//! `ctrl`, `shift`, `alt`, and `super`. The key name is lowercase and resolved
//! against GDK key names by the UI, with `enter` matching both Return and
//! KP_Enter.

/// The action name that forces the default action through the terminal wrapper.
pub const TERMINAL_ACTION: &str = "terminal";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuKey {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub logo: bool,
    pub key: String,
}

impl MenuKey {
    /// Parses `"ctrl+shift+return"`. The key is the last `+` segment.
    pub fn parse(text: &str) -> Result<Self, String> {
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        if parts.iter().any(|part| part.is_empty()) {
            return Err("the chord has an empty part".to_string());
        }

        let (key, modifiers) = parts
            .split_last()
            .ok_or_else(|| "the chord is empty".to_string())?;

        let mut parsed = Self {
            ctrl: false,
            shift: false,
            alt: false,
            logo: false,
            key: key.to_ascii_lowercase(),
        };

        for modifier in modifiers {
            match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => parsed.ctrl = true,
                "shift" => parsed.shift = true,
                "alt" => parsed.alt = true,
                "super" | "logo" | "meta" | "mod4" => parsed.logo = true,
                other => return Err(format!("unknown modifier {other:?}")),
            }
        }

        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modifier_and_key_parse() {
        let key = MenuKey::parse("ctrl+return").unwrap();
        assert!(key.ctrl);
        assert!(!key.shift);
        assert_eq!(key.key, "return");
    }

    #[test]
    fn several_modifiers_and_case_are_accepted() {
        let key = MenuKey::parse("CTRL + Shift + F1").unwrap();
        assert!(key.ctrl);
        assert!(key.shift);
        assert_eq!(key.key, "f1");
    }

    #[test]
    fn aliases_cover_super_and_enter() {
        let key = MenuKey::parse("meta+enter").unwrap();
        assert!(key.logo);
        assert_eq!(key.key, "enter");
    }

    #[test]
    fn an_unknown_modifier_is_rejected() {
        let error = MenuKey::parse("ctr+return").unwrap_err();
        assert!(error.contains("unknown modifier"));
    }

    #[test]
    fn empty_parts_are_rejected() {
        assert!(MenuKey::parse("").is_err());
        assert!(MenuKey::parse("ctrl+").is_err());
        assert!(MenuKey::parse("+return").is_err());
    }

    #[test]
    fn a_bare_key_is_accepted() {
        let key = MenuKey::parse("return").unwrap();
        assert!(!key.ctrl && !key.shift && !key.alt && !key.logo);
        assert_eq!(key.key, "return");
    }
}
