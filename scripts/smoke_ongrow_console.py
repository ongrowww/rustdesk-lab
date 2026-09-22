#!/usr/bin/env python3
"""Require a rendered, visible Console window, not just a living process."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

READY = b"ONGROW_CONSOLE_UI_READY"
ERRORS = (
    b"LateInitializationError",
    b"Unhandled Exception",
    b"EXCEPTION CAUGHT BY",
    b"MissingPluginException",
)


def smoke(command, timeout=30, settle=2):
    with tempfile.TemporaryFile() as output:
        process = subprocess.Popen(
            command,
            stdout=output,
            stderr=subprocess.STDOUT,
            env=dict(os.environ, NSUnbufferedIO="YES", RUST_BACKTRACE="1"),
        )
        try:
            deadline = time.monotonic() + timeout
            ready_at = None
            while time.monotonic() < deadline:
                log = os.pread(output.fileno(), os.fstat(output.fileno()).st_size, 0)
                if any(error in log for error in ERRORS):
                    raise RuntimeError("Console reported a Flutter runtime error")
                if process.poll() is not None:
                    raise RuntimeError("Console exited before the UI check completed")
                if READY in log:
                    if ready_at is None:
                        ready_at = time.monotonic()
                    if time.monotonic() - ready_at >= settle:
                        return
                time.sleep(0.1)
            raise RuntimeError("Console did not confirm a rendered, visible window")
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


if __name__ == "__main__":
    executable = Path(sys.argv[1]).resolve(strict=True)
    try:
        smoke([str(executable)])
    except RuntimeError as error:
        # App logs can contain credentials or device data; never dump them.
        sys.exit(str(error))
    print("Console rendered a visible window and remained running")
