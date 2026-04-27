from __future__ import annotations

from datetime import UTC, datetime
from tempfile import TemporaryDirectory

from sagens import Daemon


def log(message: str) -> None:
    stamp = datetime.now(UTC).strftime("%H:%M:%S")
    print(f"[{stamp}] {message}", flush=True)


def main() -> None:
    log("starting python standalone test")
    with TemporaryDirectory(prefix="sagens-test-") as state_dir:
        log(f"created temporary state dir: {state_dir}")
        with Daemon.start(state_dir=state_dir) as daemon:
            log(f"daemon started endpoint={daemon.client.endpoint}")
            boxes = []
            for index in range(10):
                log(f"creating box#{index}")
                box = daemon.create_box()
                boxes.append(box)
                log(
                    "created "
                    f"box#{index} id={box.box_id} status={box.record.status.value}"
                )

            for index, box in enumerate(boxes):
                log(f"starting box#{index} id={box.box_id}")
                box.start()
                log(f"started box#{index} status={box.record.status.value}")
                payload = f"box-{index}\n".encode()
                log(
                    f"writing /workspace/message.txt for box#{index} "
                    f"bytes={len(payload)}"
                )
                box.fs.write("/workspace/message.txt", payload)

            for index, box in enumerate(boxes):
                log(f"verifying commands in box#{index} id={box.box_id}")
                shell = box.exec_bash("cat /workspace/message.txt && uname -s")
                python = box.exec_python(["-c", "print('hello from python')"])
                shell_output = shell.stdout.decode().strip()
                python_output = python.stdout.decode().strip()
                expected_prefix = f"box-{index}"
                if not shell_output.startswith(expected_prefix):
                    raise RuntimeError(
                        f"unexpected shell output for box#{index}: {shell_output!r}"
                    )
                if python_output != "hello from python":
                    raise RuntimeError(
                        f"unexpected python output for box#{index}: {python_output!r}"
                    )
                log(
                    f"verified box#{index} "
                    f"shell_exit={shell.exit_status.kind} "
                    f"python_exit={python.exit_status.kind}"
                )
                print(index, shell_output, python_output, flush=True)

            for index, box in enumerate(boxes):
                log(f"stopping box#{index} id={box.box_id}")
                box.stop()
                log(f"stopped box#{index} status={box.record.status.value}")
    log("python standalone test finished successfully")


if __name__ == "__main__":
    main()
