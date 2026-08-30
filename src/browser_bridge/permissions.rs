use std::fs::File;
use std::path::Path;

use crate::core::CliError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PrivateObjectStatus {
    Private,
    Exposed,
}

pub(super) fn open_directory_without_following_symlink(
    path: &Path,
    description: &str,
) -> Result<File, CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        let mut options = std::fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW);
        return options.open(path).map_err(|error| {
            CliError::Config(format!(
                "could not open {description} {} without following symlinks: {error}",
                path.display()
            ))
        });
    }
    #[cfg(windows)]
    {
        let directory = windows::open_private_object(path, true, true).map_err(|error| {
            CliError::Config(format!(
                "could not open {description} {} without following reparse points: {error}",
                path.display()
            ))
        })?;
        verify_handle_type(&directory, path, UnixObjectKind::Directory)?;
        return Ok(directory);
    }
    #[allow(unreachable_code)]
    Err(CliError::Config(format!(
        "secure directory inspection is unsupported for {description} {} on this platform",
        path.display()
    )))
}

pub(super) fn open_locked_directory_without_following_symlink(
    path: &Path,
    description: &str,
) -> Result<File, CliError> {
    #[cfg(unix)]
    return open_directory_without_following_symlink(path, description);
    #[cfg(windows)]
    {
        let directory = windows::open_private_object(path, true, false).map_err(|error| {
            CliError::Config(format!(
                "could not lock {description} {} against replacement without following reparse points: {error}",
                path.display()
            ))
        })?;
        verify_handle_type(&directory, path, UnixObjectKind::Directory)?;
        return Ok(directory);
    }
    #[allow(unreachable_code)]
    Err(CliError::Config(format!(
        "secure locked-directory inspection is unsupported for {description} {} on this platform",
        path.display()
    )))
}

pub(super) fn verify_private_file_handle(
    file: &File,
    path: &Path,
) -> Result<PrivateObjectStatus, CliError> {
    #[cfg(unix)]
    return verify_unix_handle(
        file,
        path,
        UnixObjectKind::File,
        UnixModePolicy::Exact(0o600),
    );
    #[cfg(windows)]
    return verify_windows_handle(file, path, UnixObjectKind::File, false, "private file");
    #[allow(unreachable_code)]
    Ok(PrivateObjectStatus::Private)
}

#[allow(dead_code)]
pub(super) fn verify_private_directory_handle(
    directory: &File,
    path: &Path,
) -> Result<PrivateObjectStatus, CliError> {
    #[cfg(unix)]
    return verify_unix_handle(
        directory,
        path,
        UnixObjectKind::Directory,
        UnixModePolicy::Exact(0o700),
    );
    #[cfg(windows)]
    return verify_windows_handle(
        directory,
        path,
        UnixObjectKind::Directory,
        true,
        "private directory",
    );
    #[allow(unreachable_code)]
    Ok(PrivateObjectStatus::Private)
}

pub(super) fn verify_owned_nonwritable_directory_handle(
    directory: &File,
    path: &Path,
) -> Result<PrivateObjectStatus, CliError> {
    #[cfg(unix)]
    return verify_unix_handle(
        directory,
        path,
        UnixObjectKind::Directory,
        UnixModePolicy::OwnerOnlyWritable,
    );
    #[cfg(windows)]
    return verify_windows_handle(
        directory,
        path,
        UnixObjectKind::Directory,
        true,
        "configuration directory",
    );
    #[allow(unreachable_code)]
    Ok(PrivateObjectStatus::Private)
}

#[cfg(windows)]
fn verify_windows_handle(
    handle: &File,
    path: &Path,
    expected_kind: UnixObjectKind,
    directory: bool,
    description: &str,
) -> Result<PrivateObjectStatus, CliError> {
    verify_handle_type(handle, path, expected_kind)?;
    windows::verify_private_acl(handle, directory).map_err(|error| {
        CliError::Config(format!(
            "{description} {} failed its security check: {error}",
            path.display()
        ))
    })
}

#[derive(Clone, Copy)]
enum UnixObjectKind {
    File,
    Directory,
}

#[cfg(unix)]
#[derive(Clone, Copy)]
enum UnixModePolicy {
    Exact(u32),
    OwnerOnlyWritable,
}

#[cfg(unix)]
fn verify_unix_handle(
    handle: &File,
    path: &Path,
    expected_kind: UnixObjectKind,
    mode_policy: UnixModePolicy,
) -> Result<PrivateObjectStatus, CliError> {
    use std::os::unix::fs::MetadataExt;

    let metadata = verify_handle_type(handle, path, expected_kind)?;
    let effective_uid = unsafe { libc::geteuid() };
    if metadata.uid() != effective_uid {
        return Err(CliError::Config(format!(
            "private object {} is owned by uid {}, not the effective uid {effective_uid}",
            path.display(),
            metadata.uid()
        )));
    }

    let mode = metadata.mode() & 0o7777;
    let private = match mode_policy {
        UnixModePolicy::Exact(expected) => mode == expected,
        UnixModePolicy::OwnerOnlyWritable => mode & 0o022 == 0,
    };
    #[cfg(target_os = "macos")]
    let private = private
        && !macos_acl::has_extended_acl(handle).map_err(|error| {
            CliError::Config(format!(
                "could not inspect the extended ACL on {}: {error}",
                path.display()
            ))
        })?;
    Ok(if private {
        PrivateObjectStatus::Private
    } else {
        PrivateObjectStatus::Exposed
    })
}

fn verify_handle_type(
    handle: &File,
    path: &Path,
    expected_kind: UnixObjectKind,
) -> Result<std::fs::Metadata, CliError> {
    let metadata = handle.metadata().map_err(|error| {
        CliError::Config(format!(
            "could not inspect the opened private object {}: {error}",
            path.display()
        ))
    })?;
    #[cfg(windows)]
    let kind_matches = {
        use std::os::windows::fs::MetadataExt;

        windows_object_attributes_match_kind(metadata.file_attributes(), expected_kind).map_err(
            |reason| {
                CliError::Config(format!(
                    "private object {} failed its handle-type check: {reason}",
                    path.display()
                ))
            },
        )?
    };
    #[cfg(not(windows))]
    let kind_matches = match expected_kind {
        UnixObjectKind::File => metadata.is_file(),
        UnixObjectKind::Directory => metadata.is_dir(),
    };
    if !kind_matches {
        let expected = match expected_kind {
            UnixObjectKind::File => "regular file",
            UnixObjectKind::Directory => "directory",
        };
        return Err(CliError::Config(format!(
            "private object {} is not a {expected}",
            path.display()
        )));
    }
    Ok(metadata)
}

#[cfg(any(windows, test))]
fn windows_object_attributes_match_kind(
    attributes: u32,
    expected_kind: UnixObjectKind,
) -> Result<bool, &'static str> {
    // Keep these values local so the policy can be exercised on non-Windows
    // builders as well as against real handles on Windows CI.
    const FILE_ATTRIBUTE_DIRECTORY_VALUE: u32 = 0x0000_0010;
    const FILE_ATTRIBUTE_REPARSE_POINT_VALUE: u32 = 0x0000_0400;

    if attributes & FILE_ATTRIBUTE_REPARSE_POINT_VALUE != 0 {
        return Err("the opened object is a reparse point");
    }
    let is_directory = attributes & FILE_ATTRIBUTE_DIRECTORY_VALUE != 0;
    Ok(match expected_kind {
        UnixObjectKind::File => !is_directory,
        UnixObjectKind::Directory => is_directory,
    })
}

#[cfg(any(windows, test))]
const LOCAL_SYSTEM_SID: &str = "S-1-5-18";
#[cfg(any(windows, test))]
const BUILTIN_ADMINISTRATORS_SID: &str = "S-1-5-32-544";

#[cfg(any(windows, test))]
fn private_owner_is_allowed(actual_owner: &str, current_user: &str) -> bool {
    matches!(actual_owner, LOCAL_SYSTEM_SID | BUILTIN_ADMINISTRATORS_SID)
        || actual_owner == current_user
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy)]
enum OwnerValidationPhase {
    BeforeDaclUpdate,
    AfterDaclUpdate,
}

#[cfg(any(windows, test))]
fn ensure_private_owner_allowed(
    actual_owner: &str,
    current_user: &str,
    phase: OwnerValidationPhase,
) -> std::io::Result<()> {
    if private_owner_is_allowed(actual_owner, current_user) {
        return Ok(());
    }
    let phase = match phase {
        OwnerValidationPhase::BeforeDaclUpdate => "before updating its DACL",
        OwnerValidationPhase::AfterDaclUpdate => "after updating its DACL",
    };
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        format!(
            "refusing to trust a private object because its owner {actual_owner} is not the current user, LocalSystem, or Builtin Administrators {phase}"
        ),
    ))
}

pub(super) fn harden_private_directory(path: &Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        #[cfg(target_os = "macos")]
        macos_acl::clear_extended_acl(&File::open(path)?).map_err(|error| {
            CliError::Config(format!(
                "could not clear the extended ACL on private directory {}: {error}",
                path.display()
            ))
        })?;
    }
    #[cfg(windows)]
    {
        let directory = windows::open_private_object(path, true, true).map_err(|error| {
            CliError::Config(format!(
                "could not open private directory {} for protection: {error}",
                path.display()
            ))
        })?;
        windows::apply_private_dacl(&directory, true).map_err(|error| {
            CliError::Config(format!(
                "could not protect private directory {}: {error}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

pub(super) fn harden_private_file(path: &Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        #[cfg(target_os = "macos")]
        macos_acl::clear_extended_acl(&File::open(path)?).map_err(|error| {
            CliError::Config(format!(
                "could not clear the extended ACL on private file {}: {error}",
                path.display()
            ))
        })?;
    }
    #[cfg(windows)]
    {
        let file = windows::open_private_object(path, false, true).map_err(|error| {
            CliError::Config(format!(
                "could not open private file {} for protection: {error}",
                path.display()
            ))
        })?;
        windows::apply_private_dacl(&file, false).map_err(|error| {
            CliError::Config(format!(
                "could not protect private file {}: {error}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

pub(super) fn harden_private_file_handle(file: &File, _path: &Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        #[cfg(target_os = "macos")]
        macos_acl::clear_extended_acl(file).map_err(|error| {
            CliError::Config(format!(
                "could not clear the extended ACL on private file {}: {error}",
                _path.display()
            ))
        })?;
    }
    #[cfg(windows)]
    {
        let security_handle = windows::reopen_private_object(file, false).map_err(|error| {
            CliError::Config(format!(
                "could not reopen private file {} for protection: {error}",
                _path.display()
            ))
        })?;
        windows::apply_private_dacl(&security_handle, false).map_err(|error| {
            CliError::Config(format!(
                "could not protect private file {}: {error}",
                _path.display()
            ))
        })?;
    }
    Ok(())
}

pub(super) fn harden_private_directory_handle(
    directory: &File,
    path: &Path,
) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        directory
            .set_permissions(std::fs::Permissions::from_mode(0o700))
            .map_err(|error| {
                CliError::Config(format!(
                    "could not protect private directory {} through its opened handle: {error}",
                    path.display()
                ))
            })?;
        #[cfg(target_os = "macos")]
        macos_acl::clear_extended_acl(directory).map_err(|error| {
            CliError::Config(format!(
                "could not clear the extended ACL on private directory {}: {error}",
                path.display()
            ))
        })?;
    }
    #[cfg(windows)]
    {
        windows::apply_private_dacl(directory, true).map_err(|error| {
            CliError::Config(format!(
                "could not protect private directory {} through its opened handle: {error}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
mod macos_acl {
    use std::ffi::c_void;
    use std::fs::File;
    use std::io;
    use std::os::fd::AsRawFd;

    const ACL_TYPE_EXTENDED: libc::c_int = 0x0000_0100;

    unsafe extern "C" {
        fn acl_free(object: *mut c_void) -> libc::c_int;
        fn acl_get_fd_np(fd: libc::c_int, acl_type: libc::c_int) -> *mut c_void;
        fn acl_init(count: libc::c_int) -> *mut c_void;
        fn acl_set_fd_np(fd: libc::c_int, acl: *mut c_void, acl_type: libc::c_int) -> libc::c_int;
    }

    struct OwnedAcl(*mut c_void);

    impl Drop for OwnedAcl {
        fn drop(&mut self) {
            unsafe {
                acl_free(self.0);
            }
        }
    }

    pub(super) fn has_extended_acl(file: &File) -> io::Result<bool> {
        let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) };
        if acl.is_null() {
            let error = io::Error::last_os_error();
            // Darwin reports ENOENT when the object has no extended ACL.
            // That is the desired private state, not a missing file: the file
            // descriptor itself was already validated immediately above.
            return if error.raw_os_error() == Some(libc::ENOENT) {
                Ok(false)
            } else {
                Err(error)
            };
        }
        // Darwin returns ENOENT above when no extended ACL exists. A non-null
        // ACL therefore represents at least one extended entry.
        let _acl = OwnedAcl(acl);
        Ok(true)
    }

    pub(super) fn clear_extended_acl(file: &File) -> io::Result<()> {
        let acl = unsafe { acl_init(1) };
        if acl.is_null() {
            return Err(io::Error::last_os_error());
        }
        let acl = OwnedAcl(acl);
        if unsafe { acl_set_fd_np(file.as_raw_fd(), acl.0, ACL_TYPE_EXTENDED) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(all(test, windows))]
pub(super) fn assert_private_acl(path: &Path, directory: bool) {
    windows::assert_private_acl(path, directory);
}

#[cfg(all(test, windows))]
pub(super) fn make_world_readable_for_test(path: &Path, directory: bool) {
    windows::make_world_readable_for_test(path, directory);
}

#[cfg(windows)]
mod windows {
    use super::{
        BUILTIN_ADMINISTRATORS_SID, LOCAL_SYSTEM_SID, OwnerValidationPhase,
        ensure_private_owner_allowed, private_owner_is_allowed,
    };
    use std::collections::BTreeSet;
    use std::ffi::c_void;
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::iter;
    use std::mem::{size_of, size_of_val};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::path::Path;
    use std::ptr::{addr_of, null, null_mut};

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HANDLE, INVALID_HANDLE_VALUE,
        LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SetSecurityInfo,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, GetAce, GetAclInformation,
        GetSecurityDescriptorControl, GetSecurityDescriptorDacl, INHERITED_ACE, OBJECT_INHERIT_ACE,
        OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSID, SE_DACL_PROTECTED,
        TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ALL_ACCESS, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, READ_CONTROL,
        ReOpenFile, WRITE_DAC,
    };
    use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    struct LocalAllocation(*mut c_void);

    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    LocalFree(self.0);
                }
            }
        }
    }

    pub(super) fn open_private_object(
        path: &Path,
        directory: bool,
        share_delete: bool,
    ) -> io::Result<File> {
        let share_mode =
            FILE_SHARE_READ | FILE_SHARE_WRITE | if share_delete { FILE_SHARE_DELETE } else { 0 };
        let mut options = OpenOptions::new();
        options
            .read(true)
            .access_mode(FILE_READ_ATTRIBUTES | READ_CONTROL | WRITE_DAC)
            .share_mode(share_mode)
            .custom_flags(
                FILE_FLAG_OPEN_REPARSE_POINT
                    | if directory {
                        FILE_FLAG_BACKUP_SEMANTICS
                    } else {
                        0
                    },
            );
        options.open(path)
    }

    pub(super) fn reopen_private_object(file: &File, directory: bool) -> io::Result<File> {
        let handle = unsafe {
            ReOpenFile(
                file.as_raw_handle(),
                READ_CONTROL | WRITE_DAC,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_handle(handle) })
    }

    pub(super) fn apply_private_dacl(file: &File, directory: bool) -> io::Result<()> {
        apply_dacl(file, directory, false)
    }

    fn apply_dacl(file: &File, directory: bool, world_readable: bool) -> io::Result<()> {
        let user_sid = current_user_sid_string()?;
        verify_allowed_owner(file, &user_sid, OwnerValidationPhase::BeforeDaclUpdate)?;
        let inheritance = if directory { "OICI" } else { "" };
        let trustees = BTreeSet::from([
            user_sid.clone(),
            LOCAL_SYSTEM_SID.to_string(),
            BUILTIN_ADMINISTRATORS_SID.to_string(),
        ]);
        let trustee_aces = trustees
            .iter()
            .map(|trustee| format!("(A;{inheritance};FA;;;{trustee})"))
            .collect::<String>();
        let world_ace = if world_readable {
            format!("(A;{inheritance};GR;;;WD)")
        } else {
            String::new()
        };
        let descriptor = format!("D:P{trustee_aces}{world_ace}");
        let descriptor = wide_string(&descriptor);
        let mut security_descriptor = null_mut();
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                descriptor.as_ptr(),
                SDDL_REVISION_1,
                &mut security_descriptor,
                null_mut(),
            )
        };
        if converted == 0 {
            return Err(io::Error::last_os_error());
        }
        if security_descriptor.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned an empty security descriptor",
            ));
        }
        let _security_descriptor = LocalAllocation(security_descriptor);

        let mut dacl_present = 0;
        let mut dacl_defaulted = 0;
        let mut dacl = null_mut();
        let found_dacl = unsafe {
            GetSecurityDescriptorDacl(
                security_descriptor,
                &mut dacl_present,
                &mut dacl,
                &mut dacl_defaulted,
            )
        };
        if found_dacl == 0 {
            return Err(io::Error::last_os_error());
        }
        if dacl_present == 0 || dacl.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "generated security descriptor has no DACL",
            ));
        }

        let status = unsafe {
            SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                null(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        verify_allowed_owner(file, &user_sid, OwnerValidationPhase::AfterDaclUpdate)
    }

    fn verify_allowed_owner(
        file: &File,
        current_user: &str,
        phase: OwnerValidationPhase,
    ) -> io::Result<()> {
        let mut owner = null_mut();
        let mut security_descriptor = null_mut();
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                null_mut(),
                null_mut(),
                &mut security_descriptor,
            )
        };
        let _security_descriptor = LocalAllocation(security_descriptor);
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        if security_descriptor.is_null() || owner.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned an empty object owner",
            ));
        }
        let actual_owner = sid_to_string(owner)?;
        ensure_private_owner_allowed(&actual_owner, current_user, phase)
    }

    pub(super) fn verify_private_acl(
        file: &File,
        directory: bool,
    ) -> io::Result<super::PrivateObjectStatus> {
        let user_sid = current_user_sid_string()?;
        let mut owner = null_mut();
        let mut dacl: *mut ACL = null_mut();
        let mut security_descriptor = null_mut();
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut security_descriptor,
            )
        };
        let _security_descriptor = LocalAllocation(security_descriptor);
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        if security_descriptor.is_null() || owner.is_null() {
            return Err(invalid_private_acl(
                "Windows returned an empty security descriptor or owner",
            ));
        }
        if dacl.is_null() {
            return Ok(super::PrivateObjectStatus::Exposed);
        }

        let owner_sid = sid_to_string(owner)?;
        if !private_owner_is_allowed(&owner_sid, &user_sid) {
            return Err(permission_denied_private_acl(format!(
                "owner {owner_sid} is not the current user, LocalSystem, or Builtin Administrators"
            )));
        }

        let mut control = 0;
        let mut revision = 0;
        let found_control = unsafe {
            GetSecurityDescriptorControl(security_descriptor, &mut control, &mut revision)
        };
        if found_control == 0 {
            return Err(io::Error::last_os_error());
        }
        if control & SE_DACL_PROTECTED == 0 {
            return Ok(super::PrivateObjectStatus::Exposed);
        }

        let mut acl_size = ACL_SIZE_INFORMATION::default();
        let found_acl = unsafe {
            GetAclInformation(
                dacl,
                (&mut acl_size as *mut ACL_SIZE_INFORMATION).cast(),
                size_of_val(&acl_size) as u32,
                AclSizeInformation,
            )
        };
        if found_acl == 0 {
            return Err(io::Error::last_os_error());
        }

        let expected = BTreeSet::from([
            user_sid,
            LOCAL_SYSTEM_SID.to_string(),
            BUILTIN_ADMINISTRATORS_SID.to_string(),
        ]);
        if acl_size.AceCount as usize != expected.len() {
            return Ok(super::PrivateObjectStatus::Exposed);
        }

        let expected_flags = if directory {
            (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) as u8
        } else {
            0
        };
        let mut trustees = BTreeSet::new();
        for index in 0..acl_size.AceCount {
            let mut raw_ace = null_mut();
            let found_ace = unsafe { GetAce(dacl, index, &mut raw_ace) };
            if found_ace == 0 {
                return Err(io::Error::last_os_error());
            }
            if raw_ace.is_null() {
                return Ok(super::PrivateObjectStatus::Exposed);
            }
            let header = unsafe { &*(raw_ace.cast::<ACE_HEADER>()) };
            if header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8 {
                return Ok(super::PrivateObjectStatus::Exposed);
            }
            if (header.AceSize as usize) < size_of::<ACCESS_ALLOWED_ACE>() {
                return Ok(super::PrivateObjectStatus::Exposed);
            }
            if header.AceFlags & INHERITED_ACE as u8 != 0 {
                return Ok(super::PrivateObjectStatus::Exposed);
            }
            if header.AceFlags != expected_flags {
                return Ok(super::PrivateObjectStatus::Exposed);
            }

            let ace = unsafe { &*(raw_ace.cast::<ACCESS_ALLOWED_ACE>()) };
            if ace.Mask != FILE_ALL_ACCESS {
                return Ok(super::PrivateObjectStatus::Exposed);
            }
            let trustee = match sid_to_string(addr_of!(ace.SidStart) as PSID) {
                Ok(trustee) => trustee,
                Err(_) => return Ok(super::PrivateObjectStatus::Exposed),
            };
            if trustee == "S-1-1-0" {
                return Ok(super::PrivateObjectStatus::Exposed);
            }
            if !expected.contains(&trustee) {
                return Ok(super::PrivateObjectStatus::Exposed);
            }
            if !trustees.insert(trustee) {
                return Ok(super::PrivateObjectStatus::Exposed);
            }
        }

        if trustees != expected {
            return Ok(super::PrivateObjectStatus::Exposed);
        }
        Ok(super::PrivateObjectStatus::Private)
    }

    fn invalid_private_acl(message: impl Into<String>) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message.into())
    }

    fn permission_denied_private_acl(message: impl Into<String>) -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, message.into())
    }

    fn current_user_sid_string() -> io::Result<String> {
        let mut token = null_mut();
        let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
        if opened == 0 {
            return Err(io::Error::last_os_error());
        }
        let _token = Handle(token);

        let mut required_bytes = 0;
        let queried = unsafe {
            windows_sys::Win32::Security::GetTokenInformation(
                token,
                TokenUser,
                null_mut(),
                0,
                &mut required_bytes,
            )
        };
        if queried != 0 || required_bytes == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned an invalid token-user size",
            ));
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
            return Err(error);
        }
        if (required_bytes as usize) < size_of::<TOKEN_USER>() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned a truncated token-user record",
            ));
        }

        // TOKEN_USER contains a pointer, so use pointer-sized storage rather
        // than a byte vector to guarantee suitable alignment.
        let word_count = (required_bytes as usize).div_ceil(size_of::<usize>());
        let mut token_user = vec![0usize; word_count];
        let queried = unsafe {
            windows_sys::Win32::Security::GetTokenInformation(
                token,
                TokenUser,
                token_user.as_mut_ptr().cast(),
                required_bytes,
                &mut required_bytes,
            )
        };
        if queried == 0 {
            return Err(io::Error::last_os_error());
        }
        let token_user = unsafe { &*(token_user.as_ptr().cast::<TOKEN_USER>()) };
        if token_user.User.Sid.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows token has no user SID",
            ));
        }
        sid_to_string(token_user.User.Sid)
    }

    fn sid_to_string(sid: PSID) -> io::Result<String> {
        let mut string_sid = null_mut();
        let converted = unsafe { ConvertSidToStringSidW(sid, &mut string_sid) };
        if converted == 0 {
            return Err(io::Error::last_os_error());
        }
        if string_sid.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned an empty SID string",
            ));
        }
        let _string_sid = LocalAllocation(string_sid.cast());
        let mut length = 0;
        unsafe {
            while *string_sid.add(length) != 0 {
                length += 1;
            }
            String::from_utf16(std::slice::from_raw_parts(string_sid, length))
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        }
    }

    fn wide_string(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(iter::once(0)).collect()
    }

    #[cfg(test)]
    pub(super) fn assert_private_acl(path: &Path, directory: bool) {
        let file =
            open_private_object(path, directory, true).expect("open private ACL test fixture");
        assert_eq!(
            verify_private_acl(&file, directory).expect("inspect private ACL"),
            super::PrivateObjectStatus::Private,
            "private ACL must match the runtime policy"
        );
    }

    #[cfg(test)]
    pub(super) fn make_world_readable_for_test(path: &Path, directory: bool) {
        let file =
            open_private_object(path, directory, true).expect("open fixture for ACL mutation");
        apply_dacl(&file, directory, true).expect("add explicit Everyone read ACE");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BUILTIN_ADMINISTRATORS_SID, LOCAL_SYSTEM_SID, OwnerValidationPhase,
        ensure_private_owner_allowed, private_owner_is_allowed,
    };

    const CURRENT_USER_SID: &str = "S-1-5-21-1000-1000-1000-1001";

    #[test]
    fn private_owner_allowlist_matches_every_full_control_trustee() {
        assert!(private_owner_is_allowed(CURRENT_USER_SID, CURRENT_USER_SID));
        assert!(private_owner_is_allowed(LOCAL_SYSTEM_SID, CURRENT_USER_SID));
        assert!(private_owner_is_allowed(
            BUILTIN_ADMINISTRATORS_SID,
            CURRENT_USER_SID
        ));
    }

    #[test]
    fn private_owner_allowlist_rejects_unrelated_trustees() {
        assert!(!private_owner_is_allowed(
            "S-1-5-21-2000-2000-2000-2002",
            CURRENT_USER_SID
        ));
        assert!(!private_owner_is_allowed("S-1-1-0", CURRENT_USER_SID));
    }

    #[test]
    fn pre_and_post_update_owner_validation_reject_an_unrelated_owner() {
        let before_error = ensure_private_owner_allowed(
            "S-1-5-21-2000-2000-2000-2002",
            CURRENT_USER_SID,
            OwnerValidationPhase::BeforeDaclUpdate,
        )
        .expect_err("unrelated pre-update owner must fail closed");
        assert_eq!(before_error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(
            before_error
                .to_string()
                .contains("before updating its DACL")
        );

        let after_error = ensure_private_owner_allowed(
            "S-1-5-21-2000-2000-2000-2002",
            CURRENT_USER_SID,
            OwnerValidationPhase::AfterDaclUpdate,
        )
        .expect_err("unrelated post-update owner must fail closed");
        assert_eq!(after_error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(after_error.to_string().contains("after updating its DACL"));
    }

    #[test]
    fn windows_attribute_policy_rejects_every_file_or_directory_reparse_point() {
        const DIRECTORY: u32 = 0x0000_0010;
        const REPARSE_POINT: u32 = 0x0000_0400;

        assert_eq!(
            super::windows_object_attributes_match_kind(0, super::UnixObjectKind::File),
            Ok(true)
        );
        assert_eq!(
            super::windows_object_attributes_match_kind(
                DIRECTORY,
                super::UnixObjectKind::Directory
            ),
            Ok(true)
        );
        assert_eq!(
            super::windows_object_attributes_match_kind(DIRECTORY, super::UnixObjectKind::File),
            Ok(false)
        );
        assert_eq!(
            super::windows_object_attributes_match_kind(0, super::UnixObjectKind::Directory),
            Ok(false)
        );
        assert_eq!(
            super::windows_object_attributes_match_kind(REPARSE_POINT, super::UnixObjectKind::File),
            Err("the opened object is a reparse point")
        );
        assert_eq!(
            super::windows_object_attributes_match_kind(
                DIRECTORY | REPARSE_POINT,
                super::UnixObjectKind::Directory
            ),
            Err("the opened object is a reparse point"),
            "directory junctions, mount points, and symlinks must all fail before child paths are joined"
        );
    }

    #[cfg(windows)]
    #[test]
    fn secure_openers_reject_real_file_and_directory_reparse_points() {
        use std::io::ErrorKind;
        use std::os::windows::fs::{symlink_dir, symlink_file};

        let root = tempfile::tempdir().expect("temporary directory");
        let file_target = root.path().join("file-target");
        let file_link = root.path().join("file-link");
        let directory_target = root.path().join("directory-target");
        let directory_link = root.path().join("directory-link");
        std::fs::write(&file_target, "fixture").expect("write file target");
        std::fs::create_dir(&directory_target).expect("create directory target");

        if let Err(error) = symlink_file(&file_target, &file_link) {
            if error.kind() == ErrorKind::PermissionDenied {
                return;
            }
            panic!("create file reparse point: {error}");
        }
        symlink_dir(&directory_target, &directory_link).expect("create directory reparse point");

        let file = super::windows::open_private_object(&file_link, false, true)
            .expect("open file reparse point itself");
        let error = super::verify_private_file_handle(&file, &file_link)
            .expect_err("file reparse point must fail closed");
        assert!(error.to_string().contains("reparse point"));

        let error =
            super::open_directory_without_following_symlink(&directory_link, "fixture directory")
                .expect_err("directory reparse point must fail closed");
        assert!(error.to_string().contains("reparse point"));

        let error = super::open_locked_directory_without_following_symlink(
            &directory_link,
            "fixture locked directory",
        )
        .expect_err("locked directory reparse point must fail closed");
        assert!(error.to_string().contains("reparse point"));
    }

    #[cfg(unix)]
    #[test]
    fn private_file_verification_uses_the_open_handle_and_rejects_mode_drift() {
        use std::fs::File;
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("secret");
        std::fs::write(&path, b"secret").expect("write fixture");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("set private mode");
        let opened = File::open(&path).expect("open private file");

        assert_eq!(
            super::verify_private_file_handle(&opened, &path).expect("inspect 0600 file"),
            super::PrivateObjectStatus::Private
        );

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640))
            .expect("drift fixture mode");
        assert_eq!(
            super::verify_private_file_handle(&opened, &path)
                .expect("mode drift remains repairable"),
            super::PrivateObjectStatus::Exposed,
            "the already-opened handle must observe mode drift"
        );
    }

    #[cfg(unix)]
    #[test]
    fn private_directory_verification_uses_the_open_handle_and_rejects_special_bits() {
        use std::fs::File;
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temporary directory");
        let path = root.path().join("bundle");
        std::fs::create_dir(&path).expect("create fixture directory");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("set private mode");
        let opened = File::open(&path).expect("open private directory");

        assert_eq!(
            super::verify_private_directory_handle(&opened, &path).expect("inspect 0700 directory"),
            super::PrivateObjectStatus::Private
        );

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o1700))
            .expect("add a special mode bit");
        assert_eq!(
            super::verify_private_directory_handle(&opened, &path)
                .expect("special-bit drift remains repairable"),
            super::PrivateObjectStatus::Exposed,
            "special bits must fail the exact private-directory policy"
        );

        super::harden_private_directory_handle(&opened, &path)
            .expect("repair directory through its opened handle");
        assert_eq!(
            super::verify_private_directory_handle(&opened, &path)
                .expect("inspect repaired directory"),
            super::PrivateObjectStatus::Private
        );
    }

    #[cfg(unix)]
    #[test]
    fn owned_nonwritable_directory_accepts_readable_modes_but_rejects_writers() {
        use std::fs::File;
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temporary directory");
        let path = root.path().join("config");
        std::fs::create_dir(&path).expect("create fixture directory");
        let opened = File::open(&path).expect("open configuration directory");

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("set conventional configuration-directory mode");
        assert_eq!(
            super::verify_owned_nonwritable_directory_handle(&opened, &path)
                .expect("inspect 0755 directory"),
            super::PrivateObjectStatus::Private
        );

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o775))
            .expect("make fixture group-writable");
        assert_eq!(
            super::verify_owned_nonwritable_directory_handle(&opened, &path)
                .expect("group-write drift remains repairable"),
            super::PrivateObjectStatus::Exposed,
            "group-writable configuration directory must fail"
        );
    }

    #[cfg(unix)]
    #[test]
    fn handle_verification_rejects_the_wrong_object_type() {
        use std::fs::File;
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temporary directory");
        let directory_path = root.path().join("directory");
        std::fs::create_dir(&directory_path).expect("create fixture directory");
        std::fs::set_permissions(&directory_path, std::fs::Permissions::from_mode(0o700))
            .expect("set directory mode");
        let directory = File::open(&directory_path).expect("open fixture directory");
        let error = super::verify_private_file_handle(&directory, &directory_path)
            .expect_err("directory handle must not pass file verification");
        assert!(error.to_string().contains("is not a regular file"));
    }

    #[cfg(unix)]
    #[test]
    fn secure_directory_opener_rejects_a_symlink() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary directory");
        let target = root.path().join("target");
        let link = root.path().join("link");
        std::fs::create_dir(&target).expect("create target directory");
        symlink(&target, &link).expect("create directory symlink");

        super::open_directory_without_following_symlink(&target, "fixture directory")
            .expect("regular directory should open");
        let error = super::open_directory_without_following_symlink(&link, "fixture directory")
            .expect_err("directory symlink must not be followed");
        assert!(error.to_string().contains("without following symlinks"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_extended_acl_is_exposed_even_when_mode_is_0600() {
        use std::fs::File;
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("secret");
        std::fs::write(&path, b"secret").expect("write fixture");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("set private mode");
        let opened = File::open(&path).expect("open fixture");
        assert_eq!(
            super::verify_private_file_handle(&opened, &path).expect("inspect private file"),
            super::PrivateObjectStatus::Private
        );

        let status = Command::new("/bin/chmod")
            .args(["+a", "everyone allow read"])
            .arg(&path)
            .status()
            .expect("run chmod +a");
        assert!(status.success(), "chmod +a fixture setup failed");
        assert_eq!(
            std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600,
            "extended ACL fixture must retain its nominal mode"
        );
        assert_eq!(
            super::verify_private_file_handle(&opened, &path)
                .expect("extended ACL remains a repairable exposure"),
            super::PrivateObjectStatus::Exposed
        );

        super::harden_private_file_handle(&opened, &path).expect("clear ACL through opened handle");
        assert_eq!(
            super::verify_private_file_handle(&opened, &path).expect("inspect repaired file"),
            super::PrivateObjectStatus::Private
        );
    }

    #[cfg(windows)]
    #[test]
    fn private_acl_verification_rejects_an_explicit_everyone_ace() {
        use std::fs::File;

        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("secret");
        File::create(&path).expect("create fixture file");
        super::harden_private_file(&path).expect("harden fixture");

        let opened =
            super::windows::open_private_object(&path, false, true).expect("open private fixture");
        assert_eq!(
            super::verify_private_file_handle(&opened, &path).expect("inspect private ACL"),
            super::PrivateObjectStatus::Private
        );

        super::windows::make_world_readable_for_test(&path, false);
        assert_eq!(
            super::verify_private_file_handle(&opened, &path)
                .expect("Everyone ACE remains a repairable exposure"),
            super::PrivateObjectStatus::Exposed,
            "Everyone ACE must fail verification"
        );
    }
}
