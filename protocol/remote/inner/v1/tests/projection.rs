use cc_desk_remote_record::projection::{decode, encode, Message, MAX_CELLS};
use serde_json::{json, Value};

fn sample() -> Value {
    json!({"profile":1,"message":"SnapshotPart","body":{
        "stamp":{"target":{"host":"11".repeat(32),"session":"22".repeat(16),
            "run":"test-only","generation":"1","incarnation":"2"},
            "binding":"33".repeat(32),"barrier":"44".repeat(16),"revision":"18446744073709551615",
            "dimensionEpoch":"0","modeEpoch":"0","structureEpoch":"0","syncEpoch":"7"},
        "state":{"rows":1,"columns":2,"buffer":"Normal","focusReporting":false,
            "cursor":{"row":0,"column":0,"shape":"Block","blinking":false}},
        "start":0,"cells":[cell(),cell()]}})
}
fn cell() -> Value {
    json!({"c":"<","r":"Single","w":false,"f":{"Palette":256},"b":{"Palette":257},"s":0})
}
fn read(v: &Value, minor: u16) -> Result<Message, cc_desk_remote_record::projection::Error> {
    decode(&serde_json::to_vec(v).unwrap(), minor)
}
#[test]
fn projection_known_profile_roundtrips_at_both_negotiated_minors_without_js_integer_loss() {
    for minor in [0, 1] {
        let message = read(&sample(), minor).unwrap();
        let wire = encode(&message, minor).unwrap();
        assert!(wire.len() <= 32768);
        assert_eq!(decode(&wire, minor).unwrap(), message);
        assert!(String::from_utf8(wire)
            .unwrap()
            .contains("18446744073709551615"));
    }
    assert!(read(&sample(), 2).is_err());
}
#[test]
fn projection_shared_client_vector_is_accepted_by_the_native_codec() {
    let v = include_bytes!("../test-vectors/projection-snapshot.json");
    for minor in [0, 1] {
        let m = decode(v, minor).unwrap();
        assert_eq!(decode(&encode(&m, minor).unwrap(), minor).unwrap(), m);
    }
    for bits in [384, 448, 512, 65535] {
        let mut v = sample();
        v["body"]["cells"][0]["s"] = json!(bits);
        assert!(read(&v, 0).is_err());
    }
}
#[test]
fn projection_worst_cell_and_metadata_representation_fits_full_grid_transfer_budget() {
    let mut v = sample();
    v["body"]["state"]["rows"] = json!(100);
    v["body"]["state"]["columns"] = json!(240);
    v["body"]["stamp"]["target"]["run"] = json!("\"".repeat(128));
    for name in [
        "revision",
        "dimensionEpoch",
        "modeEpoch",
        "structureEpoch",
        "syncEpoch",
    ] {
        v["body"]["stamp"][name] = json!("18446744073709551615");
    }
    let c = json!({"c":"😀","r":"LeadingWideSpacer","w":false,"f":{"Rgb":[255,255,255]},"b":{"Rgb":[255,255,255]},"s":383});
    v["body"]["cells"] = json!(vec![c; 128]);
    let m = read(&v, 0).unwrap();
    let bytes = encode(&m, 0).unwrap();
    assert!(bytes.len() <= 32768);
    assert!(bytes.len() * 188 <= 4 * 1024 * 1024);
    // This is a wire-size upper bound, not a claim that arbitrary role layouts install.
}
#[test]
fn projection_ack_is_a_bounded_exact_control_stamp_not_a_revision_number() {
    use cc_desk_remote_record::projection::{decode_ack, encode_ack};
    let v = json!({"message":"ProjectionAck","profile":1,"stamp":sample()["body"]["stamp"]});
    for minor in [0, 1] {
        let ack = decode_ack(&serde_json::to_vec(&v).unwrap(), minor).unwrap();
        assert_eq!(ack.stamp.revision, "18446744073709551615");
        assert!(decode_ack(&encode_ack(&ack, minor).unwrap(), minor).unwrap() == ack);
    }
    assert!(decode_ack(&vec![b' '; 16385], 0).is_err());
    let mut bad = v.clone();
    bad["stamp"]["binding"] = json!("f".repeat(63));
    assert!(decode_ack(&serde_json::to_vec(&bad).unwrap(), 0).is_err());
    assert!(decode_ack(b"{\"profile\":1,\"revision\":\"1\"}", 0).is_err());
    assert!(decode_ack(&serde_json::to_vec(&v).unwrap(), 2).is_err());
    let mut wrong = v.clone();
    wrong["message"] = json!("LeaseRenewal");
    assert!(decode_ack(&serde_json::to_vec(&wrong).unwrap(), 0).is_err());
}
#[test]
fn projection_refuses_size_cell_count_geometry_scalar_and_color_before_admission() {
    assert!(decode(&vec![b' '; 32769], 0).is_err());
    let mut v = sample();
    v["body"]["state"]["rows"] = json!(100);
    v["body"]["state"]["columns"] = json!(240);
    v["body"]["cells"] = json!(vec![cell(); MAX_CELLS]);
    assert!(read(&v, 0).is_ok());
    v["body"]["cells"] = json!(vec![cell(); MAX_CELLS + 1]);
    assert!(read(&v, 0).is_err());
    for (field, value) in [("columns", json!(1)), ("rows", json!(101))] {
        let mut v = sample();
        v["body"]["state"][field] = value;
        assert!(read(&v, 0).is_err());
    }
    for value in [json!(""), json!("ab"), json!("\u{1b}"), json!("\n")] {
        let mut v = sample();
        v["body"]["cells"][0]["c"] = value;
        assert!(read(&v, 0).is_err());
    }
    let mut v = sample();
    v["body"]["cells"][0]["f"] = json!({"Palette":999});
    assert!(read(&v, 0).is_err());
}
#[test]
fn projection_rejects_target_epoch_identity_range_and_unknown_required_semantics() {
    for (field, value) in [
        ("generation", json!("01")),
        ("incarnation", json!("18446744073709551616")),
        ("host", json!("A".repeat(64))),
        ("run", json!("x".repeat(129))),
    ] {
        let mut v = sample();
        v["body"]["stamp"]["target"][field] = value;
        assert!(read(&v, 0).is_err());
    }
    let mut v = sample();
    v["profile"] = json!(2);
    assert!(read(&v, 0).is_err());
    let mut v = sample();
    v["body"]["cells"][0]["r"] = json!("RunScript");
    assert!(read(&v, 0).is_err());
    let mut v = sample();
    v["body"]["start"] = json!(1);
    assert!(read(&v, 0).is_err());
    let mut v = sample();
    v["body"]["state"]["cursor"]["column"] = json!(2);
    assert!(read(&v, 0).is_err());
}
#[test]
fn projection_ignores_additive_optional_fields_but_rejects_duplicate_known_fields() {
    let mut v = sample();
    v["optionalFuture"] = json!(true);
    v["body"]["state"]["optionalFuture"] = json!(4);
    assert!(read(&v, 0).is_ok());
    let wire = serde_json::to_string(&sample())
        .unwrap()
        .replace("\"profile\":1", "\"profile\":1,\"profile\":1");
    assert!(decode(wire.as_bytes(), 0).is_err());
}
#[test]
fn projection_delta_requires_exact_increasing_revision_and_unique_bounded_indices() {
    let v = sample();
    let stamp = &v["body"]["stamp"];
    let state = &v["body"]["state"];
    let mut delta = json!({"profile":1,"message":"Delta","body":{"stamp":stamp,"state":state,
        "base":"18446744073709551614","changes":[{"index":0,"cell":cell()}]}});
    assert!(read(&delta, 1).is_ok());
    delta["body"]["changes"] = json!([]);
    assert!(read(&delta, 1).is_ok()); // mode-only
    delta["body"]["base"] = json!("18446744073709551615");
    assert!(read(&delta, 1).is_err());
    delta["body"]["base"] = json!("0");
    delta["body"]["changes"] = json!([{"index":0,"cell":cell()},{"index":0,"cell":cell()}]);
    assert!(read(&delta, 1).is_err());
}
