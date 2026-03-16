#![no_main]
use libfuzzer_sys::fuzz_target;
use org2jsonl::json_to_org::entries_to_org;
use org2jsonl::org_to_json::org_to_entries;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let entries = org_to_entries(s);
        let org1 = entries_to_org(&entries);
        // Second round-trip must be idempotent
        let entries2 = org_to_entries(&org1);
        let org2 = entries_to_org(&entries2);
        assert_eq!(org1, org2, "idempotency violation on fuzzed input");
    }
});
