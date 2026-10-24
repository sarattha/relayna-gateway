"""Focused harness checks; no Gateway, network, credentials or disk writes."""
import asyncio
from collections import Counter
from types import SimpleNamespace
import unittest
from unittest.mock import AsyncMock, patch

import soak


class SocketRenewalTests(unittest.IsolatedAsyncioTestCase):
    def subject(self):
        value = soak.Soak.__new__(soak.Soak)
        value.args = SimpleNamespace(proxy=["http://127.0.0.1:1", "http://127.0.0.1:2"])
        value.run_id, value.prefix, value.key = "fixture", "/services/fixture", "fixture-key"
        value.secrets = [value.key]
        value.sample_lock = asyncio.Lock()
        value.socket_ids = ["old-0", "old-1"]
        value.instances = ["process-0", "process-1"]
        value.progress, value.counts = [8, 9], [Counter(), Counter()]
        value._sample = AsyncMock()

        async def terminal(request_id):
            return {"request_id": request_id, "client_status": 101,
                    "completed_at": "2026-10-25T00:00:00Z",
                    "diagnostics": {"instance_id": "process-" + request_id[-1],
                                    "outcome": "completed", "websocket": {"client_frames": 8}}}
        value.terminal = AsyncMock(side_effect=terminal)
        return value

    async def test_renewal_preserves_retired_evidence_and_serializes_sampling(self):
        value = self.subject()
        retired, sockets = [], [object(), object()]
        original = list(sockets)
        replacements = [object(), object()]
        terminal_entered, release_terminal = asyncio.Event(), asyncio.Event()
        original_terminal = value.terminal.side_effect

        async def terminal(request_id):
            if request_id == "old-0":
                terminal_entered.set()
                await release_terminal.wait()
            return await original_terminal(request_id)

        value.terminal.side_effect = terminal
        with patch.object(soak, "websocket", AsyncMock(side_effect=replacements)) as opened, \
                patch.object(soak, "probe", AsyncMock()) as probed, \
                patch.object(soak, "close_socket", AsyncMock()) as closed:
            renewal = asyncio.create_task(value.renew_sockets(sockets, retired))
            await asyncio.wait_for(terminal_entered.wait(), 1)
            sampling = asyncio.create_task(value.sample("load"))
            await asyncio.sleep(0)
            value._sample.assert_not_awaited()
            self.assertFalse(sampling.done(), "sampling must not inspect a half-swapped socket pair")
            release_terminal.set()
            await asyncio.wait_for(asyncio.gather(renewal, sampling), 5)
            self.assertEqual(opened.await_count, 2)
            self.assertEqual(probed.await_count, 2)
            self.assertEqual([call.args[0] for call in closed.await_args_list], original)
        self.assertEqual(sockets, replacements)
        self.assertEqual([row["request_id"] for row in retired], ["old-0", "old-1"])
        self.assertTrue(all(row["kind"] == "websocket_renewed" for row in retired))
        self.assertEqual(value.socket_ids, ["soak-fixture-renewed-0-1", "soak-fixture-renewed-1-1"])
        self.assertEqual(value.progress, [0, 0])
        self.assertEqual([row["socket_renewals"] for row in value.counts], [1, 1])
        self.assertEqual(value._sample.await_count, 2)

    async def test_invalid_terminal_closes_replacement_and_does_not_hide_failure(self):
        value = self.subject()
        value.terminal = AsyncMock(return_value={"completed_at": None, "diagnostics": {}})
        sockets, replacement = [object(), object()], object()
        original = list(sockets)
        retired = []
        with patch.object(soak, "websocket", AsyncMock(return_value=replacement)), \
                patch.object(soak, "probe", AsyncMock()), \
                patch.object(soak, "close_socket", AsyncMock()) as closed:
            with self.assertRaisesRegex(soak.ProtocolError, "renewed_socket_terminal_invalid"):
                await value.renew_sockets(sockets, retired)
            self.assertEqual([call.args[0] for call in closed.await_args_list], [original[0], replacement])
        self.assertEqual(sockets, original)
        self.assertEqual(retired, [])
        self.assertEqual(value.counts[0]["socket_renewals"], 0)
        value._sample.assert_not_awaited()

    async def test_cancelled_probe_closes_new_socket_without_retiring_old_one(self):
        value = self.subject()
        sockets, replacement = [object(), object()], object()
        with patch.object(soak, "websocket", AsyncMock(return_value=replacement)), \
                patch.object(soak, "probe", AsyncMock(side_effect=asyncio.CancelledError)), \
                patch.object(soak, "close_socket", AsyncMock()) as closed:
            with self.assertRaises(asyncio.CancelledError):
                await value.renew_sockets(sockets, [])
            closed.assert_awaited_once_with(replacement)
        value.terminal.assert_not_awaited()


if __name__ == "__main__":
    unittest.main()
