"""Runs pinned upstream OSWorld setup and evaluation against a forwarded VM.

Reads one JSON request on stdin and writes one JSON result on stdout:

    {"command": "setup" | "evaluate", "task": {...}, "lastAction": "DONE",
     "ports": {"server": 15000, "chromium": 19222, "vlc": 18080},
     "cacheDir": "/abs/path"}

The VM lifecycle stays in the Node.js episode runner. This bridge only builds
the parts of `DesktopEnv` that setup and `evaluate` read, so every upstream
setup step, getter, and metric keeps its original Python semantics.
"""

import json
import logging
import os
import sys


def main() -> None:
    request = json.load(sys.stdin)
    # NOTICE: upstream dependencies print to stdout (for example PyMuPDF's
    # `fitz` deprecation notice), so stdout carries only the final result.
    result_stream, sys.stdout = sys.stdout, sys.stderr
    # NOTICE: upstream is a source checkout, not an installable package with a
    # small dependency set. `scripts/sync-osworld` pins it; this path import
    # keeps its heavy model-agent dependencies out of the evaluator env.
    sys.path.insert(0, os.environ["OSWORLD_SOURCE"])
    logging.basicConfig(stream=sys.stderr, level=logging.INFO)

    from desktop_env.controllers.python import PythonController  # ty: ignore[unresolved-import]
    from desktop_env.controllers.setup import SetupController  # ty: ignore[unresolved-import]
    from desktop_env.desktop_env import DesktopEnv  # ty: ignore[unresolved-import]

    ports = request["ports"]
    env = DesktopEnv.__new__(DesktopEnv)
    env.vm_ip = "127.0.0.1"
    env.server_port, env.chromium_port, env.vlc_port = ports["server"], ports["chromium"], ports["vlc"]
    env.cache_dir_base = request["cacheDir"]
    env.client_password = "password"
    env.screen_width, env.screen_height = 1920, 1080
    env.enable_proxy = env.current_use_proxy = False
    env.is_environment_used = False
    env.os_type = "Ubuntu"
    env.action_history = []
    env._traj_no = env._step_no = 0
    env.controller = PythonController(vm_ip=env.vm_ip, server_port=env.server_port)
    env.setup_controller = SetupController(
        vm_ip=env.vm_ip,
        server_port=env.server_port,
        chromium_port=env.chromium_port,
        vlc_port=env.vlc_port,
        cache_dir=env.cache_dir_base,
        client_password=env.client_password,
        screen_width=env.screen_width,
        screen_height=env.screen_height,
    )
    # Mirrors the setup half of `DesktopEnv.reset`, which logs but never
    # raises after its retries fail; the caller needs the failure as a value.
    env._set_task_info(request["task"])
    env.setup_controller.reset_cache_dir(env.cache_dir)
    if request["command"] == "setup":
        result = {"ok": env.setup_controller.setup(env.config, False)}
    else:
        if request.get("lastAction"):
            env.action_history.append(request["lastAction"])
        result = {"score": float(env.evaluate())}
    json.dump(result, result_stream)


if __name__ == "__main__":
    main()
