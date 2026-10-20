#!/usr/bin/env bash
# Release build of the no_std firmware.
# rust-lld ships inside the esp toolchain and is not on PATH.
set -euo pipefail
cd "$(dirname "$0")"
sysroot="$(rustc --print sysroot)"
lld_bin="$sysroot/lib/rustlib/x86_64-unknown-linux-gnu/bin"
if [[ -x "$lld_bin/rust-lld" ]]; then
    export PATH="$lld_bin:$PATH"
fi
# esp-radio 1.2.2 hardcodes ce_len_type 0, which ignores the HCI event-length
# cap. The Nano's settings burst then overflows the controller's 9 ACL RX
# buffers. Patch the pinned checkout before the build sees it.
python3 - <<'PY'
import json, pathlib, subprocess, sys
meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1"]))
found = False
for pkg in meta["packages"]:
    if pkg["name"] != "esp-radio":
        continue
    path = pathlib.Path(pkg["manifest_path"]).parent / "src/ble/os_adapter_esp32c3_s3.rs"
    text = path.read_text()
    old = "        ce_len_type: 0,\n"
    new = "        ce_len_type: 1, // CE mode: honor the HCI connection-event length\n"
    if old in text:
        path.write_text(text.replace(old, new, 1))
        print(f"patched {path}", file=sys.stderr)
    elif "ce_len_type: 1," in text:
        print(f"ce_len_type already 1 in {path}", file=sys.stderr)
    else:
        sys.exit(f"ce_len_type assignment missing in {path}")
    btdm = path.parent / "btdm.rs"
    btdm_text = btdm.read_text()
    btdm_old = '''unsafe extern "C" fn btdm_sleep_check_duration(_slot_cnt: i32) -> i32 {
    todo!();
}

unsafe extern "C" fn btdm_sleep_enter_phase1(_lpcycles: i32) {
    todo!();
}

unsafe extern "C" fn btdm_sleep_enter_phase2() {
    todo!();
}

unsafe extern "C" fn btdm_sleep_exit_phase1() {
    todo!();
}

unsafe extern "C" fn btdm_sleep_exit_phase2() {
    todo!();
}

unsafe extern "C" fn btdm_sleep_exit_phase3() {
    todo!();
}'''
    btdm_new = '''unsafe extern "C" fn btdm_sleep_check_duration(_slot_cnt: i32) -> i32 {
    // Modem sleep is off. Returning non-zero makes the controller call the
    // enter/exit hooks, and those used to hit `todo!()` from its run loop.
    0
}

unsafe extern "C" fn btdm_sleep_enter_phase1(_lpcycles: i32) {}

unsafe extern "C" fn btdm_sleep_enter_phase2() {}

unsafe extern "C" fn btdm_sleep_exit_phase1() {}

unsafe extern "C" fn btdm_sleep_exit_phase2() {}

unsafe extern "C" fn btdm_sleep_exit_phase3() {}'''
    if btdm_old in btdm_text:
        btdm.write_text(btdm_text.replace(btdm_old, btdm_new, 1))
        print(f"patched {btdm}", file=sys.stderr)
    elif "Modem sleep is off" in btdm_text:
        print(f"sleep hooks already stubbed in {btdm}", file=sys.stderr)
    else:
        sys.exit(f"sleep hooks missing in {btdm}")
    found = True
    break
if not found:
    sys.exit("esp-radio package not in cargo metadata")
PY
exec cargo bare "$@"
