pub(super) fn record(calls: &mut Vec<&'static str>) -> Result<(), &'static str> {
    calls.push("record");
    Ok(())
}

pub(super) fn reject(calls: &mut Vec<&'static str>) -> Result<(), &'static str> {
    calls.push("reject");
    Err("registration failed")
}

#[test]
fn registration_borrows_the_host_registry_once() {
    fn register(
        calls: &mut Vec<&'static str>,
        evaluations: &mut usize,
    ) -> Result<(), &'static str> {
        register_family_actions! {
            [{ *evaluations += 1; calls }]
            families {
                wire: none, handlers: [], actions: [record, record]
            }
        }
        Ok(())
    }

    let mut calls = Vec::new();
    let mut evaluations = 0;
    assert_eq!(register(&mut calls, &mut evaluations), Ok(()));
    assert_eq!(evaluations, 1);
    assert_eq!(calls, ["record", "record"]);
}

#[test]
fn registration_stops_at_the_first_error() {
    fn register(calls: &mut Vec<&'static str>) -> Result<(), &'static str> {
        register_family_actions! {
            [calls]
            families {
                wire: none, handlers: [], actions: [record, reject, record]
            }
        }
        Ok(())
    }

    let mut calls = Vec::new();
    assert_eq!(register(&mut calls), Err("registration failed"));
    assert_eq!(calls, ["record", "reject"]);
}
