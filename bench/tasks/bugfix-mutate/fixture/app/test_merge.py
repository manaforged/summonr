import unittest
from merge import merged

class TestMerge(unittest.TestCase):
    def test_does_not_mutate_base(self):
        base = {"a": 1}
        out = merged(base, {"b": 2})
        self.assertEqual(out, {"a": 1, "b": 2})
        self.assertEqual(base, {"a": 1})
