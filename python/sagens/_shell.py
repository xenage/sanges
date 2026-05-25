from __future__ import annotations

import base64
import queue
from dataclasses import dataclass
from typing import Iterator, cast

from ._transport import _Transport
from ._wire import QueueItem, WireObject


@dataclass(frozen=True)
class ShellOutputEvent:
    data: bytes


@dataclass(frozen=True)
class ShellExitEvent:
    code: int


class BoxShell:
    def __init__(self, transport: _Transport, shell_id: str, events: queue.Queue[QueueItem]) -> None:
        self.shell_id = shell_id
        self._transport = transport
        self._events = events

    def send_input(self, data: bytes | str) -> None:
        payload = data.encode() if isinstance(data, str) else data
        self._transport.send_shell_request(
            {
                "type": "shell_input",
                "request_id": self._transport.next_request_id(),
                "shell_id": self.shell_id,
                "data": base64.b64encode(payload).decode(),
            }
        )

    def resize(self, cols: int, rows: int) -> None:
        self._transport.send_shell_request(
            {
                "type": "resize_shell",
                "request_id": self._transport.next_request_id(),
                "shell_id": self.shell_id,
                "cols": cols,
                "rows": rows,
            }
        )

    def close(self) -> None:
        self._transport.send_shell_request(
            {
                "type": "close_shell",
                "request_id": self._transport.next_request_id(),
                "shell_id": self.shell_id,
            }
        )

    def next_event(self) -> ShellOutputEvent | ShellExitEvent:
        item = self._events.get()
        if isinstance(item, BaseException):
            raise item
        event = cast(WireObject, item)
        if event["type"] == "shell_output":
            return ShellOutputEvent(data=base64.b64decode(cast(str, event["data"])))
        return ShellExitEvent(code=cast(int, event["code"]))

    def iter_events(self) -> Iterator[ShellOutputEvent | ShellExitEvent]:
        while True:
            event = self.next_event()
            yield event
            if isinstance(event, ShellExitEvent):
                return
