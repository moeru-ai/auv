#!/usr/bin/env python3
"""Opt-in macOS receiver tests using real Chrome/Electron windows and AUV input.

Build AUV first. Pass an installed Electron executable with --electron and a
directory for --output. Profiles are temporary; receiver evidence is retained.
"""

import argparse
import http.server
import json
import pathlib
import plistlib
import select
import shutil
import subprocess
import tempfile
import threading
import time
import urllib.parse


FIXTURES = pathlib.Path(__file__).resolve().parent
REPOSITORY = FIXTURES.parents[4]
CASES = [
    ("unicode_bmp", "", [["input.typeText", "A猫"]], "A猫", 2, 2),
    ("unicode_emoji", "", [["input.typeText", "😀"]], "😀", 1, 1),
    ("plain_key", "", [["input.keys", "b"]], "b", 1, 1),
    ("shift_key", "", [["input.keys", "shift", "b"]], "B", 1, 1),
    ("repeat_key", "", [["input.keys", "x", "--count", "3", "--interval-ms", "50"]], "xxx", 3, 3),
    ("select_all_replace", "replace me", [["input.keys", "cmd", "a"], ["input.typeText", "Z"]], "Z", 1, None),
    ("arrow_left", "ab", [["input.keys", "left"], ["input.typeText", "X"]], "aXb", 1, 2),
    ("return", "ab", [["input.keys", "return"]], "ab\n", 1, 1),
    ("modifier_release", "", [["input.keys", "shift", "b"], ["input.keys", "c"]], "Bc", 2, 2),
]
HOLD_CASES = [
    ("hold_b", "", [["hold", "b"]], "b", 1, 1),
    ("hold_shift_b", "", [["hold", "shift", "b"]], "B", 1, 1),
    ("split_b", "", [["down", "b"], ["wait", "200"], ["up"]], "b", 1, 1),
    ("timeout_b", "", [["timeout", "b"], ["wait", "350"], ["up"]], "b", 1, 1),
    ("drop_b", "", [["down", "b"], ["wait", "200"], ["drop"]], "b", 1, 1),
    ("held_shift_press_b", "", [["down", "shift"], ["wait", "200"], ["input.keys", "b"],
                               ["up"], ["input.keys", "c"]], "Bc", 2, 2),
    ("hold_select_all", "replace me", [["hold", "cmd", "a"], ["input.typeText", "Z"]], "Z", 1, None),
    ("persistent_press_b", "", [["input.keys", "b"]], "b", 1, 1),
    ("persistent_unicode_bmp", "", [["input.typeText", "A猫"]], "A猫", 2, 2),
    ("persistent_emoji", "", [["input.typeText", "😀"]], "😀", 1, 1),
]


def send(producer, command, window, mode):
    producer.stdin.write(json.dumps({"command": command, "window": window, "mode": mode}) + "\n")
    producer.stdin.flush()
    if not select.select([producer.stdout], [], [], 10)[0]:
        raise RuntimeError("persistent sender reply timed out")
    response = json.loads(producer.stdout.readline())
    if "error" in response:
        raise RuntimeError(response["error"])
    return response


def objects(value):
    if isinstance(value, dict):
        yield value
        for item in value.values():
            yield from objects(item)
    elif isinstance(value, list):
        for item in value:
            yield from objects(item)


def invoke(executable, output, arguments):
    result = subprocess.run(
        [str(executable), "invoke", *arguments, "--json", "--no-overlay", "--store-root", str(output / "runs")],
        capture_output=True, text=True, timeout=20,
    )
    if result.returncode:
        raise RuntimeError(result.stderr or result.stdout)
    return json.loads(result.stdout)


class ReceiverServer(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self):
        self.command = {"caseId": "startup", "initial": ""}
        self.receipt = {}
        self.lock = threading.Lock()
        super().__init__(("127.0.0.1", 0), ReceiverHandler)

    def snapshot(self):
        with self.lock:
            return json.loads(json.dumps(self.receipt))

    def reset(self, case_id, initial):
        with self.lock:
            self.command = {"caseId": case_id, "initial": initial}
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            receipt = self.snapshot()
            if receipt.get("caseId") == case_id:
                assert receipt["value"] == initial and receipt["inputSelected"]
                assert not receipt["events"], "reset must not generate keyboard events"
                return
            time.sleep(0.05)
        raise RuntimeError("receiver reset timed out")


class ReceiverHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if urllib.parse.urlparse(self.path).path == "/command":
            with self.server.lock:
                body = json.dumps(self.server.command).encode()
            content_type = "application/json"
        else:
            body = (FIXTURES / "receiver.html").read_bytes()
            content_type = "text/html; charset=utf-8"
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        value = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        with self.server.lock:
            if value["sequence"] > self.server.receipt.get("sequence", -1):
                self.server.receipt = value
        self.send_response(204)
        self.end_headers()

    def log_message(self, *_args):
        pass


def frontmost(focus):
    return int(subprocess.check_output([str(focus)], text=True).strip())


def run_receiver(kind, args, focus):
    output = args.output / kind
    output.mkdir()
    profile = pathlib.Path(tempfile.mkdtemp(prefix=f"auv-keyboard-{kind}-"))
    server = ReceiverServer()
    threading.Thread(target=server.serve_forever, daemon=True).start()
    previous_app = frontmost(focus)
    previous_source = subprocess.check_output([str(focus), "source"], text=True).strip()
    title = f"AUV keyboard {kind} {profile.name}"
    url = f"http://127.0.0.1:{server.server_port}/?" + urllib.parse.urlencode({"title": title})
    if kind == "chrome":
        command = [str(args.chrome), f"--user-data-dir={profile}", "--no-first-run", "--no-default-browser-check",
                   "--disable-background-networking", "--disable-sync", f"--app={url}"]
    else:
        command = [str(args.electron), str(FIXTURES / "electron.cjs"), url, str(profile), str(output)]
    results = []
    process = None
    producer = None
    try:
        with (output / "process.log").open("w") as log:
            process = subprocess.Popen(command, stdout=log, stderr=log)
        deadline = time.monotonic() + 20
        window = None
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f"receiver exited: see {output / 'process.log'}")
            if server.snapshot():
                window = next((item for item in objects(invoke(args.auv, output, ["window.list"]))
                               if item.get("title") == title and item.get("reference")), None)
                if window:
                    break
            time.sleep(0.1)
        if not window:
            raise RuntimeError("isolated receiver window not found")
        assert window["process_id"] == process.pid, "must address only this test's process"
        foreground_pid = previous_app if args.mode == "background" else process.pid
        subprocess.run([str(focus), str(foreground_pid)], capture_output=True, check=True)
        subprocess.run([str(focus), "source", args.input_source], capture_output=True, check=True)
        time.sleep(0.5)
        target = "window:" + window["reference"]["id"]
        if args.sender:
            with (output / "sender.log").open("w") as log:
                producer = subprocess.Popen([str(args.sender)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                            stderr=log, text=True, bufsize=1)
        for repetition in range(args.repetitions):
            for name, initial, commands, expected, input_count, key_count in args.cases:
                case_id = f"{name}-{repetition + 1}"
                result = {"case": name, "repetition": repetition + 1, "expected": expected, "commands": commands,
                          "started_ms": time.time_ns() // 1_000_000}
                try:
                    # Re-establish fixture preconditions when a person switched
                    # applications between cases; focus during input is still asserted.
                    current = frontmost(focus)
                    if (args.mode == "background" and current == process.pid) or \
                            (args.mode == "foreground" and current != process.pid):
                        subprocess.run([str(focus), str(foreground_pid)], capture_output=True, check=True)
                        time.sleep(0.2)
                    server.reset(case_id, initial)
                    source = subprocess.check_output([str(focus), "source"], text=True).strip()
                    assert source == args.input_source, "keyboard input source changed during test"
                    before = frontmost(focus)
                    assert (before != process.pid if args.mode == "background" else before == process.pid), \
                        "receiver is in the wrong foreground/background state before input"
                    policy = "background-only" if args.mode == "background" else "foreground-preferred"
                    responses = []
                    intermediate = []
                    for command in commands:
                        if command[0] == "wait":
                            time.sleep(int(command[1]) / 1000)
                            intermediate.append(server.snapshot())
                        elif producer:
                            responses.append(send(producer, command, window["reference"]["id"], args.mode))
                        else:
                            responses.append(invoke(args.auv, output, [*command, "--target", target,
                                                                      "--input-policy", policy]))
                    # This wait observes asynchronous receipt, including late duplicates.
                    # It does not insert a delay inside the driver's native key sequence.
                    time.sleep(0.8)
                    receipt = server.snapshot()
                    after = frontmost(focus)
                    events = receipt["events"]
                    base_keys = [event for event in events if event.get("key") not in
                                 ["Shift", "Control", "Alt", "Meta", "CapsLock"]]
                    inputs = [event for event in events if event["type"] == "input"]
                    downs = [event for event in base_keys if event["type"] == "keydown"]
                    ups = [event for event in base_keys if event["type"] == "keyup"]
                    checks = {
                        "text": receipt["value"] == expected,
                        "input_count": len(inputs) == input_count,
                        "key_pairs": key_count is None or len(downs) == len(ups) == key_count,
                        "focus": before == after
                                 and all(event["documentFocused"] == (args.mode == "foreground") for event in events),
                        "trusted_events": bool(events) and all(event["trusted"] for event in events),
                        "submission_unverified": all(response["result"].get("verified") is False
                                                     for response in responses if response.get("result") is not None),
                    }
                    if name in ["hold_b", "hold_shift_b", "split_b", "timeout_b", "drop_b"]:
                        checks["hold_duration"] = len(downs) == len(ups) == 1 and \
                            150 <= ups[0]["receivedMs"] - downs[0]["receivedMs"] <= 1500
                    if name in ["split_b", "drop_b", "held_shift_press_b"]:
                        held_events = intermediate[0]["events"]
                        checks["down_before_release"] = any(e["type"] == "keydown" for e in held_events) and \
                            not any(e["type"] == "keyup" for e in held_events)
                    if name == "timeout_b":
                        checks["timeout_released"] = any(e["type"] == "keyup" for e in intermediate[0]["events"])
                    result.update(receipt=receipt, checks=checks, driver_results=responses,
                                  intermediate=intermediate,
                                  foreground_before=before, foreground_after=after,
                                  input_source=source,
                                  passed=all(checks.values()))
                except Exception as error:
                    result.update(passed=False, error=str(error), receipt=server.snapshot())
                finally:
                    if producer:
                        send(producer, ["drop"], window["reference"]["id"], args.mode)
                result["finished_ms"] = time.time_ns() // 1_000_000
                results.append(result)
                (output / "results.json").write_text(json.dumps(results, ensure_ascii=False, indent=2) + "\n")
                print(kind, case_id, "PASS" if result["passed"] else "FAIL",
                      repr(result.get("receipt", {}).get("value")), flush=True)
        return results
    finally:
        if producer is not None:
            producer.stdin.close()
            try:
                producer.wait(timeout=5)
            except subprocess.TimeoutExpired:
                producer.kill()
                producer.wait()
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        server.shutdown()
        server.server_close()
        subprocess.run([str(focus), str(previous_app)], capture_output=True)
        subprocess.run([str(focus), "source", previous_source], capture_output=True, check=True)
        shutil.rmtree(profile, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--auv", type=pathlib.Path, default=REPOSITORY / "target/debug/auv")
    parser.add_argument("--chrome", type=pathlib.Path,
                        default=pathlib.Path("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"))
    parser.add_argument("--electron", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--repetitions", type=int, default=5)
    parser.add_argument("--sender", type=pathlib.Path,
                        help="persistent Rust sender; selects the held-key suite")
    parser.add_argument("--mode", choices=["background", "foreground"], default="background",
                        help="foreground is an explicit control run; it activates only the fixture")
    parser.add_argument("--input-source", default="com.apple.keylayout.ABC",
                        help="temporary keyboard input source; the previous source is restored")
    args = parser.parse_args()
    args.cases = HOLD_CASES if args.sender else CASES
    if args.repetitions < 1:
        parser.error("repetitions must be positive")
    for executable in [args.auv, args.chrome, args.electron] + ([args.sender] if args.sender else []):
        if not executable.is_file():
            parser.error(f"executable not found: {executable}")
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    focus = args.output / "focus"
    subprocess.run(["swiftc", str(FIXTURES / "focus.swift"), "-o", str(focus)], check=True)
    with args.chrome.parents[1].joinpath("Info.plist").open("rb") as file:
        chrome_version = plistlib.load(file)["CFBundleShortVersionString"]
    summary = {"platform": subprocess.check_output(["sw_vers"], text=True), "chrome_version": chrome_version,
               "mode": args.mode, "input_source": args.input_source,
               "suite": "held_keys" if args.sender else "cli_keyboard",
               "repetitions": args.repetitions, "results": {}}
    for kind in ["chrome", "electron"]:
        results = run_receiver(kind, args, focus)
        summary["results"][kind] = {
            name: {"passed": sum(result["passed"] for result in results if result["case"] == name),
                   "total": args.repetitions} for name, *_ in args.cases
        }
    summary["electron_versions"] = json.loads((args.output / "electron/electron-versions.json").read_text())
    (args.output / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(summary, ensure_ascii=False, indent=2), flush=True)
    return 0 if all(case["passed"] == case["total"] for cases in summary["results"].values() for case in cases.values()) else 1


if __name__ == "__main__":
    raise SystemExit(main())
