from __future__ import annotations

import base64
import hashlib
import os
import socket
import ssl
import struct
from dataclasses import dataclass
from urllib.parse import urlparse

from ._errors import SagensError

_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
_BUFFER_SIZE = 64 * 1024


@dataclass(frozen=True)
class _Endpoint:
    host: str
    port: int
    path: str
    secure: bool


class _WebSocketConnection:
    def __init__(self, endpoint: str) -> None:
        self._socket = _connect(endpoint)

    def close(self) -> None:
        try:
            self.send_close()
        except Exception:
            pass
        self._socket.close()

    def recv_text(self) -> str:
        payload = self._recv_message()
        return payload.decode()

    def send_text(self, payload: str) -> None:
        self._send_frame(0x1, payload.encode())

    def send_close(self) -> None:
        self._send_frame(0x8, b"")

    def send_pong(self, payload: bytes) -> None:
        self._send_frame(0xA, payload)

    def _recv_message(self) -> bytes:
        chunks = bytearray()
        opcode = None
        while True:
            frame = self._recv_frame()
            if frame.opcode == 0x8:
                raise SagensError("websocket connection closed")
            if frame.opcode == 0x9:
                self.send_pong(frame.payload)
                continue
            if frame.opcode == 0xA:
                continue
            if frame.opcode not in {0x0, 0x1}:
                raise SagensError(f"unsupported websocket opcode {frame.opcode}")
            opcode = frame.opcode if opcode is None else opcode
            chunks.extend(frame.payload)
            if frame.fin:
                if opcode != 0x1:
                    raise SagensError("expected text websocket message")
                return bytes(chunks)

    def _recv_frame(self) -> "_Frame":
        header = _recv_exact(self._socket, 2)
        byte1, byte2 = header
        fin = bool(byte1 & 0x80)
        opcode = byte1 & 0x0F
        masked = bool(byte2 & 0x80)
        length = byte2 & 0x7F
        if length == 126:
            length = struct.unpack("!H", _recv_exact(self._socket, 2))[0]
        elif length == 127:
            length = struct.unpack("!Q", _recv_exact(self._socket, 8))[0]
        mask = _recv_exact(self._socket, 4) if masked else b""
        payload = _recv_exact(self._socket, length)
        if masked:
            payload = bytes(value ^ mask[index % 4] for index, value in enumerate(payload))
        return _Frame(fin=fin, opcode=opcode, payload=payload)

    def _send_frame(self, opcode: int, payload: bytes) -> None:
        header = bytearray([0x80 | opcode])
        length = len(payload)
        if length < 126:
            header.append(0x80 | length)
        elif length <= 0xFFFF:
            header.append(0x80 | 126)
            header.extend(struct.pack("!H", length))
        else:
            header.append(0x80 | 127)
            header.extend(struct.pack("!Q", length))
        mask = os.urandom(4)
        masked = bytes(value ^ mask[index % 4] for index, value in enumerate(payload))
        self._socket.sendall(bytes(header) + mask + masked)


@dataclass(frozen=True)
class _Frame:
    fin: bool
    opcode: int
    payload: bytes


def _connect(endpoint: str) -> socket.socket:
    parsed = _parse_endpoint(endpoint)
    raw_socket = socket.create_connection((parsed.host, parsed.port))
    if parsed.secure:
        context = ssl.create_default_context()
        raw_socket = context.wrap_socket(raw_socket, server_hostname=parsed.host)
    _perform_handshake(raw_socket, parsed)
    return raw_socket


def _parse_endpoint(endpoint: str) -> _Endpoint:
    parsed = urlparse(endpoint)
    if parsed.scheme not in {"ws", "wss"}:
        raise SagensError(f"unsupported websocket scheme: {parsed.scheme}")
    host = parsed.hostname
    if not host:
        raise SagensError(f"invalid websocket endpoint: {endpoint}")
    secure = parsed.scheme == "wss"
    port = parsed.port or (443 if secure else 80)
    path = parsed.path or "/"
    if parsed.query:
        path = f"{path}?{parsed.query}"
    return _Endpoint(host=host, port=port, path=path, secure=secure)


def _perform_handshake(sock: socket.socket, endpoint: _Endpoint) -> None:
    key = base64.b64encode(os.urandom(16)).decode()
    request = (
        f"GET {endpoint.path} HTTP/1.1\r\n"
        f"Host: {endpoint.host}:{endpoint.port}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        "Sec-WebSocket-Version: 13\r\n\r\n"
    )
    sock.sendall(request.encode())
    status_line, headers = _read_http_headers(sock)
    if " 101 " not in status_line:
        raise SagensError(f"websocket handshake failed: {status_line.strip()}")
    expected = base64.b64encode(hashlib.sha1(f"{key}{_GUID}".encode()).digest()).decode()
    if headers.get("sec-websocket-accept") != expected:
        raise SagensError("websocket handshake returned invalid accept key")


def _read_http_headers(sock: socket.socket) -> tuple[str, dict[str, str]]:
    data = bytearray()
    while b"\r\n\r\n" not in data:
        chunk = sock.recv(_BUFFER_SIZE)
        if not chunk:
            raise SagensError("websocket handshake connection closed")
        data.extend(chunk)
    header_bytes, _, _ = data.partition(b"\r\n\r\n")
    lines = header_bytes.decode().split("\r\n")
    status_line = lines[0]
    headers: dict[str, str] = {}
    for line in lines[1:]:
        if not line:
            continue
        name, _, value = line.partition(":")
        headers[name.strip().lower()] = value.strip()
    return status_line, headers


def _recv_exact(sock: socket.socket, size: int) -> bytes:
    remaining = size
    chunks = bytearray()
    while remaining > 0:
        chunk = sock.recv(remaining)
        if not chunk:
            raise SagensError("websocket connection closed")
        chunks.extend(chunk)
        remaining -= len(chunk)
    return bytes(chunks)
