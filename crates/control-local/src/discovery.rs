//! Private descriptor publication and deterministic running-instance discovery.
//!
//! Both halves of ADR 0001's descriptor directory live here so the running
//! application (which publishes) and a client such as the MCP adapter (which
//! discovers) verify ownership and permissions with one implementation. The
//! directory is private to the current user on every platform; a descriptor
//! that fails that check is an issue, never a candidate.

use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use quantick_control::{descriptor::InstanceDescriptor, limits::CONTROL_DESCRIPTOR_MAX_BYTES};

use quantick_control::limits::CONTROL_DISCOVERY_MAX_ENTRIES;

const NO_INSTANCE_NEXT_STEP: &str =
    "Start Quantick, open Tools > Local agent access, and enable observer access.";
#[cfg(windows)]
const WINDOWS_MIN_SID_BYTES: usize = 8;

#[derive(Debug)]
pub struct PublishedDescriptor {
    path: Option<PathBuf>,
}

impl PublishedDescriptor {
    pub fn path(&self) -> &Path {
        self.path
            .as_deref()
            .expect("a live published descriptor retains its path")
    }

    pub fn remove(mut self) -> Result<(), DiscoveryError> {
        let path = self
            .path
            .take()
            .expect("a live published descriptor retains its path");
        remove_owned_descriptor(&path)
    }
}

impl Drop for PublishedDescriptor {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = remove_owned_descriptor(&path);
        }
    }
}

#[derive(Clone, Debug)]
pub struct DescriptorCandidate {
    pub descriptor: InstanceDescriptor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryIssue {
    pub file_name: String,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct DescriptorDiscovery {
    pub candidates: Vec<DescriptorCandidate>,
    pub issues: Vec<DiscoveryIssue>,
    pub next_steps: Vec<String>,
}

impl DescriptorDiscovery {
    fn finish(mut self) -> Self {
        self.candidates.sort_by(|left, right| {
            (
                left.descriptor.published_at_unix_ms,
                &left.descriptor.instance_id,
            )
                .cmp(&(
                    right.descriptor.published_at_unix_ms,
                    &right.descriptor.instance_id,
                ))
        });
        if self.candidates.is_empty() {
            self.next_steps.push(NO_INSTANCE_NEXT_STEP.to_owned());
        }
        self
    }
}

/// The platform base directory and the components beneath it that hold the
/// instance descriptors (ADR 0001 §4). Linux fails closed without an owned
/// `XDG_RUNTIME_DIR`: a shared temporary directory is no place for a bearer
/// token.
fn platform_runtime_base() -> Result<(PathBuf, &'static [&'static str]), DiscoveryError> {
    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| {
                DiscoveryError::new(
                    "XDG_RUNTIME_DIR is unavailable; local access fails closed on Linux",
                )
            })?;
        verify_unix_owner(&base)?;
        return Ok((base, &["quantick", "control", "instances"]));
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let base = dirs::data_local_dir().ok_or_else(|| {
            DiscoveryError::new("the operating system did not provide a local data directory")
        })?;
        return Ok((base, &["Quantick", "control", "instances"]));
    }

    #[allow(unreachable_code)]
    Err(DiscoveryError::new(
        "local gateway discovery is unsupported on this platform",
    ))
}

/// The private runtime directory, created and verified private on the way.
/// This is the publisher's entry point.
pub fn runtime_instances_dir() -> Result<PathBuf, DiscoveryError> {
    let (base, components) = platform_runtime_base()?;
    ensure_private_chain(&base, components)
}

/// The private runtime directory only if it already exists. Discovery uses
/// this so that looking for an instance never creates a directory a later
/// publisher would then find and trust.
pub fn runtime_instances_dir_if_present() -> Result<Option<PathBuf>, DiscoveryError> {
    let (base, components) = platform_runtime_base()?;
    let mut directory = base;
    for component in components {
        directory.push(component);
    }
    if !directory.exists() {
        return Ok(None);
    }
    Ok(Some(directory))
}

pub fn publish_descriptor(
    descriptor: &InstanceDescriptor,
) -> Result<PublishedDescriptor, DiscoveryError> {
    let directory = runtime_instances_dir()?;
    publish_descriptor_in(&directory, descriptor)
}

pub fn publish_descriptor_in(
    directory: &Path,
    descriptor: &InstanceDescriptor,
) -> Result<PublishedDescriptor, DiscoveryError> {
    descriptor
        .validate()
        .map_err(|error| DiscoveryError::owned(format!("invalid descriptor: {error}")))?;
    ensure_explicit_private_directory(directory)?;

    let encoded = serde_json::to_vec(descriptor)
        .map_err(|_| DiscoveryError::new("instance descriptor JSON encoding failed"))?;
    if encoded.len() > CONTROL_DESCRIPTOR_MAX_BYTES {
        return Err(DiscoveryError::new(
            "instance descriptor exceeds its reviewed byte limit",
        ));
    }

    let target = directory.join(descriptor.file_name());
    if target.exists() {
        let previous = read_descriptor_file(&target)?;
        if previous.instance_id != descriptor.instance_id
            || previous.process_nonce != descriptor.process_nonce
        {
            return Err(DiscoveryError::new(
                "descriptor target belongs to a different application process",
            ));
        }
        remove_owned_descriptor(&target)?;
    }

    let temporary = directory.join(format!(
        ".{}.{}.new",
        descriptor.instance_id, descriptor.process_nonce
    ));
    if temporary.exists() {
        reject_redirect(&temporary)?;
        let metadata = fs::symlink_metadata(&temporary)
            .map_err(|error| DiscoveryError::io("inspect temporary descriptor", error))?;
        if !metadata.file_type().is_file() {
            return Err(DiscoveryError::new(
                "temporary descriptor path is not a regular file",
            ));
        }
        fs::remove_file(&temporary)
            .map_err(|error| DiscoveryError::io("remove owned temporary descriptor", error))?;
    }

    let write_result = (|| {
        let mut file = private_new_file(&temporary)?;
        file.write_all(&encoded)
            .map_err(|error| DiscoveryError::io("write instance descriptor", error))?;
        file.flush()
            .map_err(|error| DiscoveryError::io("flush instance descriptor", error))?;
        file.sync_all()
            .map_err(|error| DiscoveryError::io("synchronize instance descriptor", error))?;
        verify_private_file(&temporary)?;
        fs::rename(&temporary, &target)
            .map_err(|error| DiscoveryError::io("publish instance descriptor atomically", error))?;
        verify_private_file(&target)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result?;

    Ok(PublishedDescriptor { path: Some(target) })
}

/// Discover descriptors in the platform's private runtime directory.
///
/// Creates nothing: a missing directory is an empty report with a next step,
/// never a reason to publish or to start the application (ADR 0001 §1).
pub fn discover_descriptors() -> Result<DescriptorDiscovery, DiscoveryError> {
    let directory = match runtime_instances_dir_if_present()? {
        Some(directory) => directory,
        None => {
            return Ok(DescriptorDiscovery {
                candidates: Vec::new(),
                issues: Vec::new(),
                next_steps: vec![NO_INSTANCE_NEXT_STEP.to_owned()],
            });
        }
    };
    discover_descriptors_in(&directory)
}

pub fn discover_descriptors_in(directory: &Path) -> Result<DescriptorDiscovery, DiscoveryError> {
    if !directory.exists() {
        return Ok(DescriptorDiscovery {
            candidates: Vec::new(),
            issues: Vec::new(),
            next_steps: vec![NO_INSTANCE_NEXT_STEP.to_owned()],
        });
    }
    verify_explicit_private_directory(directory)?;
    let mut report = DescriptorDiscovery {
        candidates: Vec::new(),
        issues: Vec::new(),
        next_steps: Vec::new(),
    };
    let entries = fs::read_dir(directory)
        .map_err(|error| DiscoveryError::io("read private instance directory", error))?;
    for (index, entry) in entries.enumerate() {
        if index >= CONTROL_DISCOVERY_MAX_ENTRIES {
            // A polluted directory must not make every instance undiscoverable:
            // the bounded prefix is examined and the overflow is an issue the
            // client can show, not an error that hides the live instances.
            report.issues.push(DiscoveryIssue {
                file_name: "<directory>".to_owned(),
                message: format!(
                    "more than {CONTROL_DISCOVERY_MAX_ENTRIES} entries; only the first \
                     {CONTROL_DISCOVERY_MAX_ENTRIES} were examined"
                ),
            });
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.issues.push(DiscoveryIssue {
                    file_name: "<unreadable-entry>".to_owned(),
                    message: format!("directory entry is unreadable: {error}"),
                });
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        match read_descriptor_file(&path) {
            Ok(descriptor) if descriptor.file_name() == file_name => {
                report.candidates.push(DescriptorCandidate { descriptor });
            }
            Ok(_) => report.issues.push(DiscoveryIssue {
                file_name,
                message: "descriptor file name does not match its instance ID".to_owned(),
            }),
            Err(error) => report.issues.push(DiscoveryIssue {
                file_name,
                message: error.to_string(),
            }),
        }
    }
    Ok(report.finish())
}

fn read_descriptor_file(path: &Path) -> Result<InstanceDescriptor, DiscoveryError> {
    reject_redirect(path)?;
    verify_private_file(path)?;
    let metadata = fs::metadata(path)
        .map_err(|error| DiscoveryError::io("inspect instance descriptor", error))?;
    if !metadata.is_file() {
        return Err(DiscoveryError::new(
            "instance descriptor is not a regular file",
        ));
    }
    let declared = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if declared == 0 || declared > CONTROL_DESCRIPTOR_MAX_BYTES {
        return Err(DiscoveryError::new(
            "instance descriptor has an invalid byte length",
        ));
    }
    let file =
        File::open(path).map_err(|error| DiscoveryError::io("open instance descriptor", error))?;
    let mut bytes = Vec::with_capacity(declared);
    file.take((CONTROL_DESCRIPTOR_MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| DiscoveryError::io("read instance descriptor", error))?;
    if bytes.len() > CONTROL_DESCRIPTOR_MAX_BYTES {
        return Err(DiscoveryError::new(
            "instance descriptor grew beyond its reviewed byte limit",
        ));
    }
    let descriptor: InstanceDescriptor = serde_json::from_slice(&bytes)
        .map_err(|_| DiscoveryError::new("instance descriptor is not strict versioned JSON"))?;
    descriptor
        .validate()
        .map_err(|error| DiscoveryError::owned(format!("invalid instance descriptor: {error}")))?;
    Ok(descriptor)
}

fn remove_owned_descriptor(path: &Path) -> Result<(), DiscoveryError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            reject_metadata_redirect(&metadata)?;
            if !metadata.file_type().is_file() {
                return Err(DiscoveryError::new(
                    "refusing to remove a non-regular descriptor path",
                ));
            }
            verify_private_file(path)?;
            fs::remove_file(path)
                .map_err(|error| DiscoveryError::io("remove instance descriptor", error))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DiscoveryError::io("inspect instance descriptor", error)),
    }
}

fn ensure_private_chain(base: &Path, components: &[&str]) -> Result<PathBuf, DiscoveryError> {
    if !base.is_dir() {
        return Err(DiscoveryError::new(
            "the platform runtime base is unavailable or is not a directory",
        ));
    }
    reject_redirect(base)?;
    let mut current = base.to_path_buf();
    for component in components {
        current.push(component);
        match fs::create_dir(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(DiscoveryError::io(
                    "create private instance directory",
                    error,
                ));
            }
        }
        reject_redirect(&current)?;
        if !current.is_dir() {
            return Err(DiscoveryError::new(
                "instance directory component is not a directory",
            ));
        }
    }
    set_and_verify_private_directory(&current)?;
    Ok(current)
}

fn ensure_explicit_private_directory(directory: &Path) -> Result<(), DiscoveryError> {
    if !directory.exists() {
        fs::create_dir_all(directory)
            .map_err(|error| DiscoveryError::io("create private instance directory", error))?;
    }
    reject_redirect(directory)?;
    if !directory.is_dir() {
        return Err(DiscoveryError::new(
            "instance descriptor directory is not a directory",
        ));
    }
    set_and_verify_private_directory(directory)
}

fn verify_explicit_private_directory(directory: &Path) -> Result<(), DiscoveryError> {
    reject_redirect(directory)?;
    if !directory.is_dir() {
        return Err(DiscoveryError::new(
            "instance descriptor directory is not a directory",
        ));
    }
    verify_private_directory(directory)
}

fn reject_redirect(path: &Path) -> Result<(), DiscoveryError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| DiscoveryError::io("inspect descriptor path", error))?;
    reject_metadata_redirect(&metadata)
}

fn reject_metadata_redirect(metadata: &fs::Metadata) -> Result<(), DiscoveryError> {
    if metadata.file_type().is_symlink() || metadata_is_reparse_point(metadata) {
        return Err(DiscoveryError::new(
            "descriptor path redirects through a symlink or reparse point",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn metadata_is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    metadata.file_attributes()
        & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
}

#[cfg(not(windows))]
const fn metadata_is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

fn private_new_file(path: &Path) -> Result<File, DiscoveryError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .map_err(|error| DiscoveryError::io("create private instance descriptor", error))?;
    #[cfg(windows)]
    set_private_windows_acl(path, false)?;
    Ok(file)
}

#[cfg(unix)]
fn set_and_verify_private_directory(path: &Path) -> Result<(), DiscoveryError> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| DiscoveryError::io("set private instance directory mode", error))?;
    let metadata = fs::metadata(path)
        .map_err(|error| DiscoveryError::io("inspect private instance directory", error))?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        return Err(DiscoveryError::new(
            "instance directory is not private to the current Unix user",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn set_and_verify_private_directory(path: &Path) -> Result<(), DiscoveryError> {
    verify_windows_owner(path)?;
    set_private_windows_acl(path, true)?;
    verify_windows_acl(path)
}

#[cfg(windows)]
fn verify_private_directory(path: &Path) -> Result<(), DiscoveryError> {
    verify_windows_acl(path)
}

#[cfg(unix)]
fn verify_private_directory(path: &Path) -> Result<(), DiscoveryError> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = fs::metadata(path)
        .map_err(|error| DiscoveryError::io("inspect private instance directory", error))?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        return Err(DiscoveryError::new(
            "instance directory is not private to the current Unix user",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn verify_private_file(path: &Path) -> Result<(), DiscoveryError> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = fs::metadata(path)
        .map_err(|error| DiscoveryError::io("inspect private descriptor file", error))?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        return Err(DiscoveryError::new(
            "instance descriptor is not private to the current Unix user",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn verify_private_file(path: &Path) -> Result<(), DiscoveryError> {
    verify_windows_acl(path)
}

#[cfg(target_os = "linux")]
fn verify_unix_owner(path: &Path) -> Result<(), DiscoveryError> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata =
        fs::metadata(path).map_err(|error| DiscoveryError::io("inspect XDG_RUNTIME_DIR", error))?;
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(DiscoveryError::new(
            "XDG_RUNTIME_DIR is not owned by the current user",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn verify_windows_owner(path: &Path) -> Result<(), DiscoveryError> {
    use std::{os::windows::ffi::OsStrExt as _, ptr};
    use windows_sys::Win32::{
        Foundation::{ERROR_SUCCESS, LocalFree},
        Security::{
            Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
            OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        },
    };

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut owner_sid: PSID = ptr::null_mut();
    let mut security_descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `wide` is NUL-terminated and every output pointer remains valid
    // for the call. The returned descriptor is released on every later path.
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner_sid,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut security_descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(DiscoveryError::io(
            "read descriptor owner",
            std::io::Error::from_raw_os_error(status as i32),
        ));
    }
    let result = verify_owner_sid(owner_sid);
    // SAFETY: the pointer was returned by `GetNamedSecurityInfoW` above.
    unsafe {
        if !security_descriptor.is_null() {
            LocalFree(security_descriptor);
        }
    }
    result
}

/// Whether an object's owner SID is one this process itself creates objects
/// with. One predicate for both the owner check and the ACL check: an
/// elevated process creates objects owned by its token's default owner
/// (`BUILTIN\Administrators`), not by the user SID, and a check that accepts
/// that in one place and refuses it in the other leaves elevated Quantick
/// unable to publish at all.
#[cfg(windows)]
fn verify_owner_sid(owner_sid: windows_sys::Win32::Security::PSID) -> Result<(), DiscoveryError> {
    use windows_sys::Win32::Security::{EqualSid, IsValidSid};

    if owner_sid.is_null() || unsafe { IsValidSid(owner_sid) } == 0 {
        return Err(DiscoveryError::new(
            "descriptor owner SID is unavailable or invalid",
        ));
    }
    let current_user = current_windows_user_sid()?;
    // SAFETY: both SIDs were validated; `EqualSid` only reads them.
    let owned_by_user =
        unsafe { EqualSid(owner_sid, current_user.as_ptr().cast_mut().cast()) } != 0;
    // A token whose default owner cannot be read falls back to the user SID
    // alone.
    let owned_by_token_owner = current_windows_token_owner_sid().is_ok_and(|default_owner| {
        // SAFETY: as above.
        (unsafe { EqualSid(owner_sid, default_owner.as_ptr().cast_mut().cast()) }) != 0
    });
    if !owned_by_user && !owned_by_token_owner {
        return Err(DiscoveryError::new(
            "descriptor path is not owned by the current Windows user",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn set_private_windows_acl(path: &Path, is_directory: bool) -> Result<(), DiscoveryError> {
    use std::{os::windows::ffi::OsStrExt as _, ptr};
    use windows_sys::Win32::{
        Foundation::{ERROR_SUCCESS, LocalFree},
        Security::{
            Authorization::{
                EXPLICIT_ACCESS_W, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, SET_ACCESS,
                SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_USER,
                TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
            },
            CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, OBJECT_INHERIT_ACE,
            PROTECTED_DACL_SECURITY_INFORMATION, WinBuiltinAdministratorsSid, WinLocalSystemSid,
        },
        Storage::FileSystem::FILE_ALL_ACCESS,
    };

    fn entry(
        sid: &mut [u8],
        trustee_type: windows_sys::Win32::Security::Authorization::TRUSTEE_TYPE,
        inheritance: windows_sys::Win32::Security::ACE_FLAGS,
    ) -> EXPLICIT_ACCESS_W {
        EXPLICIT_ACCESS_W {
            grfAccessPermissions: FILE_ALL_ACCESS,
            grfAccessMode: SET_ACCESS,
            grfInheritance: inheritance,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: trustee_type,
                ptstrName: sid.as_mut_ptr().cast(),
            },
        }
    }

    verify_windows_owner(path)?;
    let mut current_user = current_windows_user_sid()?;
    let mut local_system = well_known_sid(WinLocalSystemSid)?;
    let mut administrators = well_known_sid(WinBuiltinAdministratorsSid)?;
    let inheritance = if is_directory {
        OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
    } else {
        0
    };
    let entries = [
        entry(&mut current_user, TRUSTEE_IS_USER, inheritance),
        entry(&mut local_system, TRUSTEE_IS_USER, inheritance),
        entry(
            &mut administrators,
            TRUSTEE_IS_WELL_KNOWN_GROUP,
            inheritance,
        ),
    ];
    let mut acl = ptr::null_mut();
    // SAFETY: each explicit entry points into a live, validated SID buffer;
    // the output ACL pointer is valid and is released with `LocalFree` below.
    let acl_status = unsafe {
        SetEntriesInAclW(
            u32::try_from(entries.len()).expect("the private ACL entry count fits u32"),
            entries.as_ptr(),
            ptr::null(),
            &mut acl,
        )
    };
    if acl_status != ERROR_SUCCESS || acl.is_null() {
        if !acl.is_null() {
            unsafe { LocalFree(acl.cast()) };
        }
        return Err(DiscoveryError::io(
            "build private descriptor ACL",
            std::io::Error::from_raw_os_error(acl_status as i32),
        ));
    }

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: `wide` is NUL-terminated and `acl` points to the live ACL built
    // above. Protecting the DACL removes inherited access entries.
    let set_status = unsafe {
        SetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            acl,
            ptr::null(),
        )
    };
    // SAFETY: `acl` was allocated by `SetEntriesInAclW`.
    unsafe { LocalFree(acl.cast()) };
    if set_status != ERROR_SUCCESS {
        return Err(DiscoveryError::io(
            "install private descriptor ACL",
            std::io::Error::from_raw_os_error(set_status as i32),
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn verify_windows_acl(path: &Path) -> Result<(), DiscoveryError> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt as _, ptr};
    use windows_sys::Win32::{
        Foundation::{ERROR_SUCCESS, GENERIC_ALL, GENERIC_READ, LocalFree},
        Security::{
            ACCESS_ALLOWED_ACE, ACL,
            Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
            DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetLengthSid, IsValidSid,
            OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, WinBuiltinAdministratorsSid,
            WinLocalSystemSid,
        },
        Storage::FileSystem::FILE_GENERIC_READ,
        System::SystemServices::ACCESS_ALLOWED_ACE_TYPE,
    };

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut owner_sid: windows_sys::Win32::Security::PSID = ptr::null_mut();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut security_descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `wide` is NUL-terminated and every output pointer is valid for
    // the duration of the call. Windows allocates `security_descriptor`, which
    // is released with `LocalFree` below on every post-call path.
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
            &mut owner_sid,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut security_descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(DiscoveryError::io(
            "read descriptor ACL",
            std::io::Error::from_raw_os_error(status as i32),
        ));
    }

    let result = (|| {
        // The same ownership rule the publisher applies, including the
        // elevated case: a check that refused the token owner here would
        // reject the directory this process had just created.
        verify_owner_sid(owner_sid)?;
        let current_user = current_windows_user_sid()?;
        if dacl.is_null() {
            return Err(DiscoveryError::new(
                "descriptor ACL grants implicit access to everyone",
            ));
        }
        let local_system = well_known_sid(WinLocalSystemSid)?;
        let administrators = well_known_sid(WinBuiltinAdministratorsSid)?;
        let mut current_user_can_read = false;
        // SAFETY: `dacl` belongs to the live security descriptor. `AceCount`
        // bounds every `GetAce` call. Only standard allow ACEs are accepted,
        // and their SID length is checked against the ACE boundary.
        unsafe {
            for index in 0..u32::from((*dacl).AceCount) {
                let mut raw_ace: *mut c_void = ptr::null_mut();
                if GetAce(dacl, index, &mut raw_ace) == 0 || raw_ace.is_null() {
                    return Err(DiscoveryError::new(
                        "descriptor ACL contains an unreadable ACE",
                    ));
                }
                let header = &*(raw_ace.cast::<windows_sys::Win32::Security::ACE_HEADER>());
                if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE {
                    return Err(DiscoveryError::new(
                        "descriptor ACL contains an unsupported ACE type",
                    ));
                }
                let sid_offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
                let ace_size = usize::from(header.AceSize);
                if ace_size < sid_offset + WINDOWS_MIN_SID_BYTES {
                    return Err(DiscoveryError::new(
                        "descriptor ACL contains a truncated allow ACE",
                    ));
                }
                let ace = &*raw_ace.cast::<ACCESS_ALLOWED_ACE>();
                let sid = ptr::addr_of!(ace.SidStart).cast_mut().cast::<c_void>();
                if IsValidSid(sid) == 0 || sid_offset + GetLengthSid(sid) as usize > ace_size {
                    return Err(DiscoveryError::new(
                        "descriptor ACL contains an invalid or truncated SID",
                    ));
                }
                if EqualSid(sid, current_user.as_ptr().cast_mut().cast()) != 0 {
                    let read_mask = FILE_GENERIC_READ | GENERIC_READ | GENERIC_ALL;
                    current_user_can_read |= ace.Mask & read_mask != 0;
                    continue;
                }
                if EqualSid(sid, local_system.as_ptr().cast_mut().cast()) != 0
                    || EqualSid(sid, administrators.as_ptr().cast_mut().cast()) != 0
                {
                    continue;
                }
                return Err(DiscoveryError::new(
                    "descriptor ACL grants access outside the current Windows user and trusted operating-system principals",
                ));
            }
        }
        if !current_user_can_read {
            return Err(DiscoveryError::new(
                "descriptor ACL does not grant read access to the current Windows user",
            ));
        }
        Ok(())
    })();
    // SAFETY: the pointer was returned by `GetNamedSecurityInfoW` above.
    unsafe {
        if !security_descriptor.is_null() {
            LocalFree(security_descriptor);
        }
    }
    result
}

#[cfg(windows)]
fn current_windows_user_sid() -> Result<Vec<u8>, DiscoveryError> {
    use std::{ffi::c_void, mem, ptr, slice};
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{
            GetLengthSid, GetTokenInformation, IsValidSid, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token = ptr::null_mut();
    // SAFETY: `GetCurrentProcess` returns a process pseudo-handle and `token`
    // is a valid output pointer. A successful token handle is closed below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(DiscoveryError::io(
            "open current Windows process token",
            std::io::Error::last_os_error(),
        ));
    }
    let result = (|| {
        let mut required = 0u32;
        // SAFETY: a null first buffer is the documented size query. The call
        // is expected to fail with `required` set to the necessary byte count.
        unsafe {
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut required);
        }
        if required < u32::try_from(mem::size_of::<TOKEN_USER>()).unwrap_or(u32::MAX) {
            return Err(DiscoveryError::new(
                "current Windows user token did not report a valid SID size",
            ));
        }
        let word_bytes = mem::size_of::<usize>();
        let words = (required as usize).div_ceil(word_bytes);
        let mut buffer = vec![0usize; words];
        let mut written = required;
        // SAFETY: the word buffer is aligned for `TOKEN_USER` and contains at
        // least `required` writable bytes. `written` is a valid output pointer.
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast::<c_void>(),
                required,
                &mut written,
            )
        } == 0
        {
            return Err(DiscoveryError::io(
                "read current Windows user SID",
                std::io::Error::last_os_error(),
            ));
        }
        // SAFETY: the successful call initialized a `TOKEN_USER` at the start
        // of the aligned buffer and keeps its referenced SID live in `buffer`.
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        if user.User.Sid.is_null() || unsafe { IsValidSid(user.User.Sid) } == 0 {
            return Err(DiscoveryError::new(
                "current Windows process token contains an invalid user SID",
            ));
        }
        // SAFETY: `IsValidSid` succeeded and `GetLengthSid` reports the exact
        // readable byte length of the SID stored in the live token buffer.
        let sid_length = unsafe { GetLengthSid(user.User.Sid) } as usize;
        let sid = unsafe { slice::from_raw_parts(user.User.Sid.cast::<u8>(), sid_length) };
        Ok(sid.to_vec())
    })();
    // SAFETY: `token` is a real handle returned by `OpenProcessToken`.
    unsafe {
        CloseHandle(token);
    }
    result
}

/// The SID this process's token assigns as owner to the objects it creates
/// (`TokenOwner`): the user for an ordinary process, `BUILTIN\Administrators`
/// for an elevated one.
#[cfg(windows)]
fn current_windows_token_owner_sid() -> Result<Vec<u8>, DiscoveryError> {
    use std::{ffi::c_void, mem, ptr, slice};
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{
            GetLengthSid, GetTokenInformation, IsValidSid, TOKEN_OWNER, TOKEN_QUERY, TokenOwner,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token = ptr::null_mut();
    // SAFETY: `GetCurrentProcess` returns a process pseudo-handle and `token`
    // is a valid output pointer. A successful token handle is closed below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(DiscoveryError::io(
            "open current Windows process token",
            std::io::Error::last_os_error(),
        ));
    }
    let result = (|| {
        let mut required = 0u32;
        // SAFETY: a null first buffer is the documented size query.
        unsafe {
            GetTokenInformation(token, TokenOwner, ptr::null_mut(), 0, &mut required);
        }
        if required < u32::try_from(mem::size_of::<TOKEN_OWNER>()).unwrap_or(u32::MAX) {
            return Err(DiscoveryError::new(
                "current Windows token did not report a valid owner SID size",
            ));
        }
        let word_bytes = mem::size_of::<usize>();
        let words = (required as usize).div_ceil(word_bytes);
        let mut buffer = vec![0usize; words];
        let mut written = required;
        // SAFETY: the word buffer is aligned for `TOKEN_OWNER` and contains
        // at least `required` writable bytes; `written` is a valid output.
        if unsafe {
            GetTokenInformation(
                token,
                TokenOwner,
                buffer.as_mut_ptr().cast::<c_void>(),
                required,
                &mut written,
            )
        } == 0
        {
            return Err(DiscoveryError::io(
                "read current Windows token owner SID",
                std::io::Error::last_os_error(),
            ));
        }
        // SAFETY: the successful call initialized a `TOKEN_OWNER` at the
        // start of the aligned buffer and keeps its SID live in `buffer`.
        let owner = unsafe { &*buffer.as_ptr().cast::<TOKEN_OWNER>() };
        if owner.Owner.is_null() || unsafe { IsValidSid(owner.Owner) } == 0 {
            return Err(DiscoveryError::new(
                "current Windows process token contains an invalid owner SID",
            ));
        }
        // SAFETY: `IsValidSid` succeeded and `GetLengthSid` reports the exact
        // readable byte length of the SID stored in the live token buffer.
        let sid_length = unsafe { GetLengthSid(owner.Owner) } as usize;
        let sid = unsafe { slice::from_raw_parts(owner.Owner.cast::<u8>(), sid_length) };
        Ok(sid.to_vec())
    })();
    // SAFETY: `token` is a real handle returned by `OpenProcessToken`.
    unsafe {
        CloseHandle(token);
    }
    result
}

#[cfg(windows)]
fn well_known_sid(kind: i32) -> Result<Vec<u8>, DiscoveryError> {
    use std::ptr;
    use windows_sys::Win32::Security::{CreateWellKnownSid, SECURITY_MAX_SID_SIZE};
    let mut bytes = vec![0u8; SECURITY_MAX_SID_SIZE as usize];
    let mut length = SECURITY_MAX_SID_SIZE;
    // SAFETY: the buffer has `length` writable bytes, and a null domain SID is
    // valid for the three absolute well-known identities used by the caller.
    if unsafe {
        CreateWellKnownSid(
            kind,
            ptr::null_mut(),
            bytes.as_mut_ptr().cast(),
            &mut length,
        )
    } == 0
    {
        return Err(DiscoveryError::io(
            "construct well-known Windows SID",
            std::io::Error::last_os_error(),
        ));
    }
    bytes.truncate(length as usize);
    Ok(bytes)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryError(String);

impl DiscoveryError {
    fn new(message: &'static str) -> Self {
        Self(message.to_owned())
    }

    fn owned(message: String) -> Self {
        Self(message)
    }

    fn io(action: &str, error: std::io::Error) -> Self {
        Self(format!("{action} failed: {error}"))
    }
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DiscoveryError {}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_control::{
        descriptor::{
            INSTANCE_DESCRIPTOR_HOST, INSTANCE_DESCRIPTOR_TRANSPORT, INSTANCE_DESCRIPTOR_VERSION,
        },
        handshake::{BearerToken, ProtocolVersionRange},
        id::{InstanceId, ProcessNonce},
    };

    fn descriptor(id: u8, published_at: i64) -> InstanceDescriptor {
        InstanceDescriptor {
            descriptor_version: INSTANCE_DESCRIPTOR_VERSION,
            instance_id: InstanceId::from_bytes([id; 16]),
            process_nonce: ProcessNonce::from_bytes([id.wrapping_add(1); 16]),
            process_id: u32::from(id) + 1,
            process_started_at_unix_ms: 1_700_000_000_000,
            application_version: "0.1.0".to_owned(),
            application_commit: "test".to_owned(),
            protocol_versions: ProtocolVersionRange::new(1, 1).unwrap(),
            transport: INSTANCE_DESCRIPTOR_TRANSPORT.to_owned(),
            host: INSTANCE_DESCRIPTOR_HOST.to_owned(),
            port: 10_000 + u16::from(id),
            bearer_token: BearerToken::from_bytes([id; 32]),
            published_at_unix_ms: published_at,
        }
    }

    fn scratch(name: &str) -> crate::scratch::ScratchDir {
        let dir = crate::scratch::ScratchDir::new(name);
        set_and_verify_private_directory(dir.path()).unwrap();
        dir
    }

    #[test]
    fn empty_discovery_explains_how_to_create_an_instance() {
        let directory = scratch("empty");
        let report = discover_descriptors_in(&directory).unwrap();
        assert!(report.candidates.is_empty());
        assert_eq!(report.next_steps, vec![NO_INSTANCE_NEXT_STEP]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn descriptors_are_private_strict_and_deterministically_sorted() {
        let directory = scratch("sorted");
        let later = descriptor(8, 20);
        let earlier_b = descriptor(7, 10);
        let earlier_a = descriptor(6, 10);
        let mut published = Vec::new();
        for item in [&later, &earlier_b, &earlier_a] {
            published.push(publish_descriptor_in(&directory, item).unwrap());
        }
        let report = discover_descriptors_in(&directory).unwrap();
        assert!(report.issues.is_empty());
        let ids = report
            .candidates
            .iter()
            .map(|candidate| candidate.descriptor.instance_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                earlier_a.instance_id,
                earlier_b.instance_id,
                later.instance_id
            ]
        );
        drop(published);
        assert!(
            discover_descriptors_in(&directory)
                .unwrap()
                .candidates
                .is_empty()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn malformed_and_oversized_files_are_issues_not_candidates() {
        let directory = scratch("invalid");
        let malformed = directory.join("malformed.json");
        let oversized = directory.join("oversized.json");
        fs::write(&malformed, b"{not-json").unwrap();
        fs::write(&oversized, vec![b'x'; CONTROL_DESCRIPTOR_MAX_BYTES + 1]).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&malformed, fs::Permissions::from_mode(0o600)).unwrap();
            fs::set_permissions(&oversized, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let report = discover_descriptors_in(&directory).unwrap();
        assert!(report.candidates.is_empty());
        assert_eq!(report.issues.len(), 2);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn discovery_directory_entry_count_is_bounded() {
        let directory = scratch("bounded");
        for index in 0..=CONTROL_DISCOVERY_MAX_ENTRIES {
            fs::write(directory.join(format!("noise-{index}")), b"noise").unwrap();
        }

        let report = discover_descriptors_in(&directory).unwrap();
        assert!(report.candidates.is_empty());
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.message.contains("only the first")),
            "the overflow is reported as an issue, not hidden and not fatal"
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
