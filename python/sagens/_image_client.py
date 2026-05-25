from __future__ import annotations

from collections.abc import Sequence
from typing import Protocol, cast

from ._decode import image_manifest_from_dict
from ._models import VmImageManifest
from ._wire import WireObject


class _ImageClientTransport(Protocol):
    def _request(self, request: WireObject, expected_type: str) -> WireObject: ...


class ImageClientMixin:
    def build_image(
        self: _ImageClientTransport,
        name: str,
        *,
        apk: Sequence[str] | None = None,
        pip: Sequence[str] | None = None,
        npm: Sequence[str] | None = None,
        min_image_mib: int = 512,
        force_refresh: bool = False,
    ) -> VmImageManifest:
        response = self._request(
            {
                "type": "image_build",
                "name": name,
                "apk": list(apk or []),
                "pip": list(pip or []),
                "npm": list(npm or []),
                "min_image_mib": min_image_mib,
                "force_refresh": force_refresh,
            },
            "image",
        )
        return image_manifest_from_dict(cast(dict, response["image"]))

    def list_images(self: _ImageClientTransport) -> list[VmImageManifest]:
        response = self._request({"type": "image_list"}, "image_list")
        images = cast(list[dict], response["images"])
        return [image_manifest_from_dict(image) for image in images]

    def inspect_image(self: _ImageClientTransport, name: str) -> VmImageManifest:
        response = self._request({"type": "image_inspect", "name": name}, "image")
        return image_manifest_from_dict(cast(dict, response["image"]))

    def remove_image(self: _ImageClientTransport, name: str) -> None:
        self._request({"type": "image_remove", "name": name}, "image_removed")
