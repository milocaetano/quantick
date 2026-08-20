//! Dynamic module, authority, effect-policy, and capability registries.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    canonical::validate_control_value,
    cursor::PaginationConsistency,
    handshake::ProfileAuthority,
    id::{
        AvailabilityReasonId, CapabilityId, ConfirmationClassId, CostClassId, EffectId, ModuleId,
        PermissionId, PreconditionId, ProfileId, RiskFlagId,
    },
    limits::{
        CONTROL_CAPABILITY_DESCRIPTOR_MAX_BYTES, CONTROL_IDEMPOTENCY_RECORD_MAX_BYTES,
        CONTROL_MAX_PAGE_ITEMS, CONTROL_MAX_RESPONSE_BYTES, CONTROL_REASON_MAX_BYTES,
    },
    schema::{SchemaError, validate_instance, validate_schema},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModuleDescriptor {
    pub id: ModuleId,
    pub title: String,
    pub description: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DefaultGrant {
    Granted,
    Denied,
    Prompt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PermissionDescriptor {
    pub id: PermissionId,
    pub label: String,
    pub description: String,
    pub sensitive: bool,
    pub default_grant: DefaultGrant,
    pub profile_ceilings: BTreeSet<ProfileId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileDescriptor {
    pub id: ProfileId,
    pub label: String,
    #[serde(default)]
    pub inherits: BTreeSet<ProfileId>,
    #[serde(default)]
    pub permissions: BTreeSet<PermissionId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpHintFloor {
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub open_world: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EffectConstraints {
    /// `Some(true)` requires reads; `Some(false)` requires a user-visible effect.
    pub required_read_only: Option<bool>,
    pub allows_destructive: bool,
    pub durable_requires_reversible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub irreversible_transient_risk: Option<RiskFlagId>,
    pub allows_risk_reducing: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EffectPolicy {
    pub id: EffectId,
    pub permission_floor: PermissionId,
    pub profile_ceilings: BTreeSet<ProfileId>,
    pub confirmation_class: ConfirmationClassId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_reducing_confirmation_class: Option<ConfirmationClassId>,
    pub mcp_hint_floor: McpHintFloor,
    #[serde(default)]
    pub required_risk_flags: BTreeSet<RiskFlagId>,
    pub constraints: EffectConstraints,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyPolicy {
    Forbidden,
    Optional,
    Required,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RevisionPolicy {
    Forbidden,
    OptionalForAdditive,
    Required,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EffectPersistence {
    None,
    Transient,
    Durable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AvailabilityStatus {
    Available,
    Unavailable,
    ConfirmationRequired,
    PermissionDenied,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Availability {
    pub status: AvailabilityStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<AvailabilityReasonId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_step: Option<String>,
}

impl Availability {
    pub const fn available() -> Self {
        Self {
            status: AvailabilityStatus::Available,
            reason_code: None,
            next_step: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PreconditionDescriptor {
    pub id: PreconditionId,
    pub description: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExpectedCost {
    pub class: CostClassId,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = CONTROL_MAX_PAGE_ITEMS))]
    pub max_items: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = CONTROL_MAX_RESPONSE_BYTES))]
    pub max_response_bytes: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityExample {
    pub title: String,
    pub input: Value,
    pub output: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityDescriptor {
    pub id: CapabilityId,
    #[schemars(range(min = 1))]
    pub version: u32,
    pub title: String,
    pub description: String,
    pub module: ModuleId,
    pub input_schema: Value,
    pub output_schema: Value,
    #[serde(default)]
    pub examples: Vec<CapabilityExample>,
    pub effect: EffectId,
    #[serde(default)]
    pub risk_flags: BTreeSet<RiskFlagId>,
    pub read_only: bool,
    pub idempotency: IdempotencyPolicy,
    pub revision_policy: RevisionPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = CONTROL_REASON_MAX_BYTES))]
    pub stale_input_safety: Option<String>,
    pub dry_run_supported: bool,
    pub persistence: EffectPersistence,
    pub reversible: bool,
    pub destructive: bool,
    pub risk_reducing: bool,
    #[serde(default)]
    pub required_permissions: BTreeSet<PermissionId>,
    #[serde(default)]
    pub preconditions: Vec<PreconditionDescriptor>,
    pub confirmation_class: ConfirmationClassId,
    pub availability: Availability,
    pub expected_cost: ExpectedCost,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagination: Option<PaginationConsistency>,
}

pub trait ControlModule {
    /// Register module-owned permissions before profile ceilings are finalized.
    fn register_permissions(&self, _registry: &mut ControlRegistry) -> Result<(), RegistryError> {
        Ok(())
    }

    /// Register effects and capabilities after authority finalization.
    fn register(&self, registry: &mut ControlRegistry) -> Result<(), RegistryError>;
}

#[derive(Default)]
pub struct ControlRegistry {
    modules: BTreeMap<ModuleId, ModuleDescriptor>,
    permissions: BTreeMap<PermissionId, PermissionDescriptor>,
    profiles: BTreeMap<ProfileId, ProfileDescriptor>,
    flattened_profiles: BTreeMap<ProfileId, BTreeSet<PermissionId>>,
    profile_ancestry: BTreeMap<ProfileId, BTreeSet<ProfileId>>,
    authority_finalized: bool,
    effects: BTreeMap<EffectId, EffectPolicy>,
    capabilities: BTreeMap<(CapabilityId, u32), CapabilityDescriptor>,
}

impl ControlRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_module(&mut self, descriptor: ModuleDescriptor) -> Result<(), RegistryError> {
        validate_label("module", &descriptor.title, &descriptor.description)?;
        insert_unique(
            &mut self.modules,
            descriptor.id.clone(),
            descriptor,
            "module",
        )
    }

    pub fn register_permission(
        &mut self,
        descriptor: PermissionDescriptor,
    ) -> Result<(), RegistryError> {
        if self.authority_finalized {
            return Err(RegistryError::AuthorityAlreadyFinalized);
        }
        validate_label("permission", &descriptor.label, &descriptor.description)?;
        if descriptor.profile_ceilings.is_empty() {
            return Err(RegistryError::InvalidDescriptor(
                "permission has no profile ceiling".to_owned(),
            ));
        }
        insert_unique(
            &mut self.permissions,
            descriptor.id.clone(),
            descriptor,
            "permission",
        )
    }

    pub fn register_profile(&mut self, descriptor: ProfileDescriptor) -> Result<(), RegistryError> {
        if self.authority_finalized {
            return Err(RegistryError::AuthorityAlreadyFinalized);
        }
        if descriptor.label.trim().is_empty() {
            return Err(RegistryError::InvalidDescriptor(
                "profile label is empty".to_owned(),
            ));
        }
        insert_unique(
            &mut self.profiles,
            descriptor.id.clone(),
            descriptor,
            "profile",
        )
    }

    pub fn finalize_authority(&mut self) -> Result<(), RegistryError> {
        if self.authority_finalized {
            return Ok(());
        }
        for descriptor in self.permissions.values() {
            for profile in &descriptor.profile_ceilings {
                if !self.profiles.contains_key(profile) {
                    return Err(RegistryError::Unknown {
                        kind: "profile",
                        id: profile.to_string(),
                    });
                }
            }
        }

        let mut flattened = BTreeMap::new();
        let mut ancestry = BTreeMap::new();
        let mut visiting = BTreeSet::new();
        for profile in self.profiles.keys() {
            resolve_profile(
                profile,
                &self.profiles,
                &self.permissions,
                &mut visiting,
                &mut flattened,
                &mut ancestry,
            )?;
        }

        // A permission descriptor declares every profile ceiling under which
        // that permission may be granted. Descendants inherit the ceiling,
        // so modules can contribute scopes without editing central profiles.
        for (profile, ancestors) in &ancestry {
            let permissions = flattened
                .get_mut(profile)
                .expect("every resolved profile has a permission set");
            permissions.extend(
                self.permissions
                    .values()
                    .filter(|permission| !permission.profile_ceilings.is_disjoint(ancestors))
                    .map(|permission| permission.id.clone()),
            );
        }

        for (profile, permissions) in &flattened {
            let ancestors = ancestry
                .get(profile)
                .expect("every flattened profile has ancestry");
            for permission in permissions {
                let descriptor = self
                    .permissions
                    .get(permission)
                    .expect("profile resolver checked permissions");
                if descriptor.profile_ceilings.is_disjoint(ancestors) {
                    return Err(RegistryError::InvalidProfile(format!(
                        "profile `{profile}` exceeds the ceiling for permission `{permission}`"
                    )));
                }
            }
        }

        self.flattened_profiles = flattened;
        self.profile_ancestry = ancestry;
        self.authority_finalized = true;
        Ok(())
    }

    pub fn register_effect(&mut self, policy: EffectPolicy) -> Result<(), RegistryError> {
        self.require_authority()?;
        if !self.permissions.contains_key(&policy.permission_floor) {
            return Err(RegistryError::Unknown {
                kind: "permission",
                id: policy.permission_floor.to_string(),
            });
        }
        if policy.profile_ceilings.is_empty() {
            return Err(RegistryError::InvalidDescriptor(
                "effect policy has no profile ceiling".to_owned(),
            ));
        }
        for profile in &policy.profile_ceilings {
            let permissions =
                self.flattened_profiles
                    .get(profile)
                    .ok_or_else(|| RegistryError::Unknown {
                        kind: "profile",
                        id: profile.to_string(),
                    })?;
            if !permissions.contains(&policy.permission_floor) {
                return Err(RegistryError::InvalidDescriptor(format!(
                    "effect `{}` exceeds profile `{profile}` because it lacks permission `{}`",
                    policy.id, policy.permission_floor
                )));
            }
        }
        if policy.risk_reducing_confirmation_class.is_some()
            && !policy.constraints.allows_risk_reducing
        {
            return Err(RegistryError::InvalidDescriptor(
                "effect has a risk-reducing confirmation class but forbids risk reduction"
                    .to_owned(),
            ));
        }
        insert_unique(&mut self.effects, policy.id.clone(), policy, "effect")
    }

    pub fn register_capability(
        &mut self,
        descriptor: CapabilityDescriptor,
    ) -> Result<(), RegistryError> {
        self.require_authority()?;
        validate_label("capability", &descriptor.title, &descriptor.description)?;
        if descriptor.version == 0 {
            return Err(RegistryError::InvalidDescriptor(
                "capability version must be positive".to_owned(),
            ));
        }
        if !self.modules.contains_key(&descriptor.module) {
            return Err(RegistryError::Unknown {
                kind: "module",
                id: descriptor.module.to_string(),
            });
        }
        let module_prefix = format!("{}.", descriptor.module);
        if !descriptor.id.as_str().starts_with(&module_prefix) {
            return Err(RegistryError::InvalidDescriptor(format!(
                "capability `{}` is not owned by module `{}`",
                descriptor.id, descriptor.module
            )));
        }
        let effect =
            self.effects
                .get(&descriptor.effect)
                .ok_or_else(|| RegistryError::Unknown {
                    kind: "effect policy",
                    id: descriptor.effect.to_string(),
                })?;
        validate_capability_metadata(&descriptor, effect)?;

        for permission in &descriptor.required_permissions {
            if !self.permissions.contains_key(permission) {
                return Err(RegistryError::Unknown {
                    kind: "permission",
                    id: permission.to_string(),
                });
            }
        }
        if !descriptor
            .required_permissions
            .contains(&effect.permission_floor)
        {
            return Err(RegistryError::InvalidDescriptor(format!(
                "capability does not declare effect permission floor `{}`",
                effect.permission_floor
            )));
        }
        let reachable = effect.profile_ceilings.iter().any(|profile| {
            self.flattened_profiles
                .get(profile)
                .is_some_and(|permissions| descriptor.required_permissions.is_subset(permissions))
        });
        if !reachable {
            return Err(RegistryError::InvalidDescriptor(
                "no effect profile ceiling grants all required permissions".to_owned(),
            ));
        }

        validate_schema(&descriptor.input_schema)?;
        validate_schema(&descriptor.output_schema)?;
        validate_wire_json("input schema", &descriptor.input_schema)?;
        validate_wire_json("output schema", &descriptor.output_schema)?;
        for example in &descriptor.examples {
            if example.title.trim().is_empty() {
                return Err(RegistryError::InvalidDescriptor(
                    "capability example title is empty".to_owned(),
                ));
            }
            validate_wire_json("example input", &example.input)?;
            validate_wire_json("example output", &example.output)?;
            validate_instance(&descriptor.input_schema, &example.input)?;
            validate_instance(&descriptor.output_schema, &example.output)?;
        }
        let encoded = serde_json::to_vec(&descriptor)
            .map_err(|error| RegistryError::InvalidDescriptor(error.to_string()))?;
        if encoded.len() > CONTROL_CAPABILITY_DESCRIPTOR_MAX_BYTES {
            return Err(RegistryError::InvalidDescriptor(format!(
                "capability descriptor is {} bytes; limit is {CONTROL_CAPABILITY_DESCRIPTOR_MAX_BYTES}",
                encoded.len()
            )));
        }

        let key = (descriptor.id.clone(), descriptor.version);
        if self.capabilities.contains_key(&key) {
            return Err(RegistryError::Duplicate {
                kind: "capability",
                id: format!("{:?}", key),
            });
        }

        insert_unique(&mut self.capabilities, key, descriptor, "capability")
    }

    pub fn capability(&self, id: &CapabilityId, version: u32) -> Option<&CapabilityDescriptor> {
        self.capabilities.get(&(id.clone(), version))
    }

    pub fn capabilities(&self) -> impl Iterator<Item = &CapabilityDescriptor> {
        self.capabilities.values()
    }

    pub fn modules(&self) -> impl Iterator<Item = &ModuleDescriptor> {
        self.modules.values()
    }

    pub fn effect(&self, id: &EffectId) -> Option<&EffectPolicy> {
        self.effects.get(id)
    }

    pub fn permission(&self, id: &PermissionId) -> Option<&PermissionDescriptor> {
        self.permissions.get(id)
    }

    fn require_authority(&self) -> Result<(), RegistryError> {
        if self.authority_finalized {
            Ok(())
        } else {
            Err(RegistryError::AuthorityNotFinalized)
        }
    }
}

impl ProfileAuthority for ControlRegistry {
    fn permission_ceiling(&self, profile: &ProfileId) -> Option<BTreeSet<PermissionId>> {
        self.flattened_profiles.get(profile).cloned()
    }

    fn is_known_permission(&self, permission: &PermissionId) -> bool {
        self.permissions.contains_key(permission)
    }
}

fn resolve_profile(
    profile: &ProfileId,
    profiles: &BTreeMap<ProfileId, ProfileDescriptor>,
    permissions: &BTreeMap<PermissionId, PermissionDescriptor>,
    visiting: &mut BTreeSet<ProfileId>,
    flattened: &mut BTreeMap<ProfileId, BTreeSet<PermissionId>>,
    ancestry: &mut BTreeMap<ProfileId, BTreeSet<ProfileId>>,
) -> Result<(), RegistryError> {
    if flattened.contains_key(profile) {
        return Ok(());
    }
    if !visiting.insert(profile.clone()) {
        return Err(RegistryError::InvalidProfile(format!(
            "profile inheritance cycle contains `{profile}`"
        )));
    }
    let descriptor = profiles
        .get(profile)
        .ok_or_else(|| RegistryError::Unknown {
            kind: "profile",
            id: profile.to_string(),
        })?;
    let mut expanded_permissions = descriptor.permissions.clone();
    let mut expanded_ancestry = BTreeSet::from([profile.clone()]);
    for permission in &descriptor.permissions {
        if !permissions.contains_key(permission) {
            return Err(RegistryError::Unknown {
                kind: "permission",
                id: permission.to_string(),
            });
        }
    }
    for parent in &descriptor.inherits {
        resolve_profile(parent, profiles, permissions, visiting, flattened, ancestry)?;
        expanded_permissions.extend(
            flattened
                .get(parent)
                .expect("parent was resolved")
                .iter()
                .cloned(),
        );
        expanded_ancestry.extend(
            ancestry
                .get(parent)
                .expect("parent ancestry was resolved")
                .iter()
                .cloned(),
        );
    }
    visiting.remove(profile);
    flattened.insert(profile.clone(), expanded_permissions);
    ancestry.insert(profile.clone(), expanded_ancestry);
    Ok(())
}

fn validate_capability_metadata(
    descriptor: &CapabilityDescriptor,
    policy: &EffectPolicy,
) -> Result<(), RegistryError> {
    if descriptor.read_only && descriptor.destructive {
        return invalid("a read-only capability cannot be destructive");
    }
    if descriptor.read_only
        && (descriptor.persistence != EffectPersistence::None || descriptor.reversible)
    {
        return invalid("a read-only capability cannot persist or advertise reversal");
    }
    if descriptor.persistence == EffectPersistence::None && descriptor.reversible {
        return invalid("a capability without an effect cannot advertise reversal");
    }
    if policy
        .constraints
        .required_read_only
        .is_some_and(|required| required != descriptor.read_only)
    {
        return invalid("capability read-only metadata contradicts its effect policy");
    }
    if descriptor.destructive && !policy.constraints.allows_destructive {
        return invalid("capability is destructive but its effect policy forbids destruction");
    }
    if descriptor.persistence == EffectPersistence::Durable
        && policy.constraints.durable_requires_reversible
        && !descriptor.reversible
    {
        return invalid("durable effect must be reversible under its effect policy");
    }
    if descriptor.persistence == EffectPersistence::Transient
        && !descriptor.reversible
        && policy
            .constraints
            .irreversible_transient_risk
            .as_ref()
            .is_some_and(|risk| !descriptor.risk_flags.contains(risk))
    {
        return invalid("irreversible transient effect lacks its required risk flag");
    }
    if !policy.required_risk_flags.is_subset(&descriptor.risk_flags) {
        return invalid("capability omits a risk flag required by its effect policy");
    }
    if descriptor.risk_reducing {
        if !policy.constraints.allows_risk_reducing {
            return invalid("effect policy does not allow risk-reducing metadata");
        }
        if policy.risk_reducing_confirmation_class.as_ref() != Some(&descriptor.confirmation_class)
        {
            return invalid("risk-reducing capability has the wrong confirmation class");
        }
    } else if descriptor.confirmation_class != policy.confirmation_class {
        return invalid("capability confirmation class contradicts its effect policy");
    }
    if descriptor.read_only && !policy.mcp_hint_floor.read_only {
        // A policy may be conservatively writable, so this direction is safe.
    } else if !descriptor.read_only && policy.mcp_hint_floor.read_only {
        return invalid("writable capability contradicts the effect's read-only MCP hint");
    }
    if descriptor.destructive && !policy.mcp_hint_floor.destructive {
        return invalid("destructive capability contradicts the effect's MCP hint");
    }
    match descriptor.idempotency {
        IdempotencyPolicy::Forbidden if policy.mcp_hint_floor.idempotent => {
            return invalid("effect advertises idempotency but capability forbids retry keys");
        }
        IdempotencyPolicy::Required if descriptor.read_only => {
            return invalid("read-only capability cannot require an idempotency key");
        }
        _ => {}
    }
    if descriptor.read_only && descriptor.revision_policy != RevisionPolicy::Forbidden {
        return invalid("read-only capability cannot require expected revisions");
    }
    if descriptor.read_only && descriptor.dry_run_supported {
        return invalid("read-only capability cannot advertise a dry run");
    }
    if descriptor.destructive && descriptor.revision_policy != RevisionPolicy::Required {
        return invalid("destructive capability must require expected revisions");
    }
    match descriptor.revision_policy {
        RevisionPolicy::OptionalForAdditive => {
            if descriptor
                .stale_input_safety
                .as_ref()
                .is_none_or(|reason| reason.trim().is_empty())
            {
                return invalid("additive action must explain why stale input is safe");
            }
            if descriptor
                .stale_input_safety
                .as_ref()
                .is_some_and(|reason| reason.len() > CONTROL_REASON_MAX_BYTES)
            {
                return invalid("stale-input rationale exceeds its byte limit");
            }
        }
        RevisionPolicy::Forbidden | RevisionPolicy::Required => {
            if descriptor.stale_input_safety.is_some() {
                return invalid("stale-input rationale only applies to additive actions");
            }
        }
    }
    if !descriptor.read_only && descriptor.revision_policy == RevisionPolicy::Forbidden {
        return invalid("writable capability must declare its revision behavior");
    }
    if descriptor.idempotency == IdempotencyPolicy::Required
        && descriptor
            .expected_cost
            .max_response_bytes
            .is_none_or(|bytes| bytes > CONTROL_IDEMPOTENCY_RECORD_MAX_BYTES)
    {
        return invalid("idempotent terminal result lacks a bounded retention-safe size");
    }
    match descriptor.availability.status {
        AvailabilityStatus::Available => {
            if descriptor.availability.reason_code.is_some()
                || descriptor.availability.next_step.is_some()
            {
                return invalid("available capability cannot include an unavailable reason");
            }
        }
        _ => {
            if descriptor.availability.reason_code.is_none()
                || descriptor
                    .availability
                    .next_step
                    .as_ref()
                    .is_none_or(|step| step.trim().is_empty())
            {
                return invalid("unavailable capability requires a reason and next step");
            }
        }
    }
    let mut preconditions = BTreeSet::new();
    for precondition in &descriptor.preconditions {
        if precondition.description.trim().is_empty() || !preconditions.insert(&precondition.id) {
            return invalid("preconditions must have unique IDs and descriptions");
        }
    }
    if descriptor.pagination.is_some() && !descriptor.read_only {
        return invalid("only read-only capabilities may paginate");
    }
    if descriptor.pagination.is_some() && descriptor.expected_cost.max_items.is_none() {
        return invalid("paginated capability must declare a maximum page item count");
    }
    if descriptor.expected_cost.max_items == Some(0)
        || descriptor.expected_cost.max_response_bytes == Some(0)
    {
        return invalid("expected cost bounds must be positive when present");
    }
    if descriptor
        .expected_cost
        .max_items
        .is_some_and(|items| items > CONTROL_MAX_PAGE_ITEMS)
        || descriptor
            .expected_cost
            .max_response_bytes
            .is_some_and(|bytes| bytes > CONTROL_MAX_RESPONSE_BYTES)
    {
        return invalid("expected cost exceeds a reviewed protocol hard limit");
    }
    Ok(())
}

fn validate_label(kind: &str, label: &str, description: &str) -> Result<(), RegistryError> {
    if label.trim().is_empty() || description.trim().is_empty() {
        return Err(RegistryError::InvalidDescriptor(format!(
            "{kind} title/label and description must be non-empty"
        )));
    }
    Ok(())
}

fn validate_wire_json(kind: &str, value: &Value) -> Result<(), RegistryError> {
    validate_control_value(value).map_err(|error| {
        RegistryError::InvalidDescriptor(format!("{kind} is not valid control JSON: {error}"))
    })
}

fn invalid<T>(message: &str) -> Result<T, RegistryError> {
    Err(RegistryError::InvalidDescriptor(message.to_owned()))
}

fn insert_unique<K: Ord + fmt::Debug, V>(
    map: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    kind: &'static str,
) -> Result<(), RegistryError> {
    if map.contains_key(&key) {
        return Err(RegistryError::Duplicate {
            kind,
            id: format!("{key:?}"),
        });
    }
    map.insert(key, value);
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistryError {
    Duplicate { kind: &'static str, id: String },
    Unknown { kind: &'static str, id: String },
    AuthorityNotFinalized,
    AuthorityAlreadyFinalized,
    InvalidProfile(String),
    InvalidDescriptor(String),
    Schema(SchemaError),
}

impl From<SchemaError> for RegistryError {
    fn from(error: SchemaError) -> Self {
        Self::Schema(error)
    }
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate { kind, id } => write!(formatter, "duplicate {kind} ID {id}"),
            Self::Unknown { kind, id } => write!(formatter, "unknown {kind} `{id}`"),
            Self::AuthorityNotFinalized => {
                formatter.write_str("permission and profile registries are not finalized")
            }
            Self::AuthorityAlreadyFinalized => {
                formatter.write_str("permission and profile registries are already finalized")
            }
            Self::InvalidProfile(message) => write!(formatter, "invalid profile: {message}"),
            Self::InvalidDescriptor(message) => {
                write!(formatter, "invalid descriptor: {message}")
            }
            Self::Schema(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RegistryError {}
