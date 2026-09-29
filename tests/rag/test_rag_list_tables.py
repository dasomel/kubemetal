import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "rag"))
from rag_host import lancedb_table_names


class FakeListTablesResponse:
    """Stands in for lancedb 0.34.0's pydantic `ListTablesResponse`."""

    def __init__(self, tables):
        self.tables = tables


class LanceDbTableNamesTests(unittest.TestCase):
    def test_plain_list_shape_is_returned_as_is(self):
        # Older lancedb: list_tables() returns list[str] directly.
        self.assertEqual(lancedb_table_names(["default", "test_col"]), ["default", "test_col"])

    def test_response_object_shape_unwraps_tables_attribute(self):
        # Measured on this Mac with lancedb 0.34.0: list_tables() returns a
        # ListTablesResponse whose `in` check against the object itself is
        # always False, which silently reported every collection as missing.
        response = FakeListTablesResponse(["default", "test_col"])
        self.assertEqual(lancedb_table_names(response), ["default", "test_col"])
        self.assertIn("default", lancedb_table_names(response))

    def test_empty_response_object_yields_empty_list(self):
        self.assertEqual(lancedb_table_names(FakeListTablesResponse([])), [])


if __name__ == "__main__":
    unittest.main()
