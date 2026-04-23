from __future__ import annotations

from sagens import CheckpointRestoreMode

from .support import smoke_server


def test_websocket_supports_checkpoint_lineage_flow() -> None:
    with smoke_server() as daemon:
        box = daemon.create_box()
        box.start()

        box.fs.write("/workspace/notes.txt", b"hello checkpoint")
        seed = box.checkpoint.create("seed", {"purpose": "test"})
        assert seed.summary.workspace_id == str(box.box_id)
        assert seed.summary.name == "seed"
        assert seed.source_checkpoint_id is None
        assert seed.summary.metadata["purpose"] == "test"

        box.fs.write("/workspace/notes.txt", b"second version")
        second = box.checkpoint.create("second")
        assert second.source_checkpoint_id == seed.summary.checkpoint_id

        checkpoints = box.checkpoint.list()
        assert len(checkpoints) == 2
        assert checkpoints[0].source_checkpoint_id is None
        assert checkpoints[1].source_checkpoint_id == seed.summary.checkpoint_id

        restored = box.checkpoint.restore(
            seed.summary.checkpoint_id,
            CheckpointRestoreMode.ROLLBACK,
        )
        assert restored.summary.checkpoint_id == seed.summary.checkpoint_id
        assert box.fs.read("/workspace/notes.txt").data == b"hello checkpoint"

        box.fs.write("/workspace/notes.txt", b"after restore")
        after_restore = box.checkpoint.create("after-restore")
        assert after_restore.source_checkpoint_id == seed.summary.checkpoint_id

        box.fs.write("/workspace/notes.txt", b"source must stay mutated")
        forked = box.checkpoint.fork(seed.summary.checkpoint_id, "forked-box")
        assert forked.name == "forked-box"
        assert forked.box_id != box.box_id

        box.stop()
        box.start()
        assert box.fs.read("/workspace/notes.txt").data == b"source must stay mutated"

        forked_box = daemon.get_box(forked.box_id)
        forked_box.start()
        assert forked_box.fs.read("/workspace/notes.txt").data == b"hello checkpoint"

        forked_box.fs.write("/workspace/notes.txt", b"forked version")
        forked_head = forked_box.checkpoint.create("forked-head")
        assert forked_head.source_checkpoint_id is None

        box.checkpoint.delete(after_restore.summary.checkpoint_id)
        box.checkpoint.delete(second.summary.checkpoint_id)
        box.checkpoint.delete(seed.summary.checkpoint_id)
        assert box.checkpoint.list() == []

        forked_box.stop()
        forked_box.remove()
        box.remove()
