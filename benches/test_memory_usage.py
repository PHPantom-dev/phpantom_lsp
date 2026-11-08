"""Exercise memory sampling waits without launching a server or sleeping."""

from collections import deque
import importlib.util
from pathlib import Path
import queue
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "memory_usage", Path(__file__).with_name("memory_usage.py")
)
memory = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(memory)


def progress(token, kind):
    return {
        "method": "$/progress",
        "params": {"token": token, "value": {"kind": kind}},
    }


class ScheduledNotifications:
    """Advance a fake clock when the client waits for a scheduled message."""

    def __init__(self, events):
        self.now = 0.0
        self.events = deque(events)

    def get(self, timeout):
        deadline = self.now + timeout
        if self.events and self.events[0][0] <= deadline:
            self.now, message = self.events.popleft()
            return message
        self.now = deadline
        raise queue.Empty


class IndexingWaitTests(unittest.TestCase):
    def client(self, events):
        client = object.__new__(memory.LspClient)
        notifications = ScheduledNotifications(events)
        client._notifications = notifications
        clock = mock.patch.object(memory.time, "monotonic", lambda: notifications.now)
        clock.start()
        self.addCleanup(clock.stop)
        return client, notifications

    def test_waits_across_consecutive_indexing_phases(self):
        client, notifications = self.client([
            (0.0, progress("indexing", "begin")),
            (0.1, progress("indexing", "end")),
            (0.2, progress("full-index", "begin")),
            (3.0, progress("full-index", "report")),
            (4.0, progress("full-index", "end")),
        ])

        client.wait_for_indexing(timeout=10)

        self.assertFalse(notifications.events)
        self.assertEqual(notifications.now, 6.0)

    def test_overlapping_tokens_must_all_finish(self):
        client, notifications = self.client([
            (0.0, progress("indexing", "begin")),
            (0.1, progress("full-index", "begin")),
            (0.2, progress("indexing", "end")),
            (4.0, progress("full-index", "end")),
        ])

        client.wait_for_indexing(timeout=10)

        self.assertFalse(notifications.events)
        self.assertEqual(notifications.now, 6.0)

    def test_trivial_workspace_finishes_after_a_quiet_period(self):
        client, notifications = self.client([])

        client.wait_for_indexing(timeout=10)

        self.assertEqual(notifications.now, 2.0)

    def test_unrelated_notifications_do_not_finish_active_indexing(self):
        client, notifications = self.client([
            (0.0, progress("indexing", "begin")),
            (0.1, {"method": "window/logMessage"}),
            (0.2, progress("unknown", "end")),
        ])

        with self.assertRaisesRegex(TimeoutError, "Indexing did not finish"):
            client.wait_for_indexing(timeout=5)

        self.assertEqual(notifications.now, 5.0)

    def test_notifications_cannot_extend_the_indexing_deadline(self):
        client, notifications = self.client([
            (0.0, progress("indexing", "begin")),
            (1.0, progress("indexing", "report")),
            (2.0, progress("indexing", "report")),
            (3.0, progress("indexing", "end")),
        ])

        with self.assertRaises(TimeoutError):
            client.wait_for_indexing(timeout=2)

        self.assertEqual(notifications.now, 2.0)
        self.assertEqual(len(notifications.events), 1)

    def test_memory_waits_for_the_slow_diagnostic_pass(self):
        uri = "file:///project/Plugin.php"
        client, notifications = self.client([
            (0.5, {"method": "textDocument/publishDiagnostics", "params": {"uri": uri}}),
            (8.0, {"method": "textDocument/publishDiagnostics", "params": {"uri": uri}}),
        ])
        client.wait_for_diagnostics(uri, timeout=10)
        self.assertEqual(notifications.now, 8.0)
        self.assertFalse(notifications.events)

    def test_unrelated_diagnostics_do_not_complete_the_open_file(self):
        uri = "file:///project/Plugin.php"
        client, notifications = self.client([
            (0.0, {"method": "textDocument/publishDiagnostics", "params": {"uri": uri}}),
            (0.5, {"method": "textDocument/publishDiagnostics", "params": {"uri": "file:///other.php"}}),
            (1.0, progress("workspace", "end")),
        ])
        with self.assertRaisesRegex(TimeoutError, "Diagnostics did not finish"):
            client.wait_for_diagnostics(uri, timeout=5)
        self.assertEqual(notifications.now, 5.0)

    def test_diagnostic_wait_has_a_fixed_deadline(self):
        uri = "file:///project/Plugin.php"
        client, notifications = self.client([
            (0.0, {"method": "textDocument/publishDiagnostics", "params": {"uri": uri}}),
            (10.0, {"method": "textDocument/publishDiagnostics", "params": {"uri": uri}}),
        ])
        with self.assertRaises(TimeoutError):
            client.wait_for_diagnostics(uri, timeout=2)
        self.assertEqual(notifications.now, 2.0)
        self.assertEqual(len(notifications.events), 1)


if __name__ == "__main__":
    unittest.main()
