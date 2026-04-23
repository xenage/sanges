from __future__ import annotations

import json
import queue
import threading
from collections import defaultdict
from itertools import count
from typing import Any

from ._errors import SagensError
from ._websocket import _WebSocketConnection


class _Transport:
    def __init__(self, endpoint: str, auth_message: dict[str, Any], principal: dict[str, str]) -> None:
        self.endpoint = endpoint
        self._conn = _WebSocketConnection(endpoint)
        self._send_lock = threading.Lock()
        self._state_lock = threading.Lock()
        self._closed = False
        self._next_id = count(1)
        self._pending_responses: dict[str, queue.Queue[Any]] = {}
        self._exec_streams: dict[str, queue.Queue[Any]] = {}
        self._shell_streams: dict[str, queue.Queue[Any]] = {}
        self._buffered_shell_events: dict[str, list[Any]] = defaultdict(list)
        self._authenticate(auth_message, principal)
        self._reader = threading.Thread(target=self._reader_loop, daemon=True)
        self._reader.start()

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        try:
            self._conn.close()
        finally:
            self._fail_all("websocket connection closed")
            self._reader.join(timeout=1)

    def next_request_id(self) -> str:
        return str(next(self._next_id))

    def request_response(self, request: dict[str, Any], expected_type: str) -> dict[str, Any]:
        request_id = request["request_id"]
        response_queue: queue.Queue[Any] = queue.Queue(maxsize=1)
        with self._state_lock:
            self._pending_responses[request_id] = response_queue
        try:
            self._send_json({"type": "request", "request": request})
        except Exception:
            with self._state_lock:
                self._pending_responses.pop(request_id, None)
            raise
        response = self._expect_queue_item(response_queue)
        if response["type"] != expected_type:
            raise SagensError(
                f"unexpected response type {response['type']} for request {request_id}"
            )
        return response

    def open_exec_stream(self, request: dict[str, Any]) -> tuple[str, queue.Queue[Any]]:
        request_id = request["request_id"]
        event_queue: queue.Queue[Any] = queue.Queue()
        with self._state_lock:
            self._exec_streams[request_id] = event_queue
        try:
            self._send_json({"type": "request", "request": request})
        except Exception:
            with self._state_lock:
                self._exec_streams.pop(request_id, None)
            raise
        return request_id, event_queue

    def register_shell(self, shell_id: str) -> queue.Queue[Any]:
        event_queue: queue.Queue[Any] = queue.Queue()
        with self._state_lock:
            self._shell_streams[shell_id] = event_queue
            buffered = self._buffered_shell_events.pop(shell_id, [])
        for event in buffered:
            event_queue.put_nowait(event)
        return event_queue

    def send_shell_request(self, request: dict[str, Any]) -> None:
        self._send_json({"type": "request", "request": request})

    def _authenticate(self, auth_message: dict[str, Any], principal: dict[str, str]) -> None:
        self._send_json(auth_message)
        payload = json.loads(self._conn.recv_text())
        if payload.get("type") != "authenticated":
            raise SagensError(f"unexpected auth message: {payload}")
        if payload.get("principal") != principal:
            raise SagensError(
                f"unexpected auth principal {payload.get('principal')}; expected {principal}"
            )

    def _send_json(self, payload: dict[str, Any]) -> None:
        with self._send_lock:
            self._conn.send_text(json.dumps(payload))

    def _expect_queue_item(self, event_queue: queue.Queue[Any]) -> Any:
        item = event_queue.get()
        if isinstance(item, Exception):
            raise item
        return item

    def _reader_loop(self) -> None:
        try:
            while not self._closed:
                payload = json.loads(self._conn.recv_text())
                if payload.get("type") == "event":
                    self._dispatch_event(payload["event"])
        except Exception as error:
            self._fail_all(str(error))
        else:
            self._fail_all("websocket connection closed")

    def _dispatch_event(self, event: dict[str, Any]) -> None:
        kind = event["type"]
        if kind == "response":
            self._resolve_response(event["request_id"], event["response"])
            return
        if kind in {"exec_output", "exec_exit"}:
            self._push_exec_event(event)
            return
        if kind in {"shell_output", "shell_exit"}:
            self._push_shell_event(event)
            return
        if kind == "error":
            self._resolve_error(event.get("request_id"), event["message"])

    def _resolve_response(self, request_id: str, response: dict[str, Any]) -> None:
        with self._state_lock:
            response_queue = self._pending_responses.pop(request_id, None)
        if response_queue is not None:
            response_queue.put_nowait(response)

    def _push_exec_event(self, event: dict[str, Any]) -> None:
        with self._state_lock:
            event_queue = self._exec_streams.get(event["request_id"])
            if event["type"] == "exec_exit":
                self._exec_streams.pop(event["request_id"], None)
        if event_queue is not None:
            event_queue.put_nowait(event)

    def _push_shell_event(self, event: dict[str, Any]) -> None:
        shell_id = event["shell_id"]
        with self._state_lock:
            event_queue = self._shell_streams.get(shell_id)
            if event_queue is None:
                self._buffered_shell_events[shell_id].append(event)
                return
            if event["type"] == "shell_exit":
                self._shell_streams.pop(shell_id, None)
        event_queue.put_nowait(event)

    def _resolve_error(self, request_id: str | None, message: str) -> None:
        error = SagensError(message)
        if request_id is None:
            self._fail_all(message)
            return
        with self._state_lock:
            response_queue = self._pending_responses.pop(request_id, None)
            event_queue = self._exec_streams.pop(request_id, None)
        if response_queue is not None:
            response_queue.put_nowait(error)
            return
        if event_queue is not None:
            event_queue.put_nowait(error)
            return
        self._fail_all(message)

    def _fail_all(self, message: str) -> None:
        error = SagensError(message)
        with self._state_lock:
            responses = list(self._pending_responses.values())
            exec_streams = list(self._exec_streams.values())
            shell_streams = list(self._shell_streams.values())
            self._pending_responses.clear()
            self._exec_streams.clear()
            self._shell_streams.clear()
            self._buffered_shell_events.clear()
        for event_queue in [*responses, *exec_streams, *shell_streams]:
            try:
                event_queue.put_nowait(error)
            except Exception:
                pass
