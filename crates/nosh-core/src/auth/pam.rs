//! A small binding to the PAM client library.
//!
//! The library is opened at runtime, so a system without PAM still builds and
//! starts the shell. PAM blocks during the conversation, so the service calls
//! this from a blocking thread.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const PAM_SUCCESS: c_int = 0;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
const PAM_CONV_ERR: c_int = 19;
const PAM_DISALLOW_NULL_AUTHTOK: c_int = 0x0001;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

#[repr(C)]
struct PamConv {
    conv: Option<Conversation>,
    appdata_ptr: *mut c_void,
}

type Conversation =
    unsafe extern "C" fn(c_int, *mut *mut PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int;

type Start =
    unsafe extern "C" fn(*const c_char, *const c_char, *const PamConv, *mut *mut c_void) -> c_int;
type Authenticate = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type AccountManagement = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type End = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type StrError = unsafe extern "C" fn(*mut c_void, c_int) -> *const c_char;

struct PamApi {
    _handle: *mut c_void,
    start: Start,
    authenticate: Authenticate,
    account_management: AccountManagement,
    end: End,
    strerror: StrError,
}

// The function pointers are immutable and PAM keeps state per handle, so one
// loaded library is shared by every caller.
unsafe impl Send for PamApi {}
unsafe impl Sync for PamApi {}

/// The conversation state. PAM receives a pointer to this for the duration of
/// one authentication.
struct ConversationData {
    username: CString,
    password: CString,
}

/// Loads the library once. A `None` value means PAM is not installed.
fn api() -> Option<&'static PamApi> {
    static API: OnceLock<Option<PamApi>> = OnceLock::new();
    API.get_or_init(load).as_ref()
}

/// Whether PAM can be loaded.
pub fn available() -> bool {
    api().is_some()
}

fn load() -> Option<PamApi> {
    for candidate in candidates() {
        let Ok(name) = CString::new(candidate.to_string_lossy().as_bytes()) else {
            continue;
        };
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            continue;
        }

        let Some(api) = resolve(handle) else {
            unsafe { libc::dlclose(handle) };
            continue;
        };
        tracing::debug!(path = %candidate.display(), "loaded libpam");
        return Some(api);
    }

    tracing::warn!("libpam was not found, so password prompts are unavailable");
    None
}

fn resolve(handle: *mut c_void) -> Option<PamApi> {
    unsafe {
        Some(PamApi {
            _handle: handle,
            start: symbol(handle, "pam_start")?,
            authenticate: symbol(handle, "pam_authenticate")?,
            account_management: symbol(handle, "pam_acct_mgmt")?,
            end: symbol(handle, "pam_end")?,
            strerror: symbol(handle, "pam_strerror")?,
        })
    }
}

unsafe fn symbol<T: Copy>(handle: *mut c_void, name: &str) -> Option<T> {
    let name = CString::new(name).ok()?;
    let pointer = libc::dlsym(handle, name.as_ptr());
    if pointer.is_null() {
        return None;
    }
    Some(std::mem::transmute_copy::<*mut c_void, T>(&pointer))
}

fn candidates() -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from("libpam.so.0")];

    if let Ok(value) = std::env::var("LD_LIBRARY_PATH") {
        for directory in value.split(':').filter(|entry| !entry.is_empty()) {
            paths.push(Path::new(directory).join("libpam.so.0"));
        }
    }

    paths.push(PathBuf::from("/run/current-system/sw/lib/libpam.so.0"));

    // NixOS keeps libraries outside the loader search path. A shell that runs
    // from the system profile finds PAM through the entry above; anything else
    // needs the store lookup.
    if let Ok(entries) = std::fs::read_dir("/nix/store") {
        let mut stores: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains("-linux-pam-"))
            })
            .collect();
        stores.sort();
        for store in stores {
            paths.push(store.join("lib/libpam.so.0"));
        }
    }

    paths
}

/// Whether a PAM service file exists. PAM itself reports a missing service as
/// a start error, but the UI needs to know ahead of time.
pub fn service_exists(service: &str) -> bool {
    Path::new("/etc/pam.d").join(service).exists()
}

/// Checks a password against a PAM service.
pub fn authenticate(service: &str, username: &str, password: &[u8]) -> Result<(), String> {
    let Some(api) = api() else {
        return Err("PAM is not available on this system".to_string());
    };

    let service = CString::new(service).map_err(|_| "invalid PAM service name".to_string())?;
    let user = CString::new(username).map_err(|_| "invalid user name".to_string())?;
    let password =
        CString::new(password).map_err(|_| "the password contains an invalid character")?;

    let data = Box::into_raw(Box::new(ConversationData {
        username: user.clone(),
        password,
    }));
    let conv = PamConv {
        conv: Some(conversation),
        appdata_ptr: data.cast(),
    };

    let mut handle: *mut c_void = std::ptr::null_mut();
    let started = unsafe { (api.start)(service.as_ptr(), user.as_ptr(), &conv, &mut handle) };
    if started != PAM_SUCCESS {
        unsafe { drop(Box::from_raw(data)) };
        return Err(message(api, handle, started));
    }

    let auth = unsafe { (api.authenticate)(handle, PAM_DISALLOW_NULL_AUTHTOK) };
    let result = if auth != PAM_SUCCESS {
        Err(message(api, handle, auth))
    } else {
        let account = unsafe { (api.account_management)(handle, 0) };
        if account == PAM_SUCCESS {
            Ok(())
        } else {
            Err(message(api, handle, account))
        }
    };

    unsafe {
        (api.end)(handle, auth);
        drop(Box::from_raw(data));
    }
    result
}

fn message(api: &PamApi, handle: *mut c_void, code: c_int) -> String {
    let pointer = unsafe { (api.strerror)(handle, code) };
    if pointer.is_null() {
        return format!("authentication failed (code {code})");
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    if text.is_empty() {
        format!("authentication failed (code {code})")
    } else {
        text
    }
}

unsafe extern "C" fn conversation(
    count: c_int,
    messages: *mut *mut PamMessage,
    responses: *mut *mut PamResponse,
    data: *mut c_void,
) -> c_int {
    if count <= 0 || messages.is_null() || responses.is_null() || data.is_null() {
        return PAM_CONV_ERR;
    }

    let data = &*(data as *const ConversationData);
    let count = count as usize;
    let mut answers: Vec<PamResponse> = Vec::with_capacity(count);

    for index in 0..count {
        let message = *messages.add(index);
        if message.is_null() {
            free_answers(&mut answers);
            return PAM_CONV_ERR;
        }

        let source = match (*message).msg_style {
            PAM_PROMPT_ECHO_OFF => data.password.as_ptr(),
            PAM_PROMPT_ECHO_ON => data.username.as_ptr(),
            PAM_ERROR_MSG | PAM_TEXT_INFO => std::ptr::null(),
            _ => {
                free_answers(&mut answers);
                return PAM_CONV_ERR;
            }
        };

        // PAM takes ownership of the response and frees it with `free`.
        let response = if source.is_null() {
            std::ptr::null_mut()
        } else {
            libc::strdup(source)
        };
        if !source.is_null() && response.is_null() {
            free_answers(&mut answers);
            return PAM_CONV_ERR;
        }

        answers.push(PamResponse {
            resp: response,
            resp_retcode: 0,
        });
    }

    *responses = answers.as_mut_ptr();
    std::mem::forget(answers);
    PAM_SUCCESS
}

unsafe fn free_answers(answers: &mut [PamResponse]) {
    for answer in answers.iter() {
        if !answer.resp.is_null() {
            libc::free(answer.resp.cast());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_service_is_reported() {
        assert!(!service_exists("nosh-service-that-does-not-exist"));
    }

    #[test]
    fn the_conversation_rejects_a_null_data_pointer() {
        let mut messages: *mut PamMessage = std::ptr::null_mut();
        let mut responses: *mut PamResponse = std::ptr::null_mut();
        let code = unsafe { conversation(1, &mut messages, &mut responses, std::ptr::null_mut()) };
        assert_eq!(code, PAM_CONV_ERR);
    }

    #[test]
    #[ignore = "needs a PAM service and the current user"]
    fn a_wrong_password_is_rejected() {
        if !available() || !service_exists("login") {
            return;
        }
        let user = std::env::var("USER").unwrap_or_else(|_| "nobody".to_string());
        let result = authenticate("login", &user, b"nosh-not-a-password");
        assert!(result.is_err());
    }
}
