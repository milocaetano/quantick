//! Actual Windows filesystem authority fixtures, with an independent .NET oracle.
//!
//! Windows PowerShell inherits this process's token. Its WindowsIdentity and
//! ObjectSecurity APIs inspect owners and DACLs without calling our verifier.
//! No descriptor contents or bearer tokens cross that process boundary.

use super::*;
use crate::scratch::ScratchDir;
use serde_json::Value;
use std::{os::windows::process::CommandExt as _, process::Command};

const UNTRUSTED: &str = "descriptor ACL grants access outside the current Windows user and trusted operating-system principals";
const FOREIGN: &str = "descriptor path is not owned by the current Windows user";
const REDIRECT: &str = "descriptor path redirects through a symlink or reparse point";

// Persist only access rules. PowerShell's Set-Acl provider also attempts
// unrelated security sections that can require SeSecurityPrivilege.
const WRITE_DACL: &str = r#"
if ([IO.Directory]::Exists($env:Q4_PATH)) {
    [IO.Directory]::SetAccessControl($env:Q4_PATH, $acl)
} else {
    [IO.File]::SetAccessControl($env:Q4_PATH, $acl)
}
$true | ConvertTo-Json
"#;

const INSPECT: &str = r#"
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$acl = Get-Acl -LiteralPath $env:Q4_PATH
$item = Get-Item -LiteralPath $env:Q4_PATH -Force
$rules = @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]) | ForEach-Object {
    @{ sid = $_.IdentityReference.Value; kind = $_.AccessControlType.ToString(); rights = [long]$_.FileSystemRights }
})
@{
    owner = $acl.GetOwner([Security.Principal.SecurityIdentifier]).Value
    user = $identity.User.Value
    token_owner = $identity.Owner.Value
    protected = $acl.AreAccessRulesProtected
    sddl = $acl.GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access)
    rules = $rules
    directory = $item.PSIsContainer
    reparse = ([int]$item.Attributes -band 0x400) -ne 0
    link_type = $item.LinkType
    target = @($item.Target)
    filesystem = [IO.DriveInfo]::new([IO.Path]::GetPathRoot($env:Q4_PATH)).DriveFormat
} | ConvertTo-Json -Depth 4 -Compress
"#;

fn powershell(script: &str, path: &Path, extra: Option<(&str, &std::ffi::OsStr)>) -> Value {
    // Use the inbox executable, not PATH; neither paths nor SDDL become code.
    let executable = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .expect("Windows fixture prerequisite: SystemRoot is set")
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut command = Command::new(executable);
    command
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!(
            "$ErrorActionPreference = 'Stop'; [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); {script}"
        ))
        .env("Q4_PATH", path)
        // A PS7-launched test process can inherit incompatible PS7 modules.
        // Let inbox Windows PowerShell rebuild its own default module paths.
        .env_remove("PSModulePath")
        .creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    if let Some((name, value)) = extra {
        command.env(name, value);
    }
    let output = command
        .output()
        .expect("Windows fixture prerequisite: inbox PowerShell can execute");
    assert!(
        output.status.success(),
        "Windows fixture prerequisite failed for {path:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("Windows fixture oracle emits token-free JSON")
}

fn inspect(path: &Path) -> Value {
    powershell(INSPECT, path, None)
}

fn assert_owned(acl: &Value) {
    assert!(
        acl["owner"] == acl["user"] || acl["owner"] == acl["token_owner"],
        "Windows fixture prerequisite: scratch owner matches TokenUser or TokenOwner"
    );
}

fn assert_private(path: &Path) -> Value {
    let acl = inspect(path);
    assert_owned(&acl);
    assert_eq!(acl["protected"], true, "published DACL is protected");
    assert_eq!(acl["reparse"], false);
    let rules = acl["rules"].as_array().expect("oracle returns ACE array");
    assert!(!rules.is_empty(), "private DACL has explicit allow entries");
    let user = acl["user"].as_str().unwrap();
    for rule in rules {
        assert_eq!(rule["kind"], "Allow");
        assert!(
            [user, "S-1-5-18", "S-1-5-32-544"].contains(&rule["sid"].as_str().unwrap()),
            "private DACL contains only user, LocalSystem and Administrators"
        );
    }
    assert!(
        rules
            .iter()
            .any(|rule| { rule["sid"] == user && rule["rights"].as_u64().unwrap() & 1 != 0 })
    );
    acl
}

fn descriptor(id: u8) -> InstanceDescriptor {
    use quantick_control::{descriptor::*, handshake::*, id::*};
    InstanceDescriptor {
        descriptor_version: INSTANCE_DESCRIPTOR_VERSION,
        instance_id: InstanceId::from_bytes([id; 16]),
        process_nonce: ProcessNonce::from_bytes([id + 1; 16]),
        process_id: u32::from(id) + 1,
        process_started_at_unix_ms: 1_700_000_000_000,
        application_version: "0.1.0".to_owned(),
        application_commit: "windows-authority-test".to_owned(),
        protocol_versions: ProtocolVersionRange::new(1, 1).unwrap(),
        transport: INSTANCE_DESCRIPTOR_TRANSPORT.to_owned(),
        host: INSTANCE_DESCRIPTOR_HOST.to_owned(),
        port: 10_000 + u16::from(id),
        bearer_token: BearerToken::from_bytes([id; 32]),
        published_at_unix_ms: 1_700_000_000_001,
    }
}

struct RestoreAcl {
    path: PathBuf,
    sddl: String,
}

impl RestoreAcl {
    fn grant_everyone_read(path: &Path) -> Self {
        let original = assert_private(path);
        // Arm restoration before mutating, including when the setup assertion panics.
        let restore = Self {
            path: path.to_path_buf(),
            sddl: original["sddl"].as_str().unwrap().to_owned(),
        };
        let mutation = format!(
            r#"
$sections = [Security.AccessControl.AccessControlSections]::Access
if ([IO.Directory]::Exists($env:Q4_PATH)) {{
    $acl = [IO.Directory]::GetAccessControl($env:Q4_PATH, $sections)
}} else {{
    $acl = [IO.File]::GetAccessControl($env:Q4_PATH, $sections)
}}
$sid = [Security.Principal.SecurityIdentifier]::new('S-1-1-0')
$rule = [Security.AccessControl.FileSystemAccessRule]::new($sid, 'Read', 'Allow')
$acl.AddAccessRule($rule)
{WRITE_DACL}
"#
        );
        powershell(&mutation, path, None);
        let changed = inspect(path);
        assert_owned(&changed);
        assert_eq!(changed["protected"], true);
        assert!(
            changed["rules"].as_array().unwrap().iter().any(|rule| {
                rule["sid"] == "S-1-1-0"
                    && rule["kind"] == "Allow"
                    && rule["rights"].as_u64().unwrap() & 0x0002_0089 == 0x0002_0089
            }),
            "Windows fixture prerequisite: actual Everyone allow ACE grants Read"
        );
        restore
    }
}

impl Drop for RestoreAcl {
    fn drop(&mut self) {
        // Keep existing user access throughout; restore only this owned scratch DACL.
        let restoration = format!(
            r#"
if ([IO.Directory]::Exists($env:Q4_PATH)) {{
    $acl = [Security.AccessControl.DirectorySecurity]::new()
}} else {{
    $acl = [Security.AccessControl.FileSecurity]::new()
}}
$acl.SetSecurityDescriptorSddlForm($env:Q4_SDDL, [Security.AccessControl.AccessControlSections]::Access)
{WRITE_DACL}
"#
        );
        // A failed restore still fails the test, but cannot abort an existing
        // setup/assertion unwind before the remaining scratch guards can run.
        let result = std::panic::catch_unwind(|| {
            powershell(
                &restoration,
                &self.path,
                Some(("Q4_SDDL", self.sddl.as_ref())),
            );
        });
        if let Err(error) = result {
            if std::thread::panicking() {
                eprintln!(
                    "Windows fixture ACL restoration failed during unwind: {:?}",
                    self.path
                );
            } else {
                std::panic::resume_unwind(error);
            }
        }
    }
}

struct Junction(PathBuf);

impl Junction {
    fn new(root: &Path, target: &Path, name: &str) -> Self {
        assert_private(root);
        assert_private(target);
        let link = Self(root.join(name));
        powershell(
            r#"
New-Item -ItemType Junction -Path $env:Q4_PATH -Target $env:Q4_TARGET | Out-Null
$true | ConvertTo-Json
"#,
            &link.0,
            Some(("Q4_TARGET", target.as_os_str())),
        );
        let actual = inspect(&link.0);
        assert_eq!(actual["reparse"], true, "actual reparse attribute is set");
        assert_eq!(actual["link_type"], "Junction");
        let actual_target = PathBuf::from(actual["target"][0].as_str().unwrap());
        assert_eq!(
            actual_target.canonicalize().unwrap(),
            target.canonicalize().unwrap()
        );
        link
    }
}

impl Drop for Junction {
    fn drop(&mut self) {
        // Nonrecursive: remove the directory link itself, never its target.
        if self.0.exists() {
            fs::remove_dir(&self.0).expect("owned junction is removed before scratch roots");
        }
    }
}

#[test]
fn windows_private_publication_discovery_read_and_removal() {
    let root = ScratchDir::new("windows-private");
    let expected = descriptor(31);
    let published = publish_descriptor_in(&root, &expected).unwrap();
    let directory_acl = assert_private(&root);
    assert_private(published.path());
    eprintln!(
        "Windows authority prerequisite: filesystem={}, TokenUser={}, TokenOwner={}, scratch owner={}",
        directory_acl["filesystem"],
        directory_acl["user"],
        directory_acl["token_owner"],
        directory_acl["owner"]
    );
    assert_eq!(
        read_descriptor_file(published.path()).unwrap().instance_id,
        expected.instance_id
    );
    let report = discover_descriptors_in(&root).unwrap();
    assert!(report.issues.is_empty());
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(
        report.candidates[0].descriptor.instance_id,
        expected.instance_id
    );
    let path = published.path().to_path_buf();
    published.remove().unwrap();
    assert!(!path.exists());
    assert!(
        discover_descriptors_in(&root)
            .unwrap()
            .candidates
            .is_empty()
    );
}

#[test]
fn windows_everyone_read_descriptor_is_refused_without_hiding_valid_peer() {
    let root = ScratchDir::new("windows-file-acl");
    let bad = publish_descriptor_in(&root, &descriptor(32)).unwrap();
    let good_descriptor = descriptor(33);
    let good = publish_descriptor_in(&root, &good_descriptor).unwrap();
    let restore = RestoreAcl::grant_everyone_read(bad.path());
    let report = discover_descriptors_in(&root).unwrap();
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(
        report.candidates[0].descriptor.instance_id,
        good_descriptor.instance_id
    );
    assert_eq!(
        report.issues,
        vec![DiscoveryIssue {
            file_name: descriptor(32).file_name(),
            message: UNTRUSTED.to_owned(),
        }]
    );
    drop(restore);
    assert_private(bad.path());
    bad.remove().unwrap();
    good.remove().unwrap();
}

#[test]
fn windows_everyone_read_directory_is_refused_before_enumeration() {
    let root = ScratchDir::new("windows-directory-acl");
    let published = publish_descriptor_in(&root, &descriptor(34)).unwrap();
    let restore = RestoreAcl::grant_everyone_read(&root);
    // Publication would repair the adversarial directory; discovery must not.
    assert_eq!(
        discover_descriptors_in(&root).unwrap_err().to_string(),
        UNTRUSTED
    );
    drop(restore);
    assert_private(&root);
    published.remove().unwrap();
}

#[test]
fn windows_junction_directory_and_json_entry_are_refused_without_target_changes() {
    let root = ScratchDir::new("windows-junction-links");
    let target = ScratchDir::new("windows-junction-target");
    let good = publish_descriptor_in(&root, &descriptor(35)).unwrap();
    let target_descriptor = publish_descriptor_in(&target, &descriptor(36)).unwrap();
    let canary = target.join("canary.txt");
    fs::write(&canary, b"owned target must survive unchanged").unwrap();
    let target_before = fs::read(target_descriptor.path()).unwrap();
    {
        let explicit = Junction::new(&root, &target, "directory-link");
        let entry = Junction::new(&root, &target, "redirect.json");
        assert_eq!(
            discover_descriptors_in(&explicit.0)
                .unwrap_err()
                .to_string(),
            REDIRECT
        );
        let report = discover_descriptors_in(&root).unwrap();
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(
            report.candidates[0].descriptor.instance_id,
            descriptor(35).instance_id
        );
        assert_eq!(
            report.issues,
            vec![DiscoveryIssue {
                file_name: "redirect.json".to_owned(),
                message: REDIRECT.to_owned(),
            }]
        );
        drop(entry);
        drop(explicit);
    }
    assert!(!root.join("directory-link").exists());
    assert!(!root.join("redirect.json").exists());
    assert_eq!(
        fs::read(&canary).unwrap(),
        b"owned target must survive unchanged"
    );
    assert!(
        fs::read(target_descriptor.path()).unwrap() == target_before,
        "target descriptor bytes remain unchanged"
    );
    target_descriptor.remove().unwrap();
    good.remove().unwrap();
}

fn foreign_fixture(relative: &str, directory: bool) -> PathBuf {
    let path =
        PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot is set")).join(relative);
    let actual = inspect(&path);
    assert_eq!(
        actual["directory"], directory,
        "Windows fixture prerequisite: expected object type"
    );
    assert_eq!(
        actual["reparse"], false,
        "Windows fixture prerequisite: OS object is not redirected"
    );
    assert_ne!(
        actual["owner"], actual["user"],
        "Windows fixture prerequisite: OS owner differs from TokenUser"
    );
    assert_ne!(
        actual["owner"], actual["token_owner"],
        "Windows fixture prerequisite: OS owner differs from TokenOwner"
    );
    eprintln!(
        "Read-only foreign Windows fixture: {path:?}, owner={}, TokenUser={}, TokenOwner={}",
        actual["owner"], actual["user"], actual["token_owner"]
    );
    path
}

#[test]
fn windows_foreign_os_directory_is_refused_before_enumeration() {
    let path = foreign_fixture("System32", true);
    assert_eq!(
        discover_descriptors_in(&path).unwrap_err().to_string(),
        FOREIGN
    );
}

#[test]
fn windows_foreign_os_file_is_refused_before_descriptor_parsing() {
    // This proves the real file-owner boundary, not a foreign valid JSON
    // descriptor traversing enumeration or connection. The OS file is read-only.
    let path = foreign_fixture("System32/kernel32.dll", false);
    assert_eq!(
        read_descriptor_file(&path).unwrap_err().to_string(),
        FOREIGN
    );
}
