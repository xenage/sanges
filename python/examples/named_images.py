from __future__ import annotations

import os
from pathlib import Path

from sagens import Box, Daemon, VmImageManifest

IMAGE_NAME = "chromium"
STATE_DIR = Path(os.environ.get("SAGENS_EXAMPLE_STATE_DIR", ".sagens-python-named-images")).resolve()
CHROMIUM_SMOKE = (
    "chromium --headless --no-sandbox --disable-gpu "
    "--dump-dom 'data:text/html,<h1>ok</h1>'"
)


def smoke_chromium(box: Box) -> None:
    box.set("memory_mb", 512)
    box.set("max_processes", 512)
    box.start()
    result = box.exec_bash(CHROMIUM_SMOKE)
    box.stop()
    if b"<h1>ok</h1>" not in result.stdout:
        raise RuntimeError(f"chromium smoke failed in {box.box_id}: {result.stderr.decode()}")


def ensure_chromium_image(daemon: Daemon) -> VmImageManifest:
    for image in daemon.list_images():
        if image.name == IMAGE_NAME:
            return image
    return daemon.build_image(
        IMAGE_NAME,
        apk=["chromium"],
        min_image_mib=1024,
    )


def main() -> None:
    with Daemon.start(state_dir=STATE_DIR) as daemon:
        image = ensure_chromium_image(daemon)
        first = daemon.create_box(image=image.name)
        second = daemon.create_box(image=image.name)
        smoke_chromium(first)
        smoke_chromium(second)
    print(f"created two BOXes from named image '{IMAGE_NAME}'")


if __name__ == "__main__":
    main()
