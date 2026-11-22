//! Descriptor docking for the app-owned strategy conversation adapter.

pub(crate) fn action_descriptors() -> Vec<quantick_control::registry::CapabilityDescriptor> {
    use quantick_control_schema::strategy_describe as schema;
    [
        schema::OPEN_CAPABILITY_ID,
        schema::READ_CAPABILITY_ID,
        schema::SUBMIT_CAPABILITY_ID,
        schema::CANCEL_CAPABILITY_ID,
        schema::SAVE_CAPABILITY_ID,
    ]
    .map(|id| schema::descriptor(id, false))
    .into()
}
