import json
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "mlx"))
import benchmark_local_inference


class LocalInferenceBenchmarkMetadataTests(unittest.TestCase):
    def test_on_device_real_host_thermal_state_is_null_with_reason(self):
        """On real Apple Silicon macOS, standalone benchmark must report thermal_state as null."""
        thermal_meta = benchmark_local_inference.collect_thermal_metadata()
        host_meta = benchmark_local_inference.collect_host_metadata(thermal_metadata=thermal_meta)

        self.assertIsNone(host_meta["thermal_state"])
        self.assertIsNone(host_meta["thermal_source"])
        self.assertIsNotNone(host_meta["thermal_unavailable_reason"])
        self.assertIn("CLI", host_meta["thermal_unavailable_reason"])
        self.assertIn("NSProcessInfo", host_meta["thermal_unavailable_reason"])
        self.assertIn("D2", host_meta["thermal_unavailable_reason"])

        # Never 0, "0", nominal, normal, or boolean
        self.assertFalse(host_meta["thermal_state"] == 0)
        self.assertFalse(host_meta["thermal_state"] == "0")
        self.assertFalse(host_meta["thermal_state"] == "nominal")
        self.assertFalse(host_meta["thermal_state"] == "normal")

        # Diagnostics preserves pmset raw output
        self.assertIn("diagnostics", host_meta)
        self.assertIn("pmset_therm", host_meta["diagnostics"])

    def test_pmset_warning_note_does_not_become_nominal_or_zero(self):
        """Even with 'No thermal warning...' from pmset, thermal_state stays null."""
        sample_output = (
            "Note: No thermal warning level has been recorded\n"
            "Note: No performance warning level has been recorded\n"
            "Note: No CPU power status has been recorded"
        )
        meta = benchmark_local_inference.collect_thermal_metadata(pmset_raw=sample_output)

        self.assertIsNone(meta["thermal_state"])
        self.assertIsNone(meta["thermal_source"])
        self.assertIsNotNone(meta["thermal_unavailable_reason"])
        self.assertEqual(meta["pmset_therm"], sample_output)

    def test_pmset_cli_missing_does_not_fabricate_state(self):
        """When pmset CLI is missing (FileNotFoundError / None), thermal_state stays null."""
        with patch.object(benchmark_local_inference, "command_output", return_value=None):
            meta = benchmark_local_inference.collect_thermal_metadata()
            self.assertIsNone(meta["thermal_state"])
            self.assertIsNone(meta["thermal_source"])
            self.assertIsNotNone(meta["thermal_unavailable_reason"])
            self.assertIsNone(meta["pmset_therm"])

    def test_pmset_timeout_does_not_fabricate_state(self):
        """When pmset command times out, thermal_state stays null."""
        def mock_output(*args, **kwargs):
            if args and args[0] == "pmset":
                return None
            return "mocked"

        with patch.object(benchmark_local_inference, "command_output", side_effect=mock_output):
            meta = benchmark_local_inference.collect_thermal_metadata()
            self.assertIsNone(meta["thermal_state"])
            self.assertIsNone(meta["thermal_source"])
            self.assertIsNotNone(meta["thermal_unavailable_reason"])
            self.assertIsNone(meta["pmset_therm"])

    def test_pmset_empty_string_does_not_fabricate_state(self):
        """When pmset returns an empty string, thermal_state stays null."""
        meta = benchmark_local_inference.collect_thermal_metadata(pmset_raw="")
        self.assertIsNone(meta["thermal_state"])
        self.assertIsNone(meta["thermal_source"])
        self.assertIsNotNone(meta["thermal_unavailable_reason"])
        self.assertEqual(meta["pmset_therm"], "")

    def test_json_consumer_compatibility_and_unknown_display(self):
        """JSON serialization emits null, and consumer maps it to unknown/unavailable."""
        host_meta = benchmark_local_inference.collect_host_metadata(
            thermal_metadata=benchmark_local_inference.collect_thermal_metadata(
                pmset_raw="Note: No thermal warning level has been recorded"
            )
        )
        report = {
            "schema_version": 2,
            "host": host_meta,
            "diagnostics": {
                "pmset_therm": host_meta["diagnostics"]["pmset_therm"],
            },
        }

        serialized = json.dumps(report)
        deserialized = json.loads(serialized)

        self.assertIsNone(deserialized["host"]["thermal_state"])
        self.assertIsNone(deserialized["host"]["thermal_source"])
        self.assertIsNotNone(deserialized["host"]["thermal_unavailable_reason"])
        self.assertEqual(
            deserialized["diagnostics"]["pmset_therm"],
            "Note: No thermal warning level has been recorded",
        )

        def consumer_display(raw_state: str | None) -> str:
            if raw_state in ("nominal", "fair", "serious", "critical"):
                return raw_state
            return "unknown"

        self.assertEqual(consumer_display(deserialized["host"]["thermal_state"]), "unknown")

    def test_legacy_report_consumer_handling(self):
        """Legacy evidence containing multi-line pmset output in thermal_state is mapped to unknown."""
        legacy_host = {
            "thermal_state": (
                "Note: No thermal warning level has been recorded\n"
                "Note: No performance warning level has been recorded"
            ),
        }

        def consumer_display(raw_state: str | None) -> str:
            if raw_state in ("nominal", "fair", "serious", "critical"):
                return raw_state
            return "unknown"

        self.assertEqual(consumer_display(legacy_host["thermal_state"]), "unknown")

    def test_no_parameter_can_inject_a_thermal_state(self):
        """Review follow-up: a caller must not be able to record an arbitrary state as measured."""
        with self.assertRaises(TypeError):
            benchmark_local_inference.collect_thermal_metadata(ipc_thermal_state="normal")
        meta = benchmark_local_inference.collect_thermal_metadata(pmset_raw="anything")
        self.assertIsNone(meta["thermal_state"])
        self.assertIsNone(meta["thermal_source"])
        self.assertTrue(meta["thermal_unavailable_reason"])

if __name__ == "__main__":
    unittest.main()
