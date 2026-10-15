import io
import json
import sys
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "mlx"))
import finetune_wrapper


class FineTuneStartupTests(unittest.TestCase):
    def run_wrapper(self, lookup_error=None, child_returncode=0, checkpoint=False):
        requests = []
        events_at_params = []
        output = io.StringIO()

        def request(req, timeout):
            body = json.loads(req.data) if req.data is not None else None
            path = req.full_url.split("5001", 1)[1]
            requests.append((req.method, path, body))
            if "get-by-name" in path:
                if lookup_error is not None:
                    raise lookup_error
                response = {"experiment": {"experiment_id": "experiment-142"}}
            elif path.endswith("experiments/create"):
                response = {"experiment_id": "experiment-142"}
            elif path.endswith("runs/create"):
                response = {"run": {"info": {"run_id": "run-142"}}}
            else:
                response = {}
                if body and "params" in body:
                    events_at_params.extend(json.loads(line) for line in output.getvalue().splitlines())
            result = Mock()
            result.__enter__ = Mock(return_value=result)
            result.__exit__ = Mock(return_value=False)
            result.read.return_value = json.dumps(response).encode()
            return result

        lines = "Iter 1: Train loss 2.5\n" + ("Iter 1: Saved adapter weights to ignored and ignored.\n" if checkpoint else "")
        child = Mock(stdout=io.StringIO(lines),
                     stderr=io.StringIO("training failed"), returncode=child_returncode)
        argv = ["finetune_wrapper.py", "--model", "test-model", "--data", "test-data",
                "--iters", "1", "--batch-size", "1", "--learning-rate", "0.0001",
                "--adapter-name", "test-adapter"]
        def spawn_child(*args, **kwargs):
            if checkpoint:
                Path(home, "0000001_adapters.safetensors").write_bytes(b"saved weights")
            return child

        with tempfile.TemporaryDirectory() as home, \
                patch.object(sys, "argv", argv + ["--output-dir", home]), \
                patch.object(finetune_wrapper.Path, "home", return_value=Path(home)), \
                patch("urllib.request.urlopen", side_effect=request), \
                patch.object(finetune_wrapper.subprocess, "Popen", side_effect=spawn_child) as spawn, \
                patch.object(sys, "stdout", output):
            result = finetune_wrapper.main()
        spawn.assert_called_once()
        self.assertEqual(spawn.call_args.args[0][3:5], ["mlx_lm", "lora"])
        self.assertIn("--save-every", spawn.call_args.args[0])
        self.assertEqual(result, child_returncode)
        return requests, [json.loads(line) for line in output.getvalue().splitlines()], events_at_params

    def test_existing_experiment_starts_run_and_training(self):
        requests, events, events_at_params = self.run_wrapper()
        self.assertIn("experiment_name=kubemetal-finetune", requests[0][1])
        self.assertEqual(requests[1][2]["experiment_id"], "experiment-142")
        self.assertEqual(events_at_params, [{"type": "mlflow_run_started", "run_id": "run-142"}])
        self.assertEqual([event["type"] for event in events], ["mlflow_run_started", "progress", "done"])
        self.assertEqual(requests[-2][2]["metrics"][0]["value"], 2.5)
        self.assertEqual(requests[-1][2]["status"], "FINISHED")

    def test_missing_experiment_is_created_before_run(self):
        error = urllib.error.HTTPError("http://127.0.0.1:5001", 404, "Not Found", {}, None)
        requests, events, _ = self.run_wrapper(error)
        self.assertEqual(requests[1][2], {"name": "kubemetal-finetune"})
        self.assertEqual(requests[2][2]["experiment_id"], "experiment-142")
        self.assertEqual(events[-1]["type"], "done")

    def test_unavailable_mlflow_warns_but_training_completes(self):
        errors = [urllib.error.URLError("connection refused"),
                  urllib.error.HTTPError("http://127.0.0.1:5001", 500, "Server Error", {}, None)]
        for error in errors:
            with self.subTest(error=error):
                requests, events, _ = self.run_wrapper(error)
                self.assertEqual(len(requests), 1)
                self.assertEqual([event["type"] for event in events], ["warning", "progress", "done"])
                self.assertIn("MLflow:", events[0]["message"])

    def test_save_completed_line_emits_checkpoint_after_file_exists(self):
        _, events, _ = self.run_wrapper(checkpoint=True)
        self.assertEqual([e["type"] for e in events], ["mlflow_run_started", "progress", "checkpoint", "done"])
        self.assertEqual(events[2], {"type": "checkpoint", "iter": 1})

    def test_child_failure_marks_run_failed(self):
        requests, events, _ = self.run_wrapper(child_returncode=1)
        self.assertEqual(requests[-1][2]["status"], "FAILED")
        self.assertEqual(events[-1], {"type": "error", "message": "training failed"})


if __name__ == "__main__":
    unittest.main()
