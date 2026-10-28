//! The freedesktop notification daemon, store, and persistence.

pub mod backend;
pub mod image;
pub mod model;
pub mod persistence;
pub mod service;
pub mod store;

pub use backend::{
    default_candidates, DaemonRole, ExistingDaemonProxyBackend, IncomingRequest,
    NotificationBackend, NotificationBackendRef, NotificationCandidate, NotificationSession,
    NotifyRequest, NullNotificationBackend, OwnDaemonBackend, NOTIFICATION_INTERFACE,
    NOTIFICATION_PATH, NOTIFICATION_SERVICE,
};
pub use image::{cache_image_value, normalize_image_hints, MAX_DIMENSION, MAX_RAW_BYTES};
pub use model::{
    hint_bool, hint_i32, hint_string, hint_u8, parse_actions, sanitize_body, sanitize_label,
    ClosedReason, Notification, NotificationAction, NotificationModel, NotificationSettings,
    NotificationState, Urgency, ACTION_DISMISS, ACTION_EXPIRE, DEFAULT_CAPABILITIES,
    HINT_ACTION_ICONS, HINT_ACTIVATION_TOKEN, HINT_BODY_HYPERLINKS, HINT_BODY_IMAGES,
    HINT_CATEGORY, HINT_DESKTOP_ENTRY, HINT_ICON_DATA, HINT_IMAGE_DATA, HINT_IMAGE_DATA_LEGACY,
    HINT_IMAGE_PATH, HINT_RESIDENT, HINT_SOUND_FILE, HINT_SOUND_NAME, HINT_SUPPRESS_SOUND,
    HINT_TRANSIENT, HINT_URGENCY, HINT_X, HINT_Y, SERVER_NAME, SERVER_VENDOR, SERVER_VERSION,
    SPEC_VERSION,
};
pub use persistence::{load, restore, save, PersistedNotification, PersistedState};
pub use service::{NotificationService, NotificationServiceConfig, FLUSH_DELAY};
pub use store::NotificationStore;
