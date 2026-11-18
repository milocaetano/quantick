//! The order in which a frame settles changed markets and deferred layouts.
pub trait Host {
    /// Attach the selected market; report whether a live market changed.
    fn switch_market(&mut self) -> bool;
    fn apply_spec(&mut self);
    fn follow_market(&mut self);
    fn pending_layouts(&mut self);
    fn maintain_layouts(&mut self);
    fn maintain_workspaces(&mut self);
}

pub fn apply_feed_switch(host: &mut impl Host) {
    let changed = host.switch_market();
    host.apply_spec();
    if changed {
        host.follow_market();
    }
}

pub fn apply_layout_switches(host: &mut impl Host) {
    host.pending_layouts();
    host.maintain_layouts();
    host.maintain_workspaces();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct FakeHost {
        changed: bool,
        calls: Vec<&'static str>,
    }
    impl Host for FakeHost {
        fn switch_market(&mut self) -> bool {
            self.calls.push("switch");
            self.changed
        }
        fn apply_spec(&mut self) {
            self.calls.push("spec");
        }
        fn follow_market(&mut self) {
            self.calls.push("follow");
        }
        fn pending_layouts(&mut self) {
            self.calls.push("pending");
        }
        fn maintain_layouts(&mut self) {
            self.calls.push("layouts");
        }
        fn maintain_workspaces(&mut self) {
            self.calls.push("workspaces");
        }
    }
    #[test]
    fn only_a_changed_market_moves_layouts_after_its_spec() {
        let mut host = FakeHost::default();
        apply_feed_switch(&mut host);
        assert_eq!(host.calls, ["switch", "spec"]);
        host.calls.clear();
        host.changed = true;
        apply_feed_switch(&mut host);
        assert_eq!(host.calls, ["switch", "spec", "follow"]);
    }
    #[test]
    fn new_panes_are_seeded_before_the_workspace_is_saved() {
        let mut host = FakeHost::default();
        apply_layout_switches(&mut host);
        assert_eq!(host.calls, ["pending", "layouts", "workspaces"]);
    }
}
